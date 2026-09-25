import { mkdir, readFile, writeFile, rm, cp } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
const root = new URL("../../", import.meta.url);
const selection = JSON.parse(await readFile(new URL("provenance/modes.json", root), "utf8"));
const apiWasm = JSON.parse(await readFile(new URL(selection.apiWasm, root), "utf8"));
if (apiWasm.schema !== 1 || apiWasm.apiVersion !== 1)
  throw Error("Unsupported Tapirscan WASM manifest");
const versions = JSON.parse(
  await readFile(new URL("demo/src/lib/scanner-versions.json", root), "utf8"),
);
const current = versions.versions.find((entry) => entry.version === versions.default);
if (JSON.stringify(current?.modes) !== JSON.stringify(apiWasm.modes))
  throw Error("Demo default version differs from the selected scanner build");
const assets = versions.versions.flatMap((entry) =>
  entry.modes.map(({ file, sha256 }) => [file, sha256]),
);
const turbo = JSON.parse(await readFile(new URL("demo/src/lib/turbo.json", root), "utf8"));
const experimental = [turbo, ...(turbo.previous ?? []), ...(turbo.variants ?? [])];
assets.push(...experimental.map(({ file, sha256 }) => [file, sha256]));
async function archivedEngine(file) {
  const failures = [];
  for (const host of [
    "tapirscan.netlify.app",
    "tapirscan.f-kleinicke.de",
    "tapirscan.netlify.app",
  ]) {
    try {
      const response = await fetch(`https://${host}/engines/${file}`, {
        signal: AbortSignal.timeout(30_000),
      });
      if (!response.ok) throw Error(`HTTP ${response.status}`);
      return Buffer.from(await response.arrayBuffer());
    } catch (error) {
      failures.push(error);
    }
  }
  throw new AggregateError(failures, `Cannot fetch archived engine ${file}`);
}

const loaded = [];
// Avoid a burst of archive requests on fresh CI runners. Every byte remains hash-pinned.
for (const [file, hash] of assets) {
  const local = new URL(
    (experimental.some((entry) => entry.file === file)
      ? "build/demo-experiments/"
      : "bindings/javascript/wasm/") + file,
    root,
  );
  let bytes;
  try {
    bytes = await readFile(local);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
    bytes = await archivedEngine(file);
  }
  if (createHash("sha256").update(bytes).digest("hex") !== hash)
    throw Error(`Engine hash mismatch: ${file}`);
  loaded.push([file, bytes]);
}
const dest = new URL("public/engines/", new URL("../", import.meta.url));
await rm(dest, { recursive: true, force: true });
await mkdir(dest, { recursive: true });
for (const [file, bytes] of loaded) await writeFile(new URL(file, dest), bytes);
execFileSync("python3", [fileURLToPath(new URL("scripts/prepare_demo.py", root))], {
  stdio: "inherit",
});

for (const [name, path] of [
  ["zxing_reader.wasm", "zxing-wasm/dist/reader/zxing_reader.wasm"],
  ["zbar.wasm", "@undecaf/zbar-wasm/dist/zbar.wasm"],
]) {
  await writeFile(new URL(name, dest), await readFile(new URL("demo/node_modules/" + path, root)));
}

// Serve PDF.js support files locally; never use an external font/CDN endpoint.
const pdfDest = new URL("public/pdf/", new URL("../", import.meta.url));
await rm(pdfDest, { recursive: true, force: true });
for (const dir of ["cmaps", "standard_fonts", "wasm"]) {
  await cp(new URL(`demo/node_modules/pdfjs-dist/${dir}/`, root), new URL(`${dir}/`, pdfDest), {
    recursive: true,
  });
}

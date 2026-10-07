import { mkdir, readdir, readFile, writeFile, rm, cp } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { releases } from "./releases.mjs";
import { sourceDigest } from "../../bindings/javascript/scripts/verify-package.mjs";
await import("./build-docs.mjs");
const root = new URL("../../", import.meta.url);
const demo = new URL("../", import.meta.url);
const modes = ["low", "medium", "high", "very-high"];
const presets = [2, 4, 8, 16];

// engines/<version>/<mode>.wasm for each npm release, engines/next/ for the
// repository's current build. Every file is verified by npm's lockfile
// integrity (releases) or by the build manifest (next).
const dest = new URL("public/engines/", demo);
await rm(dest, { recursive: true, force: true });
for (const { version, alias } of releases()) {
  const folder = new URL(`node_modules/${alias}/wasm/`, demo);
  let names;
  try {
    names = await readdir(folder);
  } catch (error) {
    if (error.code === "ENOENT")
      throw Error(`Run pnpm install: ${alias} is missing`, { cause: error });
    throw error;
  }
  await mkdir(new URL(`${version}/`, dest), { recursive: true });
  for (const mode of modes) {
    const matches = names.filter((name) => name === `${mode}.wasm` || name.startsWith(`${mode}-`));
    if (matches.length !== 1)
      throw Error(`Tapirscan ${version} must ship exactly one ${mode} engine: ${matches}`);
    await cp(new URL(matches[0], folder), new URL(`${version}/${mode}.wasm`, dest));
  }
}

const library = new URL("bindings/javascript/", root);
const build = new URL("wasm/build.json", library);
let manifest;
try {
  manifest = JSON.parse(await readFile(build, "utf8"));
} catch (error) {
  if (error.code === "ENOENT")
    throw Error("Build the library first: python3 scripts/build_wasm.py", { cause: error });
  throw error;
}
if (manifest.sourceDigest !== (await sourceDigest()))
  throw Error("Library WASM files are stale: run python3 scripts/build_wasm.py");
await readFile(new URL("dist/index.js", library)).catch((error) => {
  throw Error("Build the JavaScript package: npm run build --prefix bindings/javascript", {
    cause: error,
  });
});
await mkdir(new URL("next/", dest), { recursive: true });
for (const name of [...modes, ...presets.map((preset) => `experimental-turbo${preset}`)]) {
  const bytes = await readFile(new URL(`wasm/${name}.wasm`, library));
  if (createHash("sha256").update(bytes).digest("hex") !== manifest.files[`${name}.wasm`]?.sha256)
    throw Error(`Library WASM does not match build.json: ${name}`);
  await writeFile(new URL(`next/${name}.wasm`, dest), bytes);
}
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

// A stale precomputed benchmark must never masquerade as results for new engine bytes.
const { writeBenchmarkIdentity } = await import("./benchmark-identity.mjs");
await writeBenchmarkIdentity();

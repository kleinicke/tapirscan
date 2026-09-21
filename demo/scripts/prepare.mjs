import { mkdir, readFile, writeFile, rm, cp } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
const root = new URL("../../", import.meta.url);
const modes = JSON.parse(await readFile(new URL("provenance/modes.json", root), "utf8")).modes;
const extra = JSON.parse(
  await readFile(new URL("bindings/javascript/wasm/multiformat.json", root), "utf8"),
);
const assets = [
  ...modes.map((m) => [m.tag + ".wasm", m.binarySha256]),
  ["multiformat.wasm", extra.sha256],
];
const loaded = await Promise.all(
  assets.map(async ([file, hash]) => {
    const bytes = await readFile(new URL("bindings/javascript/wasm/" + file, root));
    if (createHash("sha256").update(bytes).digest("hex") !== hash)
      throw Error(`Engine hash mismatch: ${file}`);
    return [file, bytes];
  }),
);
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

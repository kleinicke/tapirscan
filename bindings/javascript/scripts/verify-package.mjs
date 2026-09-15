// Refuse to pack an incomplete or stale scanner distribution.
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
const root = new URL("../../../", import.meta.url);
const pkg = new URL("../", import.meta.url);
const { modes } = JSON.parse(await readFile(new URL("provenance/modes.json", root), "utf8"));
const source = await readFile(new URL("src/index.ts", pkg), "utf8");
const compiled = await readFile(new URL("dist/index.js", pkg), "utf8");
for (const { tag, binarySha256 } of modes) {
  if (!source.includes(tag) || !compiled.includes(tag)) throw Error(`Stale mode selection: ${tag}`);
  const bytes = await readFile(new URL(`wasm/${tag}.wasm`, pkg));
  if (createHash("sha256").update(bytes).digest("hex") !== binarySha256)
    throw Error(`WASM does not match its recipe: ${tag}`);
}
const extra = JSON.parse(await readFile(new URL("wasm/multiformat.json", pkg), "utf8"));
const bytes = await readFile(new URL("wasm/multiformat.wasm", pkg));
if (createHash("sha256").update(bytes).digest("hex") !== extra.sha256)
  throw Error("Additional-reader WASM hash mismatch");
for (const file of [
  "dist/index.d.ts",
  "dist/detail.js",
  "dist/detail-canvas.js",
  "dist/detail-20260914/scanner.mjs",
  "THIRD_PARTY_NOTICES.md",
])
  await readFile(new URL(file, pkg));
console.log("Package inputs verified: four modes, recovery runtime, additional reader and notices");

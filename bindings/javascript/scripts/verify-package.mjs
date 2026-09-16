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
const sourceManifest = await readFile(new URL("provenance/import.json", root));
const { releaseRevision, files } = JSON.parse(sourceManifest);
const sourceDigest = createHash("sha256").update(sourceManifest);
if (releaseRevision) {
  const revision = await readFile(new URL(releaseRevision, root));
  sourceDigest.update(revision);
  Object.assign(files, JSON.parse(revision).targetHashes);
}
for (const [file, hash] of Object.entries(files)) {
  if (!file.startsWith("multiformat/")) continue;
  const contents = await readFile(new URL(file, root));
  if (createHash("sha256").update(contents).digest("hex") !== hash)
    throw Error(`Additional-reader source drift: ${file}`);
}
if (sourceDigest.digest("hex") !== extra.sourceDigest)
  throw Error("Additional-reader WASM was built from a different source revision");
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

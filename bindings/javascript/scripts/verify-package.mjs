import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

const root = new URL("../../../", import.meta.url);
const pkg = new URL("../", import.meta.url);
const selection = JSON.parse(await readFile(new URL("provenance/modes.json", root), "utf8"));
const runtime = JSON.parse(await readFile(new URL(selection.runtimeRevision, root), "utf8"));
for (const [file, hash] of Object.entries(runtime.files)) {
  const bytes = await readFile(new URL(file, root));
  if (createHash("sha256").update(bytes).digest("hex") !== hash)
    throw Error(`Release runtime source drift: ${file}`);
}
if (typeof selection.apiWasm !== "string") throw Error("Missing public Rust WASM selection");
const manifest = JSON.parse(await readFile(new URL(selection.apiWasm, root), "utf8"));
if (manifest.schema !== 1 || manifest.apiVersion !== 1)
  throw Error("Unsupported public Rust WASM ABI");

const sourceFiles = Object.entries(manifest.sourceFiles).sort(([a], [b]) =>
  a < b ? -1 : a > b ? 1 : 0,
);
for (const [file, hash] of sourceFiles) {
  const bytes = await readFile(new URL(file, root));
  if (createHash("sha256").update(bytes).digest("hex") !== hash)
    throw Error(`Public Rust WASM source drift: ${file}`);
}
if (
  createHash("sha256").update(JSON.stringify(sourceFiles)).digest("hex") !== manifest.sourceDigest
)
  throw Error("Public Rust WASM source digest mismatch");

const source = await readFile(new URL("src/index.ts", pkg), "utf8");
const compiled = await readFile(new URL("dist/index.js", pkg), "utf8");
const expectedModes = ["low", "medium", "high", "very-high"];
if (
  !Array.isArray(manifest.modes) ||
  manifest.modes.length !== expectedModes.length ||
  expectedModes.some((mode) => !manifest.modes.some((entry) => entry.mode === mode))
)
  throw Error("Public Rust WASM manifest must contain all four modes exactly once");
const packageMetadata = JSON.parse(await readFile(new URL("package.json", pkg), "utf8"));
const packedWasm = packageMetadata.files.filter((file) => file.endsWith(".wasm"));
if (
  packedWasm.length !== manifest.modes.length ||
  manifest.modes.some(({ file }) => !packedWasm.includes(`wasm/${file}`))
)
  throw Error("Package file list must include exactly the selected public WASM assets");
for (const { mode, file, sha256, bytes } of manifest.modes) {
  if (!source.includes(file) || !compiled.includes(file))
    throw Error(`Stale ${mode} WASM selection`);
  const binary = await readFile(new URL(`wasm/${file}`, pkg));
  if (binary.byteLength !== bytes || createHash("sha256").update(binary).digest("hex") !== sha256)
    throw Error(`Public Rust WASM does not match its manifest: ${mode}`);
}
for (const file of [
  "dist/index.d.ts",
  "dist/index.js",
  "dist/rust-session.js",
  "THIRD_PARTY_NOTICES.md",
])
  await readFile(new URL(file, pkg));
console.log("Package inputs verified: public Rust API WASM in all four modes and notices");

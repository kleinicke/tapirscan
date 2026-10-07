import { createHash } from "node:crypto";
import { readdir, readFile, realpath } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const root = new URL("../../../", import.meta.url);
const pkg = new URL("../", import.meta.url);
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

// Mirrors scripts/build_wasm.py source_files(); the packaging test compares both.
const folders = [
  "core/src",
  "multiformat",
  "bindings/rust",
  "bindings/wasm",
  "tools/package-source",
];
const suffixes = [".rs", ".toml", ".in", ".lock"];
const files = [
  "core/Cargo.toml",
  "core/Cargo.lock",
  "scripts/build.py",
  "scripts/prepare_rust.py",
  "scripts/build_wasm.py",
  "scripts/build_turbo.py",
  "scripts/wasm_rustc.py",
  "config/formats.json",
  "config/modes.json",
];

async function* walk(folder) {
  for (const entry of await readdir(new URL(`${folder}/`, root), { withFileTypes: true })) {
    if (entry.name === "target") continue;
    const path = `${folder}/${entry.name}`;
    if (entry.isDirectory()) yield* walk(path);
    else if (entry.isFile() && suffixes.some((suffix) => entry.name.endsWith(suffix))) yield path;
  }
}

export async function sourceDigest() {
  const names = new Set(files);
  for (const folder of folders) for await (const path of walk(folder)) names.add(path);
  const entries = [];
  for (const name of [...names].sort())
    entries.push([name, sha256(await readFile(new URL(name, root)))]);
  return sha256(JSON.stringify(entries));
}

const expectedModes = ["low", "medium", "high", "very-high"];
const presets = [2, 4, 8, 16];
const expectedFiles = [
  ...expectedModes.map((mode) => ({ name: mode, mode })),
  ...presets.map((preset) => ({ name: `experimental-turbo${preset}`, mode: "low", preset })),
];

export async function verifyPackage() {
  const manifest = JSON.parse(await readFile(new URL("wasm/build.json", pkg), "utf8"));
  if (manifest.schema !== 2) throw Error("Unsupported WASM build manifest");
  if ((await sourceDigest()) !== manifest.sourceDigest)
    throw Error("WASM files are stale: rebuild them with scripts/build_wasm.py");

  const packageMetadata = JSON.parse(await readFile(new URL("package.json", pkg), "utf8"));
  const listed = Object.keys(manifest.files).sort();
  if (
    JSON.stringify(listed) !==
    JSON.stringify(expectedFiles.map(({ name }) => `${name}.wasm`).sort())
  )
    throw Error("WASM build manifest must list the four modes and Turbo presets 2, 4, 8 and 16");
  const packedWasm = packageMetadata.files.filter((file) => file.endsWith(".wasm"));
  if (
    packedWasm.length !== expectedFiles.length ||
    expectedFiles.some(({ name }) => !packedWasm.includes(`wasm/${name}.wasm`)) ||
    !packageMetadata.files.includes("wasm/build.json")
  )
    throw Error("Package file list must include exactly the public WASM files and build.json");

  const source = await readFile(new URL("src/index.ts", pkg), "utf8");
  const compiled = await readFile(new URL("dist/index.js", pkg), "utf8");
  const worker = await readFile(new URL("dist/browser-worker.js", pkg), "utf8");
  for (const { name, mode, preset } of expectedFiles) {
    const file = `${name}.wasm`;
    const record = manifest.files[file];
    if (!source.includes(file) || !compiled.includes(file))
      throw Error(`Missing ${name} selection`);
    // tapirscan/browser needs literal URLs so bundlers emit the assets.
    if (preset === undefined && !worker.includes(`new URL("../wasm/${file}", import.meta.url)`))
      throw Error(`Missing ${name} URL in the browser worker`);
    if (packageMetadata.exports[`./wasm/${file}`] !== `./wasm/${file}`)
      throw Error(`Missing WASM export: ${name}`);
    const binary = await readFile(new URL(`wasm/${file}`, pkg));
    if (binary.byteLength !== record.bytes || sha256(binary) !== record.sha256)
      throw Error(`WASM does not match build.json: ${name}`);
    const { instance } = await WebAssembly.instantiate(binary, {});
    if (
      instance.exports.tapirscan_experimental_turbo() !== (preset ?? 0) ||
      instance.exports.tapirscan_mode() !== expectedModes.indexOf(mode)
    )
      throw Error(`WASM compiled selection does not match its name: ${name}`);
  }
  for (const file of [
    "dist/index.d.ts",
    "dist/index.js",
    "dist/browser.d.ts",
    "dist/browser.js",
    "dist/browser-worker.js",
    "dist/freeze.js",
    "dist/rust-session.js",
    "THIRD_PARTY_NOTICES.md",
  ])
    await readFile(new URL(file, pkg));
  console.log(
    "Package inputs verified: WASM built from the current source in four stable modes and four experimental Turbo presets, plus notices",
  );
}

if (import.meta.url === pathToFileURL(await realpath(process.argv[1])).href) await verifyPackage();

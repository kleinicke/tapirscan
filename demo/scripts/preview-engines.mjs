// Publish a build of all four modes as the demo's TS-*-next readers.
// Usage: node scripts/preview-engines.mjs <assets-dir> <version> <label>
//   <assets-dir> holds {low,medium,high,very-high}.wasm, e.g. build/wasm-development/assets
//   <version>    immutable registry version, e.g. 1.2.2+segment-voting.20261005
// Copies the engines under immutable names, registers them (keeping the default)
// and points the next readers at them. Re-running with the same version is refused.
import { readFile, writeFile, copyFile, access } from "node:fs/promises";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

const [assets, version, label] = process.argv.slice(2);
if (!assets || !version?.includes("+") || !label)
  throw Error("Usage: preview-engines.mjs <assets-dir> <version with +tag> <label>");
const root = new URL("../../", import.meta.url);
const registryUrl = new URL("demo/src/lib/scanner-versions.json", root);
const registry = JSON.parse(await readFile(registryUrl, "utf8"));
if (registry.versions.some((entry) => entry.version === version))
  throw Error(`${version} is already registered; pick a new version`);

const suffix = version.split("+")[1].replaceAll(".", "-");
const sourceDigest = createHash("sha256");
const modes = [];
for (const mode of ["low", "medium", "high", "very-high"]) {
  const bytes = await readFile(resolve(assets, `${mode}.wasm`));
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const abi = instance.exports.tapirscan_abi_version();
  if (![1, 2].includes(abi)) throw Error(`${mode}.wasm has unsupported ABI ${abi}`);
  const file = `${mode}-${suffix}.wasm`;
  const target = new URL(`bindings/javascript/wasm/${file}`, root);
  if (
    await access(target).then(
      () => true,
      () => false,
    )
  )
    throw Error(`${file} already exists; engine files are immutable`);
  await copyFile(resolve(assets, `${mode}.wasm`), target);
  const sha256 = createHash("sha256").update(bytes).digest("hex");
  sourceDigest.update(sha256);
  modes.push({ mode, file, sha256, bytes: bytes.length });
}
const manifest = JSON.parse(
  await readFile(resolve(assets, "../manifest.json"), "utf8").catch(() => "{}"),
);
registry.versions.unshift({
  version,
  label,
  sourceDigest: manifest.sourceDigest ?? sourceDigest.digest("hex"),
  modes,
});
await writeFile(registryUrl, `${JSON.stringify(registry, null, 2)}\n`);

const comparisonUrl = new URL("demo/src/lib/comparison.ts", root);
const comparison = await readFile(comparisonUrl, "utf8");
const pattern = /^const nextRelease = ".*";$/m;
if (!pattern.test(comparison)) throw Error("nextRelease constant not found in comparison.ts");
await writeFile(comparisonUrl, comparison.replace(pattern, `const nextRelease = "${version}";`));
console.log(`Registered ${version}:`);
for (const { file, sha256 } of modes) console.log(`  ${file} ${sha256}`);

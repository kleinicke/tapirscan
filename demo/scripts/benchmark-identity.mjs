import { createHash } from "node:crypto";
import { readFile, readdir, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { releases } from "./releases.mjs";
export const examples = [
  "synthetic-barcode.png",
  "pesto.jpg",
  "pills.jpg",
  "sauce.jpg",
  "sunscreen.jpg",
];
const root = new URL("../", import.meta.url);
export async function benchmarkIdentity() {
  // Saved reports measure the main release: its host and engine bytes.
  const { version, alias } = releases().at(-1);
  const paths = [
    "src/lib/benchmark.ts",
    "src/lib/scan.worker.ts",
    "src/lib/reference.worker.ts",
    "package.json",
    `node_modules/${alias}/dist/index.js`,
    `node_modules/${alias}/dist/rust-session.js`,
    ...examples.map((name) => `public/images/${name}`),
    ...(await readdir(new URL(`public/engines/${version}/`, root)))
      .sort()
      .map((name) => `public/engines/${version}/${name}`),
    "public/engines/zxing_reader.wasm",
    "public/engines/zbar.wasm",
  ];
  const files = {};
  for (const path of paths)
    files[path] = createHash("sha256")
      .update(await readFile(new URL(path, root)))
      .digest("hex");
  return { identity: createHash("sha256").update(JSON.stringify(files)).digest("hex"), files };
}
export async function writeBenchmarkIdentity() {
  const manifest = await benchmarkIdentity();
  await mkdir(new URL("public/benchmarks/", root), { recursive: true });
  await writeFile(
    new URL("public/benchmarks/manifest.json", root),
    JSON.stringify(manifest, null, 2) + "\n",
  );
  return manifest;
}
if (process.argv[1] === fileURLToPath(import.meta.url)) await writeBenchmarkIdentity();

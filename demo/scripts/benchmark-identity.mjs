import { createHash } from "node:crypto";
import { readFile, readdir, mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
export const examples = [
  "synthetic-barcode.png",
  "pesto.jpg",
  "pills.jpg",
  "sauce.jpg",
  "sunscreen.jpg",
];
const root = new URL("../", import.meta.url);
export async function benchmarkIdentity() {
  const paths = [
    "src/lib/benchmark.ts",
    "src/lib/scan.worker.ts",
    "src/lib/scanner-versions.json",
    "src/lib/reference.worker.ts",
    "package.json",
    "../bindings/javascript/dist/index.js",
    "../bindings/javascript/dist/rust-session.js",
    "node_modules/tapirscan/dist/index.js",
    "node_modules/tapirscan/dist/rust-session.js",
    ...examples.map((name) => `public/images/${name}`),
    ...(await readdir(new URL("public/engines/", root)))
      .sort()
      .map((name) => `public/engines/${name}`),
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

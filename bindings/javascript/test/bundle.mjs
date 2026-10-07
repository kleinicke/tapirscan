// Usage: node test/bundle.mjs package.tgz
// Builds the Vite example against a packed tarball and checks that the production
// bundle contains the browser worker and every WASM file it can load.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cp, mkdtemp, readdir, readFile, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const [tarball] = process.argv.slice(2).map((path) => resolve(path));
assert.ok(tarball, "Supply the npm tarball");
const app = await realpath(await mkdtemp(join(tmpdir(), "tapirscan-vite-")));
try {
  await cp(new URL("../examples/vite/", import.meta.url), app, { recursive: true });
  const npm = (...args) => execFileSync("npm", args, { cwd: app, stdio: "inherit" });
  npm("install", "--no-audit", "--no-fund", tarball);
  npm("run", "build");
  const assets = await readdir(join(app, "dist/assets"));
  const worker = assets.find((name) => name.startsWith("browser-worker") && name.endsWith(".js"));
  assert.ok(worker, `No worker in the Vite build: ${assets.join(", ")}`);
  const source = await readFile(join(app, "dist/assets", worker), "utf8");
  const presets = [2, 4, 8, 16].map((preset) => `experimental-turbo${preset}`);
  for (const name of ["low", "medium", "high", "very-high", ...presets]) {
    const asset = assets.find((file) => file.startsWith(`${name}-`) && file.endsWith(".wasm"));
    assert.ok(asset, `Vite did not emit ${name}.wasm`);
    assert.ok(source.includes(asset), `The bundled worker does not reference ${asset}`);
  }
  console.log(`Vite bundled the worker and ${String(4 + presets.length)} WASM files.`);
} finally {
  await rm(app, { recursive: true, force: true });
}

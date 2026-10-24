// Usage: node test/bundle.mjs package.tgz
// Builds the Vite example against a packed tarball: tapirscan/browser/medium must
// emit its worker and only medium.wasm, and tapirscan/browser all eight WASM files.
// Serves the medium build and scans a barcode photo through the page in installed
// Google Chrome.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cp, mkdtemp, readdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fixture } from "./fixtures.mjs";
import { contentType, launchChrome, serve } from "./harness.mjs";

const [tarball] = process.argv.slice(2).map((path) => resolve(path));
assert.ok(tarball, "Supply the npm tarball");
const app = await realpath(await mkdtemp(join(tmpdir(), "tapirscan-vite-")));
const modes = [
  "low",
  "medium",
  "high",
  "very-high",
  ...[2, 4, 8, 16].map((n) => `experimental-turbo${n}`),
];
try {
  await cp(new URL("../examples/vite/", import.meta.url), app, { recursive: true });
  const npm = (...args) => execFileSync("npm", args, { cwd: app, stdio: "inherit" });
  npm("install", "--no-audit", "--no-fund", tarball);

  /** Build the app and return the WASM names its single worker references. */
  async function build() {
    npm("run", "build");
    const assets = await readdir(join(app, "dist/assets"));
    const workers = assets.filter(
      (name) => name.startsWith("browser-worker") && name.endsWith(".js"),
    );
    assert.equal(workers.length, 1, `Workers in the Vite build: ${assets.join(", ")}`);
    const source = await readFile(join(app, "dist/assets", workers[0]), "utf8");
    const wasm = assets.filter((file) => file.endsWith(".wasm"));
    for (const file of wasm)
      assert.ok(source.includes(file), `The worker does not reference ${file}`);
    // Vite names assets `<name>-<hash>.wasm`; hashes may contain "-".
    const named = modes.filter((name) => wasm.some((file) => file.startsWith(`${name}-`)));
    assert.equal(named.length, wasm.length, `Vite emitted ${wasm.join(", ")}`);
    return named;
  }

  assert.deepEqual(await build(), ["medium"]);
  await scanThroughPage(join(app, "dist"));
  const main = join(app, "main.js");
  const code = await readFile(main, "utf8");
  await writeFile(main, code.replace('"tapirscan/browser/medium"', '"tapirscan/browser"'));
  assert.deepEqual(await build(), modes);
  console.log(
    "Vite emitted only medium.wasm for tapirscan/browser/medium and all eight for tapirscan/browser.",
  );
} finally {
  await rm(app, { recursive: true, force: true });
}

/** Serve the production build and scan the test barcode as a PNG upload. */
async function scanThroughPage(dist) {
  const server = await serve(async (path, response) => {
    const file = path === "/" ? "/index.html" : path;
    const body = await readFile(join(dist, file));
    response.writeHead(200, { "content-type": contentType(file) }).end(body);
  });
  const browser = await launchChrome();
  try {
    const page = await browser.newPage();
    await page.goto(`${server.origin}/`);
    await page.waitForSelector("input:not([disabled])");
    const { text, image } = fixture();
    await page.evaluate(
      async ({ data, width, height }) => {
        const canvas = new OffscreenCanvas(width, height);
        const rgba = new Uint8ClampedArray(width * height * 4);
        data.forEach((value, i) =>
          rgba.fill(value, i * 4, i * 4 + 3).fill(255, i * 4 + 3, i * 4 + 4),
        );
        canvas.getContext("2d").putImageData(new ImageData(rgba, width), 0, 0);
        const photo = new File([await canvas.convertToBlob()], "barcode.png", {
          type: "image/png",
        });
        const input = document.querySelector("input");
        const files = new DataTransfer();
        files.items.add(photo);
        input.files = files.files;
        input.dispatchEvent(new Event("change"));
      },
      { data: [...image.data], width: image.width, height: image.height },
    );
    await page.waitForFunction(
      (expected) => document.querySelector('[role="status"]').textContent.includes(expected),
      text,
      { timeout: 60_000 },
    );
    console.log("The bundled page scanned an uploaded PNG in Chrome.");
  } finally {
    await browser.close();
    await server.close();
  }
}

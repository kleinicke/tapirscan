// Usage: node test/bundle.mjs package.tgz
// Builds the Vite example against a packed tarball, checks that the production
// bundle of tapirscan/browser/medium contains its worker and only medium.wasm, then
// serves the build and scans a barcode photo through the page in installed Google Chrome.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cp, mkdtemp, readdir, readFile, realpath, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";
import { chromium } from "playwright-core";
import { fixture } from "./fixtures.mjs";

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
  const wasm = assets.filter((file) => file.endsWith(".wasm"));
  assert.equal(wasm.length, 1, `Vite emitted ${wasm.join(", ")}`);
  assert.ok(wasm[0].startsWith("medium-"), `Vite emitted ${wasm[0]}`);
  assert.ok(source.includes(wasm[0]), `The bundled worker does not reference ${wasm[0]}`);
  console.log("Vite bundled the worker and only medium.wasm.");
  await scanThroughPage(join(app, "dist"));
} finally {
  await rm(app, { recursive: true, force: true });
}

/** Serve the production build and scan the test barcode as a PNG upload. */
async function scanThroughPage(dist) {
  const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm" };
  const server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    try {
      const body = await readFile(join(dist, path === "/" ? "index.html" : path));
      response.writeHead(200, { "content-type": types[extname(path || ".html")] ?? "text/html" });
      response.end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const browser = await chromium.launch(
    process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: "chrome" },
  );
  try {
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${String(server.address().port)}/`);
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
    server.close();
  }
}

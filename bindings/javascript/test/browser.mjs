// tapirscan/browser in real Chrome: worker startup, WASM loading and image sources.
// Uses the installed Google Chrome; set CHROME_PATH to use another Chromium build.
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { chromium } from "playwright-core";

const root = new URL("../", import.meta.url);
const types = { ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm" };
let server, browser, page;

before(async () => {
  server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    try {
      if (path === "/") {
        response
          .writeHead(200, { "content-type": "text/html" })
          .end("<!doctype html><title>t</title>");
        return;
      }
      const body = await readFile(new URL(`.${path}`, root));
      const type = types[path.slice(path.lastIndexOf("."))] ?? "application/octet-stream";
      response.writeHead(200, { "content-type": type }).end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  browser = await chromium.launch(
    process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: "chrome" },
  );
  page = await browser.newPage();
  page.on("pageerror", (error) => console.error(error));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  // Shared helpers: the EAN-13 fixture as a canvas, plus the module under test.
  await page.evaluate(async () => {
    const { fixture } = await import("/test/fixtures.mjs");
    const { text, image } = fixture();
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    const rgba = new Uint8ClampedArray(image.width * image.height * 4);
    image.data.forEach((value, i) =>
      rgba.fill(value, i * 4, i * 4 + 3).fill(255, i * 4 + 3, i * 4 + 4),
    );
    canvas.getContext("2d").putImageData(new ImageData(rgba, image.width), 0, 0);
    globalThis.fixture = { text, image, canvas, tapirscan: await import("/dist/browser.js") };
  });
});

after(async () => {
  await browser?.close();
  server?.close();
});

test("one-shot scan reads files and every image source kind", async () => {
  const read = await page.evaluate(async () => {
    const { image, canvas, tapirscan } = globalThis.fixture;
    const { scan } = tapirscan;
    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    const img = new Image();
    img.src = URL.createObjectURL(blob);
    await img.decode();
    const bitmap = await createImageBitmap(canvas);
    const context = canvas.getContext("2d");
    const sources = {
      blob,
      file: new File([blob], "code.png", { type: "image/png" }),
      img,
      canvas,
      bitmap,
      imageData: context.getImageData(0, 0, canvas.width, canvas.height),
      pixels: image,
    };
    const results = {};
    for (const [name, source] of Object.entries(sources))
      results[name] = (await scan(source)).values;
    // The caller's bitmap is copied before transfer, so it stays usable.
    results.bitmapUsable = bitmap.width === canvas.width;
    return results;
  });
  const expected = [await page.evaluate(() => globalThis.fixture.text)];
  for (const name of ["blob", "file", "img", "canvas", "bitmap", "imageData", "pixels"])
    assert.deepEqual(read[name], expected, name);
  assert.equal(read.bitmapUsable, true);
});

test("a reusable scanner queues concurrent scans and freezes results", async () => {
  const outcome = await page.evaluate(async () => {
    const { image, canvas, tapirscan } = globalThis.fixture;
    const { Scanner, best } = tapirscan;
    const scanner = new Scanner({ mode: "low", formats: ["EAN13", "QRCode"] });
    try {
      await scanner.ready;
      const reads = await Promise.all([
        scanner.scan(canvas),
        scanner.scan(image),
        scanner.scan(canvas),
      ]);
      const report = await scanner.inspect(canvas, { formats: "EAN13", extendedBudget: true });
      return {
        texts: reads.map((result) => result.values),
        best: best(reads[0].barcodes)?.text,
        frozen: Object.isFrozen(reads[0]) && Object.isFrozen(reads[0].barcodes[0].polygon),
        report: [report.mode, report.values, typeof report.unfinished, Boolean(report.diagnostics)],
      };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome.texts, [[text], [text], [text]]);
  assert.equal(outcome.best, text);
  assert.equal(outcome.frozen, true);
  assert.deepEqual(outcome.report, ["low", [text], "boolean", true]);
});

test("errors keep their types, and disposal rejects queued work", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas, tapirscan } = globalThis.fixture;
    const { Scanner, ScannerError } = tapirscan;
    const failure = async (promise) => {
      try {
        await promise;
        return "resolved";
      } catch (error) {
        return [error.constructor.name, error.message];
      }
    };
    const scanner = new Scanner();
    const results = {
      unknownOption: await failure((async () => new Scanner({ loadWasm: () => {} }))()),
      badMode: await failure(new Scanner({ mode: "ultra" }).ready),
      badModeScan: await failure(new Scanner({ mode: "ultra" }).scan(canvas)),
      notSubset: await failure(scanner.scan(canvas, { formats: "QRCode" })),
      tiny: await failure(scanner.scan(new ImageData(2, 2))),
      notImage: await failure(scanner.scan(null)),
      missingWasm: await failure(new Scanner({ wasmBaseUrl: "/missing/" }).ready),
    };
    const queued = scanner.scan(canvas);
    scanner.dispose();
    scanner.dispose();
    results.queued = await failure(queued);
    results.afterDispose = await failure(scanner.scan(canvas));
    results.scannerErrorExported = typeof ScannerError === "function";
    return results;
  });
  assert.match(outcome.unknownOption.join(" "), /^TypeError Unknown scanner option: loadWasm/);
  assert.equal(outcome.badMode[0], "TypeError");
  assert.deepEqual(outcome.badModeScan, outcome.badMode);
  assert.match(outcome.notSubset.join(" "), /^TypeError .*subset/);
  assert.equal(outcome.tiny[0], "TypeError");
  assert.deepEqual(outcome.notImage, ["TypeError", "Expected an image source"]);
  assert.match(outcome.missingWasm.join(" "), /^Error .*404/);
  assert.deepEqual(outcome.queued, ["Error", "Scanner was disposed"]);
  assert.deepEqual(outcome.afterDispose, ["Error", "Scanner was disposed"]);
  assert.equal(outcome.scannerErrorExported, true);
});

test("server rendering can import and construct without browser globals", async () => {
  // Node has no Worker, window or document, like a SvelteKit or Next.js server.
  const { Scanner, scan } = await import("../dist/browser.js");
  const scanner = new Scanner({ mode: "high" });
  await assert.rejects(scanner.ready, /scans in browsers/);
  await assert.rejects(
    scanner.scan({ data: new Uint8Array(9), width: 3, height: 3, channels: 1 }),
    /scans in browsers/,
  );
  scanner.dispose();
  await assert.rejects(scan(new Blob()), /scans in browsers/);
});

test("the worker's static WASM URLs match the packaged assets", async () => {
  const worker = await readFile(new URL("dist/browser-worker.js", root), "utf8");
  const packageJson = JSON.parse(await readFile(new URL("package.json", root), "utf8"));
  for (const mode of ["low", "medium", "high", "very-high"]) {
    const file = packageJson.exports[`./wasm/${mode}.wasm`].replace("./wasm/", "");
    assert.ok(worker.includes(`new URL("../wasm/${file}", import.meta.url)`), mode);
  }
});

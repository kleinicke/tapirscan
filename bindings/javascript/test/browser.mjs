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
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
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
    const offscreen = new OffscreenCanvas(canvas.width, canvas.height);
    offscreen.getContext("2d").drawImage(canvas, 0, 0);
    const frame = new VideoFrame(canvas, { timestamp: 0 });
    // A playing <video> fed by a live canvas stream, as a camera would feed it.
    // A canvas stream only emits frames while the canvas is redrawn.
    const live = document.createElement("canvas");
    live.width = canvas.width;
    live.height = canvas.height;
    const redraw = setInterval(() => live.getContext("2d").drawImage(canvas, 0, 0), 30);
    const video = document.createElement("video");
    video.muted = true;
    video.srcObject = live.captureStream(30);
    document.body.append(video);
    await video.play();
    await new Promise((resolve) => video.requestVideoFrameCallback(resolve));
    const sources = {
      blob,
      file: new File([blob], "code.png", { type: "image/png" }),
      img,
      canvas,
      bitmap,
      imageData: context.getImageData(0, 0, canvas.width, canvas.height),
      pixels: image,
      offscreen,
      frame,
      video,
    };
    const results = {};
    for (const [name, source] of Object.entries(sources))
      results[name] = (await scan(source)).values;
    // The caller's bitmap is copied before transfer, so it stays usable.
    results.bitmapUsable = bitmap.width === canvas.width;
    frame.close();
    clearInterval(redraw);
    video.srcObject.getTracks().forEach((track) => track.stop());
    video.remove();
    return results;
  });
  const expected = [await page.evaluate(() => globalThis.fixture.text)];
  const kinds = ["blob", "file", "img", "canvas", "bitmap", "imageData", "pixels"];
  for (const name of [...kinds, "offscreen", "frame", "video"])
    assert.deepEqual(read[name], expected, name);
  assert.equal(read.bitmapUsable, true);
});

test("a reusable scanner queues concurrent scans and freezes results", async () => {
  const outcome = await page.evaluate(async () => {
    const { image, canvas, tapirscan } = globalThis.fixture;
    const { Scanner } = tapirscan;
    const scanner = new Scanner({ mode: "low", formats: ["EAN13", "QRCode"] });
    try {
      await scanner.ready;
      const reads = await Promise.all([
        scanner.scan(canvas),
        scanner.scan(image),
        scanner.scan(canvas),
      ]);
      const report = await scanner.inspect(canvas, { formats: "EAN13" });
      return {
        texts: reads.map((result) => result.values),
        best: reads[0].best?.text,
        frozen: Object.isFrozen(reads[0]) && Object.isFrozen(reads[0].barcodes[0].polygon),
        report: [report.mode, report.values, Boolean(report.diagnostics)],
      };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome.texts, [[text], [text], [text]]);
  assert.equal(outcome.best, text);
  assert.equal(outcome.frozen, true);
  assert.deepEqual(outcome.report, ["low", [text], true]);
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
      badFormat: await failure(scanner.scan(canvas, { formats: "QR" })),
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
  assert.match(outcome.badFormat.join(" "), /^TypeError Unsupported barcode format or preset: QR/);
  assert.equal(outcome.tiny[0], "TypeError");
  assert.deepEqual(outcome.notImage, ["TypeError", "Expected an image source"]);
  assert.match(outcome.missingWasm.join(" "), /^Error .*404/);
  assert.deepEqual(outcome.queued, ["ScannerError", "Scanner was disposed"]);
  assert.deepEqual(outcome.afterDispose, ["ScannerError", "Scanner was disposed"]);
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
  const names = ["low", "medium", "high", "very-high", 2, 4, 8, 16].map((name) =>
    typeof name === "number" ? `experimental-turbo${name}` : name,
  );
  for (const mode of names) {
    const file = packageJson.exports[`./wasm/${mode}.wasm`].replace("./wasm/", "");
    assert.ok(worker.includes(`new URL("../wasm/${file}", import.meta.url)`), mode);
  }
});

test("browser scans snapshot reusable pixels before loading or yielding", async () => {
  const values = await page.evaluate(async () => {
    const { image, canvas, tapirscan } = globalThis.fixture;
    const scanner = new tapirscan.Scanner();
    try {
      const input = { ...image, data: image.data.slice() };
      const first = scanner.scan(input);
      input.data.fill(255);
      input.width = 3;
      await scanner.ready;
      const rgba = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height);
      const second = scanner.scan(rgba);
      rgba.data.fill(255);
      return [(await first).values, (await second).values];
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(values, [[text], [text]]);
});

test("invalid URLs allocate no worker and failed initialization terminates its worker", async () => {
  const outcome = await page.evaluate(async () => {
    const { Scanner } = globalThis.fixture.tapirscan;
    const OriginalWorker = globalThis.Worker;
    let created = 0,
      terminated = 0;
    globalThis.Worker = class extends OriginalWorker {
      constructor(...args) {
        super(...args);
        created++;
      }
      terminate() {
        terminated++;
        super.terminate();
      }
    };
    try {
      let invalid;
      try {
        new Scanner({ wasmBaseUrl: "http://[" });
      } catch (error) {
        invalid = error.name;
      }
      const afterInvalid = [created, terminated];
      const scanner = new Scanner({ wasmBaseUrl: "/missing/" });
      let failed = false;
      try {
        await scanner.ready;
      } catch {
        failed = true;
      }
      const afterFailure = [created, terminated];
      scanner.dispose();
      return { invalid, afterInvalid, failed, afterFailure, terminated };
    } finally {
      globalThis.Worker = OriginalWorker;
    }
  });
  assert.deepEqual(outcome, {
    invalid: "TypeError",
    afterInvalid: [0, 0],
    failed: true,
    afterFailure: [1, 1],
    terminated: 1,
  });
});

test("Turbo presets work in the browser entry", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas, tapirscan } = globalThis.fixture;
    const scanner = new tapirscan.Scanner({ experimentalTurbo: 2 });
    try {
      const report = await scanner.inspect(canvas);
      return { values: report.values, preset: report.experimentalTurbo, hasMode: "mode" in report };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome, { values: [text], preset: 2, hasMode: false });
});

test("scans capture their options and reach the worker in call order", async () => {
  const outcome = await page.evaluate(async () => {
    const { image, canvas, tapirscan } = globalThis.fixture;
    const OriginalWorker = globalThis.Worker;
    const sent = [];
    globalThis.Worker = class extends OriginalWorker {
      postMessage(message, transfer) {
        if (message.type !== "create")
          sent.push(message.source instanceof ImageBitmap ? "bitmap" : "pixels");
        super.postMessage(message, transfer);
      }
    };
    try {
      const scanner = new tapirscan.Scanner({ formats: ["EAN13", "QRCode"] });
      try {
        const options = { formats: "EAN13" };
        // The canvas needs an asynchronous bitmap snapshot; the pixels do not.
        const first = scanner.scan(canvas, options);
        options.formats = "QRCode";
        // A call that fails validation must not let later calls overtake earlier ones.
        const invalid = scanner.scan({ ...image, width: 1 }).catch((error) => error.name);
        const second = scanner.scan({ ...image, data: image.data.slice() });
        const results = await Promise.all([first, invalid, second]);
        return { sent, first: results[0].values, invalid: results[1] };
      } finally {
        scanner.dispose();
      }
    } finally {
      globalThis.Worker = OriginalWorker;
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome, { sent: ["bitmap", "pixels"], first: [text], invalid: "TypeError" });
});

test("disposal rejects scans that are still being prepared", async () => {
  const message = await page.evaluate(async () => {
    const { canvas, tapirscan } = globalThis.fixture;
    const scanner = new tapirscan.Scanner();
    const pending = scanner.scan(canvas);
    scanner.dispose();
    return pending.then(
      () => "resolved",
      (error) => `${error.name} ${error.code} ${error.message}`,
    );
  });
  assert.equal(message, "ScannerError disposed Scanner was disposed");
});

test("transparent images scan as if shown on white", async () => {
  const values = await page.evaluate(async () => {
    const { image, tapirscan } = globalThis.fixture;
    // Black bars on fully transparent pixels; the hidden RGB is black everywhere.
    const rgba = new Uint8ClampedArray(image.width * image.height * 4);
    image.data.forEach((value, i) => {
      rgba[i * 4 + 3] = value < 128 ? 255 : 0;
    });
    const canvas = document.createElement("canvas");
    canvas.width = image.width;
    canvas.height = image.height;
    canvas.getContext("2d").putImageData(new ImageData(rgba, image.width), 0, 0);
    return (await tapirscan.scan(canvas)).values;
  });
  assert.deepEqual(values, [await page.evaluate(() => globalThis.fixture.text)]);
});

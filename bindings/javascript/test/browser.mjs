// tapirscan/browser in real Chrome: worker startup, WASM loading and image sources.
// Uses the installed Google Chrome; set CHROME_PATH to use another Chromium build.
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { contentType, launchChrome, serve } from "./harness.mjs";

const root = new URL("../", import.meta.url);
let server, browser, page;
/** A Content-Security-Policy that every response carries while it is set. */
let csp = "";

before(async () => {
  server = await serve(async (path, response) => {
    const policy = csp ? { "content-security-policy": csp } : {};
    if (path === "/csp") {
      response.writeHead(200, { "content-type": "text/html", ...policy }).end("<title>t</title>");
      return;
    }
    if (path === "/") {
      response
        .writeHead(200, { "content-type": "text/html" })
        .end("<!doctype html><title>t</title>");
      return;
    }
    const body = await readFile(new URL(`.${path}`, root));
    response.writeHead(200, { "content-type": contentType(path), ...policy }).end(body);
  });
  browser = await launchChrome();
  page = await browser.newPage();
  page.on("pageerror", (error) => console.error(error));
  await page.goto(`${server.origin}/`);
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
  await server?.close();
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
    // The caller's bitmap is copied and its frame cloned before transfer, so both stay usable.
    results.bitmapUsable = bitmap.width === canvas.width;
    results.frameUsable = frame.displayWidth === canvas.width;
    // Videos and frames reach the worker as VideoFrames, without a bitmap copy.
    const OriginalWorker = globalThis.Worker;
    const sent = [];
    globalThis.Worker = class extends OriginalWorker {
      postMessage(message, transfer) {
        if (message.source) sent.push(message.source.constructor.name);
        super.postMessage(message, transfer);
      }
    };
    try {
      for (const source of [video, frame, canvas]) await scan(source);
    } finally {
      globalThis.Worker = OriginalWorker;
    }
    results.sent = sent;
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
  assert.equal(read.frameUsable, true);
  assert.deepEqual(read.sent, ["VideoFrame", "VideoFrame", "ImageBitmap"]);
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
      unknownGeneric: await failure((async () => new Scanner({ transferr: true }))()),
      badMode: await failure(new Scanner({ mode: "ultra" }).ready),
      badModeScan: await failure(new Scanner({ mode: "ultra" }).scan(canvas)),
      badFormat: await failure(scanner.scan(canvas, { formats: "QR" })),
      tiny: await failure(scanner.scan(new ImageData(2, 2))),
      notImage: await failure(scanner.scan(null)),
      missingWasm: await failure(new Scanner({ wasmBaseUrl: "/missing/" }).ready),
    };
    // Browser decoding errors keep their DOMException name.
    results.undecodable = await scanner
      .scan(new Blob(["not an image"]))
      .catch((error) => [error.constructor.name, error.name]);
    const queued = scanner.scan(canvas);
    scanner.dispose();
    scanner.dispose();
    results.queued = await failure(queued);
    results.afterDispose = await failure(scanner.scan(canvas));
    results.scannerErrorExported = typeof ScannerError === "function";
    return results;
  });
  assert.match(outcome.unknownOption.join(" "), /^TypeError Unknown scanner option: loadWasm/);
  assert.deepEqual(outcome.unknownGeneric, ["TypeError", "Unknown scanner option: transferr"]);
  assert.equal(outcome.badMode[0], "TypeError");
  assert.deepEqual(outcome.badModeScan, outcome.badMode);
  assert.match(outcome.badFormat.join(" "), /^TypeError Unsupported barcode format or preset: QR/);
  assert.equal(outcome.tiny[0], "TypeError");
  assert.deepEqual(outcome.notImage, ["TypeError", "Expected an image source"]);
  assert.match(outcome.missingWasm.join(" "), /^Error .*404/);
  assert.deepEqual(outcome.undecodable, ["Error", "InvalidStateError"]);
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
  // Relative URLs cannot be resolved on a server; other values still throw.
  new Scanner({ wasmBaseUrl: "/tapirscan/" }).dispose();
  assert.throws(() => new Scanner({ wasmBaseUrl: 5 }), TypeError);
});

test("construction survives a location getter that throws, as in Deno", async () => {
  // Deno without --location defines location but throws on reading it.
  const { Scanner } = await import("../dist/browser.js");
  Object.defineProperty(globalThis, "location", {
    configurable: true,
    get() {
      throw new ReferenceError("Access to location requires --location");
    },
  });
  try {
    const scanners = [new Scanner(), new Scanner({ wasmBaseUrl: "/tapirscan/" })];
    for (const scanner of scanners) {
      await assert.rejects(scanner.ready, /scans in browsers/);
      scanner.dispose();
    }
    assert.throws(() => new Scanner({ wasmBaseUrl: 5 }), TypeError);
  } finally {
    delete globalThis.location;
  }
});

test("single-mode entries reject mode and Turbo options", async () => {
  // The entry's worker fetches only medium.wasm.
  const wasm = [];
  const record = (request) => {
    if (request.url().endsWith(".wasm")) wasm.push(request.url().split("/").pop());
  };
  page.on("request", record);
  const outcome = await page.evaluate(async () => {
    const medium = await import("/dist/browser-medium.js");
    const attempt = (options) => {
      try {
        new medium.Scanner(options).dispose();
        return "ok";
      } catch (error) {
        return error.message;
      }
    };
    const scanner = new medium.Scanner();
    await scanner.ready;
    scanner.dispose();
    return {
      mode: attempt({ mode: "medium" }),
      turbo: attempt({ experimentalTurbo: 8 }),
    };
  });
  page.off("request", record);
  assert.equal(
    outcome.mode,
    'tapirscan/browser/medium has no mode option; use "tapirscan/browser"',
  );
  assert.match(outcome.turbo, /has no experimentalTurbo option/);
  assert.deepEqual(wasm, ["medium.wasm"]);
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

test("images above the working size decode in bands with the same result", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas: fixture, tapirscan } = globalThis.fixture;
    // 24x the fixture is above the 32-megapixel working size.
    const scale = 24;
    const large = new OffscreenCanvas(fixture.width * scale, fixture.height * scale);
    const context = large.getContext("2d");
    context.imageSmoothingEnabled = false;
    context.drawImage(fixture, 0, 0, large.width, large.height);
    const pixels = context.getImageData(0, 0, large.width, large.height);
    const blob = await large.convertToBlob({ type: "image/png" });
    const scanner = new tapirscan.Scanner();
    try {
      const fromPixels = await scanner.inspect(pixels);
      const fromBlob = await scanner.inspect(blob);
      const compactPixels = await scanner.scan(pixels);
      const compactBlob = await scanner.scan(blob);
      const strip = ({ elapsedMs: _elapsed, diagnostics: _diagnostics, ...rest }) => rest;
      // Diagnostic regions share the source coordinates of barcodes and undecoded regions.
      const { proposals, searchWindows } = fromBlob.diagnostics.regions;
      const points = [...proposals, ...searchWindows].flatMap(({ polygon }) => polygon);
      return {
        megapixels: (large.width * large.height) / 2 ** 20,
        same: JSON.stringify(strip(fromBlob)) === JSON.stringify(strip(fromPixels)),
        sameCompact:
          JSON.stringify(compactPixels.barcodes) === JSON.stringify(fromPixels.barcodes) &&
          JSON.stringify(compactBlob.barcodes) === JSON.stringify(fromBlob.barcodes),
        sameRegions:
          JSON.stringify(fromBlob.diagnostics.regions) ===
          JSON.stringify(fromPixels.diagnostics.regions),
        proposals: proposals.length,
        inside: points.every(
          ([x, y]) => x >= -1 && y >= -1 && x <= large.width + 1 && y <= large.height + 1,
        ),
        fullFrame: searchWindows
          .filter(({ kind }) => kind === "full_frame_search")
          .every(({ polygon }) => polygon[2][0] === large.width && polygon[2][1] === large.height),
        values: fromBlob.values,
      };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.ok(outcome.megapixels > 32, `${outcome.megapixels} MP`);
  assert.deepEqual(outcome.values, [text]);
  assert.ok(outcome.same, "banded Blob decoding changed the result");
  assert.ok(outcome.sameCompact, "compact scanning changed reduced-image barcodes");
  assert.ok(outcome.sameRegions, "banded Blob decoding changed the diagnostic regions");
  assert.ok(outcome.proposals > 0, "expected localization proposals");
  assert.ok(outcome.inside, "diagnostic regions outside the source image");
  assert.ok(outcome.fullFrame, "full-frame search window does not span the source image");
});

test("one-shot scan and inspect accept the per-call transfer option", async () => {
  const outcome = await page.evaluate(async () => {
    const { image, text, tapirscan } = globalThis.fixture;
    const whole = { ...image, data: image.data.slice() };
    const scanned = await tapirscan.scan(whole, { transfer: true, formats: "retail" });
    const detached = whole.data.byteLength;
    const again = { ...image, data: image.data.slice() };
    const inspected = await tapirscan.inspect(again, { transfer: true });
    let rejected = "";
    await tapirscan.scan(image, { transfer: "yes" }).catch((error) => (rejected = error.name));
    return { text, scanned: scanned.values, inspected: inspected.values, detached, rejected };
  });
  assert.deepEqual(outcome.scanned, [outcome.text]);
  assert.deepEqual(outcome.inspected, [outcome.text]);
  assert.equal(outcome.detached, 0);
  assert.equal(outcome.rejected, "TypeError");
});

test("transfer moves whole pixel buffers to the worker and copies partial views", async () => {
  const outcome = await page.evaluate(async () => {
    const { image, tapirscan } = globalThis.fixture;
    const scanner = new tapirscan.Scanner();
    try {
      const whole = { ...image, data: image.data.slice() };
      const call = scanner.scan(whole, { transfer: true, formats: "retail" });
      // Detached when the call starts, so later writes cannot reach the scan.
      const detachedAtCall = whole.data.byteLength;
      const moved = await call;
      // A view into a larger buffer cannot be moved without taking the rest along.
      const backing = new Uint8Array(image.data.length + 16);
      backing.set(image.data, 16);
      const view = { ...image, data: backing.subarray(16) };
      const copied = await scanner.scan(view, { transfer: true });
      let rejected = "";
      await scanner.scan(whole, { transfer: "yes" }).catch((error) => (rejected = error.name));
      return {
        moved: moved.values,
        detached: whole.data.byteLength,
        detachedAtCall,
        copied: copied.values,
        kept: backing.byteLength,
        rejected,
      };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome, {
    moved: [text],
    detached: 0,
    detachedAtCall: 0,
    copied: [text],
    kept: (await page.evaluate(() => globalThis.fixture.image.data.length)) + 16,
    rejected: "TypeError",
  });
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

test("a custom worker runs a single-mode entry's worker module", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas } = globalThis.fixture;
    const { Scanner } = await import("/dist/browser-medium.js");
    let invalid;
    try {
      new Scanner({ worker: "/dist/browser-worker-medium.js" });
    } catch (error) {
      invalid = error.name;
    }
    let spawned = 0;
    const scanner = new Scanner({
      wasmBaseUrl: "/wasm/",
      worker: () => {
        spawned++;
        return new Worker("/dist/browser-worker-medium.js", { type: "module" });
      },
    });
    // Without wasmBaseUrl, the worker module must bundle the selected preset.
    const { Scanner: AnyMode } = globalThis.fixture.tapirscan;
    const turbo = new AnyMode({
      experimentalTurbo: 8,
      worker: () => new Worker("/dist/browser-worker-medium.js", { type: "module" }),
    });
    const missing = await turbo.ready.catch((error) => error.message);
    turbo.dispose();
    try {
      const report = await scanner.inspect(canvas);
      return { invalid, spawned, values: report.values, mode: report.mode, missing };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome, {
    invalid: "TypeError",
    spawned: 1,
    values: [text],
    mode: "medium",
    missing: "This tapirscan/browser entry does not bundle the Turbo 8 preset",
  });
});

test("a worker that fails after loading is replaced on the next scan", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas } = globalThis.fixture;
    const { Scanner } = await import("/dist/browser-medium.js");
    const workers = [];
    const scanner = new Scanner({
      worker: () => {
        const worker = new Worker("/dist/browser-worker-medium.js", { type: "module" });
        workers.push(worker);
        return worker;
      },
    });
    // As when a browser stops the worker: it ends and reports an error.
    const kill = (worker) => {
      worker.terminate();
      worker.onerror(new ErrorEvent("error", { message: "worker stopped", cancelable: true }));
    };
    try {
      const first = (await scanner.scan(canvas)).values;
      // Stop the worker once it has received the next scan.
      const post = workers[0].postMessage.bind(workers[0]);
      workers[0].postMessage = (message, transfer) => {
        post(message, transfer);
        kill(workers[0]);
      };
      const failed = await scanner.scan(canvas).then(
        () => "resolved",
        (error) => error.message,
      );
      const queued = [scanner.scan(canvas), scanner.scan(canvas)];
      const replaced = (await Promise.all(queued)).map((result) => result.values);
      // The replacement fails the same way and is replaced again.
      kill(workers[1]);
      const again = (await scanner.scan(canvas)).values;
      await scanner.ready;
      return { first, failed, replaced, again, workers: workers.length };
    } finally {
      scanner.dispose();
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome, {
    first: [text],
    failed: "worker stopped",
    replaced: [[text], [text]],
    again: [text],
    workers: 3,
  });
});

test("an idle scanner warms up once loaded; one with a scan waiting does not", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas } = globalThis.fixture;
    const { Scanner } = await import("/dist/browser-medium.js");
    const OriginalWorker = globalThis.Worker;
    const sent = [];
    globalThis.Worker = class extends OriginalWorker {
      postMessage(message, transfer) {
        sent.push(message.type);
        super.postMessage(message, transfer);
      }
    };
    const settle = () => new Promise((resolve) => setTimeout(resolve, 50));
    try {
      const idle = new Scanner();
      await idle.ready;
      await settle();
      const values = (await idle.scan(canvas)).values;
      idle.dispose();
      const idleSent = sent.splice(0);
      // A scan requested before loading, or right after, skips the warm-up.
      const busy = new Scanner();
      await busy.scan(canvas);
      await settle();
      busy.dispose();
      const busySent = sent.splice(0);
      const prompt = new Scanner();
      await prompt.ready;
      await prompt.scan(canvas);
      await settle();
      prompt.dispose();
      return { idleSent, values, busySent, promptSent: sent.splice(0) };
    } finally {
      globalThis.Worker = OriginalWorker;
    }
  });
  const text = await page.evaluate(() => globalThis.fixture.text);
  assert.deepEqual(outcome, {
    idleSent: ["create", "warm", "scan"],
    values: [text],
    busySent: ["create", "scan"],
    promptSent: ["create", "scan"],
  });
});

test("a worker that fails while loading is not replaced", async () => {
  const outcome = await page.evaluate(async () => {
    const { canvas } = globalThis.fixture;
    const { Scanner } = await import("/dist/browser-medium.js");
    let spawned = 0;
    const scanner = new Scanner({
      worker: () => {
        spawned++;
        return new Worker("/missing-worker.js", { type: "module" });
      },
    });
    const errors = [];
    for (let i = 0; i < 3; i++)
      errors.push(await scanner.scan(canvas).catch((error) => error.message));
    scanner.dispose();
    return { spawned, same: new Set(errors).size, failed: errors[0].includes("failed to start") };
  });
  assert.deepEqual(outcome, { spawned: 1, same: 1, failed: true });
});

test("browsers without WebAssembly SIMD reject scanners without starting a worker", async () => {
  const outcome = await page.evaluate(async () => {
    const { Scanner } = globalThis.fixture.tapirscan;
    const { browserSupported } = await import("/dist/support.js");
    const { validate } = WebAssembly;
    const OriginalWorker = globalThis.Worker;
    let created = 0;
    globalThis.Worker = class extends OriginalWorker {
      constructor(...args) {
        super(...args);
        created++;
      }
    };
    WebAssembly.validate = () => false;
    try {
      const supported = browserSupported();
      const scanner = new Scanner();
      const [ready, scan] = await Promise.allSettled([
        scanner.ready,
        scanner.scan(globalThis.fixture.canvas),
      ]);
      scanner.dispose();
      const invalid = [5, "http://["].map((wasmBaseUrl) => {
        try {
          new Scanner({ wasmBaseUrl }).dispose();
          return "accepted";
        } catch (error) {
          return error.name;
        }
      });
      return {
        supported,
        created,
        ready: ready.reason?.code,
        scan: scan.reason?.code,
        name: ready.reason?.name,
        invalid,
      };
    } finally {
      WebAssembly.validate = validate;
      globalThis.Worker = OriginalWorker;
    }
  });
  assert.deepEqual(outcome, {
    supported: false,
    created: 0,
    ready: "unsupported",
    scan: "unsupported",
    name: "ScannerError",
    invalid: ["TypeError", "TypeError"],
  });
});

test("pages without module workers are unsupported", async () => {
  const outcome = await page.evaluate(async () => {
    const { Scanner } = globalThis.fixture.tapirscan;
    const OriginalWorker = globalThis.Worker;
    // Each query string loads a fresh copy, so the support check runs again.
    const supported = async (query) =>
      (await import(`/dist/support.js?${query}`)).browserSupported();
    try {
      // Firefox 105 to 113 start only classic workers and never read the type option.
      globalThis.Worker = class {
        constructor(url) {
          new URL(url);
        }
        terminate() {}
      };
      const classic = await supported("classic");
      delete globalThis.Worker;
      const scanner = new Scanner();
      const missing = await scanner.ready.catch((error) => error.code);
      scanner.dispose();
      globalThis.Worker = OriginalWorker;
      return { classic, missing, module: await supported("module") };
    } finally {
      globalThis.Worker = OriginalWorker;
    }
  });
  assert.deepEqual(outcome, { classic: false, missing: "unsupported", module: true });
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

test("Content-Security-Policy needs 'wasm-unsafe-eval'", async () => {
  const { text, image } = await page.evaluate(() => {
    const { text, image } = globalThis.fixture;
    return { text, image: { ...image, data: [...image.data] } };
  });
  const attempt = async (policy) => {
    csp = policy;
    const strict = await browser.newPage();
    try {
      await strict.goto(`${server.origin}/csp`);
      return await strict.evaluate(
        async ({ image }) => {
          const { scan } = await import("/dist/browser-medium.js");
          const pixels = { ...image, data: Uint8Array.from(image.data) };
          return scan(pixels).then(
            (result) => result.values.join(),
            (error) => error.message,
          );
        },
        { image },
      );
    } finally {
      await strict.close();
      csp = "";
    }
  };
  const base = "default-src 'self'; script-src 'self'";
  assert.equal(await attempt(`${base} 'wasm-unsafe-eval'`), text);
  assert.match(await attempt(base), /WebAssembly|Content Security Policy|unsafe-eval/i);
});

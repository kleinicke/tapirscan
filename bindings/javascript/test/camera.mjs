// tapirscan/camera and the single-mode browser entries in real browsers: installed
// Google Chrome (set CHROME_PATH for another Chromium) and Playwright's WebKit when
// it is installed. Cameras are faked: Chrome's fake device, or a canvas stream of
// the test barcode returned by a replaced getUserMedia.
import { test, before, after, describe } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { chromium, webkit } from "playwright-core";

const root = new URL("../", import.meta.url);
const types = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".wasm": "application/wasm",
};
let server, origin;

before(async () => {
  server = createServer(async (request, response) => {
    const path = new URL(request.url, "http://localhost").pathname;
    try {
      if (path === "/") {
        response
          .writeHead(200, { "content-type": "text/html" })
          .end("<!doctype html><title>t</title><video></video>");
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
  origin = `http://127.0.0.1:${server.address().port}`;
});

after(() => server?.close());

/** Page setup: the fixture as a redrawn canvas and a getUserMedia that streams it. */
async function preparePage(page) {
  await page.goto(`${origin}/`);
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
    const pixels = new ImageData(rgba, image.width);
    // A canvas stream emits frames only while the canvas is redrawn.
    setInterval(() => canvas.getContext("2d").putImageData(pixels, 0, 0), 30);
    const log = { requests: [], streams: [] };
    let delay = 0;
    const fake = async (constraints) => {
      log.requests.push(constraints);
      if (delay) await new Promise((resolve) => setTimeout(resolve, delay));
      const stream = canvas.captureStream(30);
      log.streams.push(stream);
      return stream;
    };
    Object.defineProperty(navigator, "mediaDevices", {
      configurable: true,
      value: { getUserMedia: fake, enumerateDevices: async () => [] },
    });
    globalThis.t = {
      text,
      log,

      setDelay(ms) {
        delay = ms;
      },
      video: document.querySelector("video"),
      camera: await import("/dist/camera.js"),
      medium: await import("/dist/browser-medium.js"),
      /** Resolve with the first scan result that `accept`s, or reject after `ms`. */
      until(register, accept, ms = 15000) {
        return new Promise((resolve, reject) => {
          const timer = setTimeout(() => reject(new Error("timed out")), ms);
          register((result) => {
            if (accept(result)) {
              clearTimeout(timer);
              resolve(result);
            }
          });
        });
      },
      live: (stream) => stream.getTracks().some((track) => track.readyState === "live"),
    };
  });
}

const browsers = [
  [
    "Chrome",
    () =>
      chromium.launch({
        ...(process.env.CHROME_PATH
          ? { executablePath: process.env.CHROME_PATH }
          : { channel: "chrome" }),
        args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"],
      }),
  ],
  ["WebKit", () => webkit.launch()],
];

for (const [name, launch] of browsers) {
  describe(`camera in ${name}`, () => {
    let browser, page, skip;
    before(async () => {
      try {
        browser = await launch();
      } catch (error) {
        if (name === "Chrome") throw error;
        skip = `${name} is not installed: ${error.message.split("\n")[0]}`;
        return;
      }
      page = await browser.newPage();
      page.on("pageerror", (error) => console.error(error));
      await preparePage(page);
    });
    after(() => browser?.close());

    test("scans a live stream and stops cleanly", async (context) => {
      if (skip) return context.skip(skip);
      const outcome = await page.evaluate(async () => {
        const { camera, medium, video, log, until, live } = globalThis.t;
        const scanner = new medium.Scanner({ formats: ["EAN13"] });
        let onScan;
        const errors = [];
        const controller = new camera.CameraScanner(video, scanner, {
          onScan: (result) => onScan?.(result),
          onError: (error) => errors.push(error.message),
        });
        const found = until(
          (callback) => (onScan = callback),
          (result) => result.barcodes.length > 0,
        );
        await controller.start();
        const result = await found;
        const running = controller.running;
        const hadTrack = controller.track?.readyState === "live";
        controller.stop();
        // Nothing arrives after stop, even from a scan that was in flight.
        let late = 0;
        onScan = () => late++;
        await new Promise((resolve) => setTimeout(resolve, 400));
        scanner.dispose();
        return {
          values: result.values,
          running,
          hadTrack,
          stoppedRunning: controller.running,
          released: !live(log.streams[0]) && video.srcObject === null,
          late,
          errors,
          constraints: log.requests[0].video,
        };
      });
      assert.deepEqual(outcome.values, [await page.evaluate(() => globalThis.t.text)]);
      assert.ok(outcome.running && outcome.hadTrack);
      assert.ok(!outcome.stoppedRunning && outcome.released);
      assert.equal(outcome.late, 0);
      assert.deepEqual(outcome.errors, []);
      assert.deepEqual(outcome.constraints.facingMode, { ideal: "environment" });
    });

    test("releases a camera granted after stop", async (context) => {
      if (skip) return context.skip(skip);
      const outcome = await page.evaluate(async () => {
        const { camera, medium, video, log, setDelay, live } = globalThis.t;
        const scanner = new medium.Scanner();
        const controller = new camera.CameraScanner(video, scanner);
        const before = log.streams.length;
        setDelay(300);
        const starting = controller.start();
        controller.stop();
        await starting;
        setDelay(0);
        scanner.dispose();
        const stream = log.streams[before];
        return { granted: Boolean(stream), live: stream && live(stream), track: controller.track };
      });
      assert.deepEqual(outcome, { granted: true, live: false, track: undefined });
    });

    test("switching cameras releases the old stream first", async (context) => {
      if (skip) return context.skip(skip);
      const outcome = await page.evaluate(async () => {
        const { camera, medium, video, log, live } = globalThis.t;
        const scanner = new medium.Scanner();
        const controller = new camera.CameraScanner(video, scanner);
        await controller.start();
        const first = log.streams.at(-1);
        await controller.start({ deviceId: "back-2" });
        const second = log.streams.at(-1);
        const result = {
          firstLive: live(first),
          secondLive: live(second),
          constraints: log.requests.at(-1).video.deviceId,
        };
        controller.stop();
        scanner.dispose();
        return result;
      });
      assert.deepEqual(outcome, {
        firstLive: false,
        secondLive: true,
        constraints: { exact: "back-2" },
      });
    });

    test("a disposed scanner or ended camera stops scanning with onError", async (context) => {
      if (skip) return context.skip(skip);
      const outcome = await page.evaluate(async () => {
        const { camera, medium, video, log, live } = globalThis.t;
        const run = async (fail) => {
          const scanner = new medium.Scanner();
          const failed = new Promise((resolve) => {
            const controller = new camera.CameraScanner(video, scanner, {
              onError: (error) => resolve({ message: error.message, controller }),
            });
            controller.start().then(() => fail(scanner, controller));
          });
          const { message, controller } = await failed;
          scanner.dispose();
          return { message, running: controller.running, live: live(log.streams.at(-1)) };
        };
        return {
          disposed: await run((scanner) => scanner.dispose()),
          // A track's ended event fires for ends the page did not request.
          ended: await run((scanner, controller) =>
            controller.track.dispatchEvent(new Event("ended")),
          ),
        };
      });
      assert.deepEqual(outcome.disposed, {
        message: "Scanner was disposed",
        running: false,
        live: false,
      });
      assert.deepEqual(outcome.ended, {
        message: "The camera stopped",
        running: false,
        live: false,
      });
    });

    test("pauses while the page is hidden and resumes when shown", async (context) => {
      if (skip) return context.skip(skip);
      const outcome = await page.evaluate(async () => {
        const { camera, medium, video, log, until, live } = globalThis.t;
        const scanner = new medium.Scanner();
        let onScan;
        const controller = new camera.CameraScanner(video, scanner, {
          onScan: (result) => onScan?.(result),
        });
        await controller.start();
        const first = log.streams.at(-1);
        let state = "hidden";
        Object.defineProperty(document, "visibilityState", {
          configurable: true,
          get: () => state,
        });
        document.dispatchEvent(new Event("visibilitychange"));
        const hidden = { live: live(first), running: controller.running, track: controller.track };
        state = "visible";
        const resumed = until(
          (callback) => (onScan = callback),
          (result) => result.barcodes.length > 0,
        );
        document.dispatchEvent(new Event("visibilitychange"));
        await resumed;
        const second = log.streams.at(-1);
        controller.stop();
        scanner.dispose();
        delete document.visibilityState;
        return { hidden, reopened: second !== first, secondReleased: !live(second) };
      });
      assert.deepEqual(outcome, {
        hidden: { live: false, running: true, track: undefined },
        reopened: true,
        secondReleased: true,
      });
    });

    test("start rejects when the camera is refused, without leaving it running", async (context) => {
      if (skip) return context.skip(skip);
      const outcome = await page.evaluate(async () => {
        const { camera, medium, video } = globalThis.t;
        const scanner = new medium.Scanner();
        const saved = navigator.mediaDevices;
        Object.defineProperty(navigator, "mediaDevices", {
          configurable: true,
          value: {
            getUserMedia: () =>
              Promise.reject(new DOMException("Permission denied", "NotAllowedError")),
          },
        });
        const controller = new camera.CameraScanner(video, scanner);
        let error;
        await controller.start().catch((cause) => (error = cause.name));
        Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: saved });
        scanner.dispose();
        return { error, running: controller.running };
      });
      assert.deepEqual(outcome, { error: "NotAllowedError", running: false });
    });

    test("single-mode entries reject other modes and Turbo", async (context) => {
      if (skip) return context.skip(skip);
      // The entry's worker fetches only medium.wasm.
      const wasm = [];
      const record = (request) => {
        if (request.url().endsWith(".wasm")) wasm.push(request.url().split("/").pop());
      };
      page.on("request", record);
      const outcome = await page.evaluate(async () => {
        const { medium } = globalThis.t;
        const attempt = (options) => {
          try {
            new medium.Scanner(options).dispose();
            return "ok";
          } catch (error) {
            return error.message;
          }
        };
        const scanner = new medium.Scanner({ mode: "medium" });
        await scanner.ready;
        scanner.dispose();
        return {
          high: attempt({ mode: "high" }),
          turbo: attempt({ experimentalTurbo: 8 }),
        };
      });
      assert.match(outcome.high, /tapirscan\/browser\/medium scans in medium mode/);
      assert.match(outcome.turbo, /has no Turbo presets/);
      page.off("request", record);
      assert.deepEqual(wasm, ["medium.wasm"]);
    });

    if (name === "Chrome")
      test("the camera example starts, scans and stops", async (context) => {
        if (skip) return context.skip(skip);
        const example = await browser.newPage();
        try {
          await example.goto(`${origin}/examples/camera.html`);
          await example.click("#start");
          // Chrome's fake camera shows no barcode, so scanned frames report [].
          await example.waitForFunction(() => document.querySelector("pre").textContent === "[]");
          await example.click("#stop");
          const state = await example.evaluate(() => ({
            stream: document.querySelector("video").srcObject,
            start: document.querySelector("#start").disabled,
          }));
          assert.deepEqual(state, { stream: null, start: false });
        } finally {
          await example.close();
        }
      });

    if (name === "Chrome")
      test("opens Chrome's fake camera with the default constraints", async (context) => {
        if (skip) return context.skip(skip);
        const outcome = await page.evaluate(async () => {
          const { camera, medium, video } = globalThis.t;
          const saved = navigator.mediaDevices;
          // Removing the page's override exposes the browser's own camera access.
          delete navigator.mediaDevices;
          const scanner = new medium.Scanner();
          let frames = 0;
          let resolveFrames;
          const scanned = new Promise((resolve) => (resolveFrames = resolve));
          const controller = new camera.CameraScanner(video, scanner, {
            onScan: () => ++frames === 3 && resolveFrames(),
          });
          await controller.start();
          await scanned;
          const track = controller.track;
          const settings = track.getSettings();
          const cameras = await camera.CameraScanner.cameras();
          const torch = await controller.setTorch(true);
          controller.stop();
          scanner.dispose();
          Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: saved });
          return {
            size: [settings.width, settings.height],
            ended: track.readyState,
            cameras: cameras.length,
            torch,
          };
        });
        assert.deepEqual(outcome.size, [1280, 720]);
        assert.equal(outcome.ended, "ended");
        assert.ok(outcome.cameras >= 1);
        assert.equal(outcome.torch, false);
      });
  });
}

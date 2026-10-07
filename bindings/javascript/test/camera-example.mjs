import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";
import { setImmediate } from "node:timers/promises";
import { runInNewContext } from "node:vm";

async function cameraExample() {
  const html = await readFile(new URL("../examples/camera.html", import.meta.url), "utf8");
  const source = html.match(/<script type="module">([\s\S]*?)<\/script>/)[1];
  const frames = [];
  const cameras = [];
  const scanners = [];
  const elements = {
    video: { requestVideoFrameCallback: (callback) => frames.push(callback) },
    pre: {},
    "#start": {},
    "#stop": {},
  };
  let pagehide;
  runInNewContext(source.replace(/import .*?;/, ""), {
    Scanner: class {
      calls = 0;
      constructor() {
        scanners.push(this);
      }
      scan() {
        this.calls++;
        return [];
      }
      dispose() {}
    },
    document: { querySelector: (selector) => elements[selector] },
    navigator: {
      mediaDevices: {
        getUserMedia: () => new Promise((resolve) => cameras.push(resolve)),
      },
    },
    window: { addEventListener: (_, callback) => (pagehide = callback) },
  });
  let stops = 0;
  return {
    elements,
    scanners,
    start: () => elements["#start"].onclick(),
    stop: () => elements["#stop"].onclick(),
    hide: () => pagehide(),
    grant: () => cameras.shift()({ getTracks: () => [{ stop: () => stops++ }] }),
    frame: () => frames.splice(0).forEach((callback) => callback()),
    stops: () => stops,
  };
}

test("camera restart cannot revive the previous scan loop", async () => {
  const camera = await cameraExample();
  const first = camera.start();
  camera.grant();
  await setImmediate();
  camera.stop();
  const second = camera.start();
  camera.grant();
  await setImmediate();
  camera.frame();
  await setImmediate();
  assert.deepEqual(
    camera.scanners.map((scanner) => scanner.calls),
    [0, 1],
  );
  camera.stop();
  camera.frame();
  await Promise.all([first, second]);
  assert.equal(camera.stops(), 2);
});

test("leaving while camera permission is pending stops a late stream", async () => {
  const camera = await cameraExample();
  const running = camera.start();
  camera.hide();
  camera.grant();
  await running;
  assert.equal(camera.stops(), 1);
  assert.equal(camera.elements.video.srcObject, null);
});

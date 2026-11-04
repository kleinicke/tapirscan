import { after, test } from "node:test";
import assert from "node:assert/strict";
import { CameraControls } from "../dist/camera.js";

const original = Object.getOwnPropertyDescriptor(globalThis, "navigator");
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: {
    mediaDevices: { getSupportedConstraints: () => ({ pointsOfInterest: true }) },
  },
});
after(() => {
  if (original) Object.defineProperty(globalThis, "navigator", original);
  else delete globalThis.navigator;
});

function camera() {
  let constraints = {
    width: { ideal: 1280 },
    focusMode: "manual",
    advanced: [{ zoom: 1, focusMode: "manual", exposureMode: "continuous" }],
  };
  const settings = { zoom: 1 };
  const calls = [];
  const track = {
    readyState: "live",
    getCapabilities: () => ({
      focusMode: ["continuous", "single-shot"],
      torch: [false, true],
      zoom: { min: 1, max: 4, step: 0.1 },
    }),
    getConstraints: () => structuredClone(constraints),
    getSettings: () => structuredClone(settings),
    async applyConstraints(next) {
      calls.push(structuredClone(next));
      constraints = structuredClone(next);
      for (const value of next.advanced ?? []) Object.assign(settings, value);
    },
  };
  return { track, calls, settings, controls: new CameraControls(track) };
}

test("construction preserves defaults; point focus, zoom and torch share one queue", async () => {
  const { controls, calls, settings } = camera();
  assert.equal(calls.length, 0);
  assert.equal(controls.pointFocusAvailable, true);
  const point = { x: 0.25, y: 0.75 };
  const focus = controls.focusAt(point);
  point.x = 1; // Caller's subsequent mutation cannot move the requested point.
  assert.deepEqual(await Promise.all([focus, controls.setZoom(2), controls.setTorch(true)]), [
    true,
    true,
    true,
  ]);
  assert.deepEqual(settings.pointsOfInterest, [{ x: 0.25, y: 0.75 }]);
  assert.equal(settings.focusMode, "continuous");
  assert.equal(settings.zoom, 2);
  assert.equal(settings.torch, true);
  assert.deepEqual(calls[2].width, { ideal: 1280 });
  assert.equal("focusMode" in calls[2], false);
  assert.equal(settings.exposureMode, "continuous");
  assert.equal(calls[0].advanced.filter((entry) => "focusMode" in entry).length, 1);
  assert.equal(await controls.setContinuousFocus(), true);
  assert.deepEqual(settings.pointsOfInterest, []);
  assert.equal(settings.zoom, 2);
  assert.equal(settings.torch, true);
});

test("reset queued behind point focus clears that new point", async () => {
  const { controls, settings } = camera();
  assert.deepEqual(
    await Promise.all([controls.focusAt({ x: 0, y: 1 }), controls.setContinuousFocus()]),
    [true, true],
  );
  assert.deepEqual(settings.pointsOfInterest, []);
});

test("single-shot fallback and unconfirmed requests report honestly", async () => {
  const { controls, track, settings } = camera();
  track.getCapabilities = () => ({ focusMode: ["single-shot"] });
  assert.equal(await controls.focusAt({ x: 0.5, y: 0.5 }), true);
  assert.equal(settings.focusMode, "single-shot");
  assert.equal(await controls.setContinuousFocus(), false);
  track.applyConstraints = async () => {};
  assert.equal(await controls.focusAt({ x: 0.8, y: 0.2 }), false);
});

test("unsupported cameras and missing browser controls are not offered", async () => {
  const { controls, track, calls } = camera();
  const supported = navigator.mediaDevices.getSupportedConstraints;
  try {
    navigator.mediaDevices.getSupportedConstraints = () => ({});
    assert.equal(controls.pointFocusAvailable, false);
    assert.equal(await controls.focusAt({ x: 0.5, y: 0.5 }), false);
    navigator.mediaDevices.getSupportedConstraints = supported;
    track.getCapabilities = () => ({ focusMode: ["manual"] });
    assert.equal(controls.pointFocusAvailable, false);
    track.getCapabilities = undefined;
    assert.equal(await controls.setContinuousFocus(), false);
    assert.equal(await controls.setZoom(2), false);
    assert.equal(calls.length, 0);
  } finally {
    navigator.mediaDevices.getSupportedConstraints = supported;
  }
});

test("invalid coordinates and zoom never reach the camera", async () => {
  const { controls, calls } = camera();
  for (const point of [
    null,
    {},
    { x: NaN, y: 0 },
    { x: 0, y: Infinity },
    { x: -0.1, y: 0.5 },
    { x: 0.5, y: 1.1 },
  ])
    await assert.rejects(controls.focusAt(point), RangeError);
  for (const value of [NaN, Infinity, 0, 5])
    await assert.rejects(controls.setZoom(value), RangeError);
  assert.equal(calls.length, 0);
});

test("device refusal does not poison the control queue", async () => {
  const { controls, track } = camera();
  const apply = track.applyConstraints;
  track.applyConstraints = async () => {
    throw new Error("refused");
  };
  await assert.rejects(controls.focusAt({ x: 0.5, y: 0.5 }), /refused/);
  track.applyConstraints = apply;
  assert.equal(await controls.setZoom(2), true);
});

for (const ending of ["dispose", "end"])
  test(`${ending} invalidates in-flight and queued requests without stopping track`, async () => {
    const { controls, track, calls } = camera();
    let release;
    track.applyConstraints = () =>
      new Promise((resolve) => {
        calls.push({});
        release = resolve;
      });
    const first = controls.focusAt({ x: 0.5, y: 0.5 });
    const queued = controls.setZoom(2);
    await Promise.resolve();
    if (ending === "dispose") controls.dispose();
    else track.readyState = "ended";
    release();
    assert.deepEqual(await Promise.all([first, queued]), [false, false]);
    assert.equal(calls.length, 1);
    assert.equal(controls.pointFocusAvailable, false);
    if (ending === "dispose") assert.equal(track.readyState, "live");
  });

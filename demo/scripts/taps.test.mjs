import assert from "node:assert/strict";
import { test } from "node:test";
import { DoubleTap } from "../src/lib/taps.ts";
const point = (timeStamp, clientX = 100, clientY = 100) => ({ timeStamp, clientX, clientY });
const tap = (tracker, time, x = 100) => {
  tracker.down(point(time, x));
  return tracker.up(point(time + 70, x));
};
test("two nearby touch taps recenter once despite finger jitter", () => {
  const tracker = new DoubleTap();
  assert.equal(tap(tracker, 0), false);
  tracker.down(point(180, 105));
  assert.equal(tracker.move(point(200, 109)), true);
  assert.equal(tracker.up(point(240, 110)), true);
  assert.equal(tap(tracker, 320), false);
});
test("distant taps, slow taps and long presses are not double taps", () => {
  for (const [time, x] of [
    [180, 200],
    [700, 100],
  ]) {
    const tracker = new DoubleTap();
    tap(tracker, 0);
    assert.equal(tap(tracker, time, x), false);
  }
  const tracker = new DoubleTap();
  tap(tracker, 0);
  tracker.down(point(100));
  assert.equal(tracker.up(point(450)), false);
});
test("drag, pinch cancellation and lost capture break the tap sequence", () => {
  const tracker = new DoubleTap();
  tap(tracker, 0);
  tracker.down(point(160));
  assert.equal(tracker.move(point(180, 130)), false);
  assert.equal(tracker.up(point(220)), false);
  assert.equal(tap(tracker, 280), false);
  tracker.cancel();
  assert.equal(tap(tracker, 380), false);
  tracker.down(point(500));
  tracker.cancel();
  assert.equal(tracker.up(point(540)), false);
});

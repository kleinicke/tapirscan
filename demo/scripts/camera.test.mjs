import assert from "node:assert/strict";
import { test } from "node:test";
import { scanDimensions } from "../src/lib/camera.ts";

test("photo scan limits preserve aspect ratio without upscaling and respect the scanner cap", () => {
  assert.deepEqual(scanDimensions(640, 480, 1920), [640, 480]);
  assert.deepEqual(scanDimensions(6000, 4000, 3840), [3840, 2560]);
  assert.deepEqual(scanDimensions(4000, 6000, 1920), [1280, 1920]);
  const [width, height] = scanDimensions(8000, 6000, Infinity);
  assert.ok(width * height <= 32 * 1024 * 1024);
});

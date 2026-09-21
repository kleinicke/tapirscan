import assert from "node:assert/strict";
import { test } from "node:test";
import { visibleResults } from "../src/lib/comparison.ts";
test("partial completions retain slow readers and fixed order, but never another source", () => {
  const order = ["fast", "zxing", "zbar"];
  const slow = { id: "zbar", contentRevision: 1, viewRevision: 1, value: "old" };
  const fast = { id: "fast", contentRevision: 1, viewRevision: 2, value: "new" };
  const stale = { id: "zxing", contentRevision: 0, viewRevision: 1 };
  assert.deepEqual(visibleResults([slow, stale, fast], order, order, 1), [fast, slow]);
  const replacement = { ...slow, viewRevision: 2, value: "replacement" };
  assert.deepEqual(visibleResults([fast, replacement], order, order, 1), [fast, replacement]);
  assert.deepEqual(visibleResults([fast, slow], order, ["fast"], 1), [fast]);
  assert.deepEqual(visibleResults([fast, slow], order, order, 2), []);
});

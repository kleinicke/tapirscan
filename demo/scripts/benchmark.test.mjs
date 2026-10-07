import assert from "node:assert/strict";
import { test } from "node:test";
import { repeats, statistics } from "../src/lib/benchmark.ts";
const region = (text) => ({ text, polygon: [] });
test("statistics distinguish errors, no reads, repeated values", () => {
  const scenes = [
    {
      observations: {
        medium: {
          regions: [region("123"), region("123"), region("456")],
          scanMs: 10,
        },
      },
    },
    { observations: { medium: { regions: [], scanMs: 20 } } },
    { observations: { medium: { regions: [], error: "unavailable" } } },
    { observations: {} },
  ];
  assert.equal(repeats(scenes[0].observations.medium.regions), 1);
  assert.deepEqual(statistics(scenes, "medium"), {
    attempted: 3,
    completed: 2,
    errors: 1,
    found: 1,
    zeroUnique: 1,
    oneUnique: 0,
    multipleUnique: 1,
    reads: 3,
    repeated: 1,
    extras: 1,
    mean: 15,
    median: 15,
    p95: 20,
    total: 30,
  });
  assert.equal(statistics(scenes, "native").median, null);
});

test("repeated copies of one value remain exactly one unique code", () => {
  const stats = statistics(
    [{ observations: { medium: { regions: [region("123"), region("123")], scanMs: 1 } } }],
    "medium",
  );
  assert.equal(stats.oneUnique, 1);
  assert.equal(stats.multipleUnique, 0);
  assert.equal(stats.repeated, 1);
});

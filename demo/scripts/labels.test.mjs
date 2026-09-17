import assert from "node:assert/strict";
import { test } from "node:test";
import { LabelLayout, overlaps } from "../src/lib/labels.ts";
const scanners = [
  { id: "medium", label: "Medium", color: "red", pending: true },
  { id: "zxing", label: "ZXing", color: "blue", pending: true },
  { id: "zbar", label: "ZBar", color: "purple", pending: true },
];
const region = {
  polygon: [
    [230, 240],
    [330, 240],
    [330, 310],
    [230, 310],
  ],
  text: "4104420031326",
  scanner: "medium",
  scanMs: 123.4,
};
const box = (label) => [label.id, label.x, label.y, label.width, label.height];
test("matching values at the same location share one label with fixed scanner rows", () => {
  const layout = new LabelLayout();
  const first = layout.update([region], scanners, 800, 600, 1, "photo");
  const later = layout.update(
    [region, { ...region, scanner: "zxing", scanMs: 15 }],
    scanners,
    800,
    600,
    1,
    "photo",
  );
  assert.equal(later.placed.length, 1);
  assert.deepEqual(box(first.placed[0]), box(later.placed[0]));
  assert.deepEqual(
    later.placed[0].rows.map((r) => r.id),
    ["medium", "zxing", "zbar"],
  );
  assert.deepEqual(
    later.placed[0].rows.map((r) => r.found),
    [true, true, false],
  );
  const rerun = layout.update([{ ...region, scanMs: 9876.5 }], scanners, 800, 600, 1, "photo");
  assert.deepEqual(box(later.placed[0]), box(rerun.placed[0]));
});
test("identical payloads at separate locations remain separate instances", () => {
  const layout = new LabelLayout();
  const second = { ...region, polygon: region.polygon.map(([x, y]) => [x + 330, y]) };
  const { placed, hidden } = layout.update([region, second], scanners, 900, 650, 1, "photo");
  assert.equal(placed.length, 2);
  assert.equal(hidden, 0);
  assert.ok(!overlaps(placed[0], placed[1]));
});
test("label positions tolerate small detection jitter and reset for a new source", () => {
  const layout = new LabelLayout();
  const first = layout.update([region], scanners, 800, 600, 1, "first").placed[0];
  const moved = { ...region, polygon: region.polygon.map(([x, y]) => [x + 1, y + 1]) };
  assert.deepEqual(
    box(first),
    box(layout.update([moved], scanners, 800, 600, 1, "first").placed[0]),
  );
  assert.notEqual(first.id, layout.update([moved], scanners, 800, 600, 1, "second").placed[0].id);
});
test("crowded views omit groups rather than overlapping other labels or detections", () => {
  const { placed, hidden } = new LabelLayout().update(
    Array.from({ length: 30 }, () => region),
    scanners,
    320,
    360,
    1,
    "photo",
  );
  assert.ok(hidden > 0);
  assert.equal(placed.length + hidden, 30);
  for (const [i, label] of placed.entries()) {
    assert.ok(label.x >= 0 && label.x + label.width <= 320);
    assert.ok(!overlaps(label, { x: 230, y: 240, width: 100, height: 70 }));
    for (const other of placed.slice(i + 1)) assert.ok(!overlaps(label, other));
  }
});

test("one-run obstruction does not relocate a label; sustained obstruction eventually does", () => {
  const layout = new LabelLayout();
  const original = layout.update([region], scanners, 800, 600, 1, "photo", 0).placed[0];
  const obstacle = {
    ...region,
    text: "other",
    polygon: [
      [original.x - 10, original.y - 10],
      [original.x + original.width + 10, original.y - 10],
      [original.x + original.width + 10, original.y + original.height + 10],
      [original.x - 10, original.y + original.height + 10],
    ],
  };
  const blocked = layout.update([region, obstacle], scanners, 800, 600, 1, "photo", 100);
  assert.ok(!blocked.placed.some((label) => label.id === original.id));
  const recovered = layout
    .update([region], scanners, 800, 600, 1, "photo", 200)
    .placed.find((label) => label.id === original.id);
  assert.deepEqual(box(recovered), box(original));
  layout.update([region, obstacle], scanners, 800, 600, 1, "photo", 300);
  const persistent = layout
    .update([region, obstacle], scanners, 800, 600, 1, "photo", 1000)
    .placed.find((label) => label.id === original.id);
  assert.ok(persistent);
  assert.notDeepEqual(box(persistent), box(original));
});

test("brief missing detection retains placement memory without displaying stale results", () => {
  const layout = new LabelLayout();
  const original = layout.update([region], scanners, 800, 600, 1, "photo", 0).placed[0];
  assert.equal(layout.update([], scanners, 800, 600, 1, "photo", 100).placed.length, 0);
  assert.deepEqual(
    box(layout.update([region], scanners, 800, 600, 1, "photo", 200).placed[0]),
    box(original),
  );
});

test("rotation keeps distant relocations on hold even past the normal delay", () => {
  const layout = new LabelLayout();
  const original = layout.update([region], scanners, 800, 600, 1, "photo", 0).placed[0];
  const obstacle = {
    ...region,
    text: "other",
    polygon: [
      [original.x - 10, original.y - 10],
      [original.x + original.width + 10, original.y - 10],
      [original.x + original.width + 10, original.y + original.height + 10],
      [original.x - 10, original.y + original.height + 10],
    ],
  };
  for (const now of [100, 500, 900, 1400]) {
    const result = layout.update([region, obstacle], scanners, 800, 600, 1, "photo", now, true);
    assert.ok(!result.placed.some((label) => label.id === original.id));
  }
  const recovered = layout.update([region], scanners, 800, 600, 1, "photo", 1500).placed;
  assert.deepEqual(box(recovered.find((label) => label.id === original.id)), box(original));
});

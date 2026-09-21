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
test("labels follow their barcode while retaining the same relative offset", () => {
  const layout = new LabelLayout();
  const first = layout.update([region], scanners, 800, 600, 1, "first").placed[0];
  const moved = { ...region, polygon: region.polygon.map(([x, y]) => [x + 1, y + 1]) };
  assert.deepEqual(
    box({ ...first, x: first.x + 1, y: first.y + 1 }),
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

test("brief missing detection retains placement memory without displaying stale results", () => {
  const layout = new LabelLayout();
  const original = layout.update([region], scanners, 800, 600, 1, "photo", 0).placed[0];
  assert.equal(layout.update([], scanners, 800, 600, 1, "photo", 100).placed.length, 0);
  assert.deepEqual(
    box(layout.update([region], scanners, 800, 600, 1, "photo", 200).placed[0]),
    box(original),
  );
});

test("labels travel with a barcode through repeated motion without switching sides", () => {
  const layout = new LabelLayout();
  const original = layout.update([region], scanners, 900, 700, 1, "photo", 0).placed[0];
  for (let step = 1; step <= 12; step++) {
    const dx = step * 8,
      dy = step * 4;
    const moved = { ...region, polygon: region.polygon.map(([x, y]) => [x + dx, y + dy]) };
    const label = layout.update([moved], scanners, 900, 700, 1, "photo", step * 40).placed[0];
    assert.equal(label.id, original.id);
    assert.equal(label.x - original.x, dx);
    assert.equal(label.y - original.y, dy);
  }
});

test("resizing a barcode recomputes a nearby label instead of carrying its old offset", () => {
  const layout = new LabelLayout();
  layout.update([region], scanners, 900, 700, 1, "photo", 0);
  for (const height of [140, 30, 190, 70]) {
    const changed = {
      ...region,
      polygon: [
        [230, 275 - height / 2],
        [330, 275 - height / 2],
        [330, 275 + height / 2],
        [230, 275 + height / 2],
      ],
    };
    const current = layout.update([changed], scanners, 900, 700, 1, "photo", 100).placed[0];
    const fresh = new LabelLayout().update([changed], scanners, 900, 700, 1, "photo", 100)
      .placed[0];
    assert.deepEqual([current.x, current.y], [fresh.x, fresh.y]);
    assert.equal(current.anchor.y - current.y - current.height, 6);
  }
});

test("a nearby correction beats flipping sides, without accumulating drift", () => {
  const layout = new LabelLayout();
  const original = layout.update([region], scanners, 800, 600, 1, "photo", 0).placed[0];
  const obstacle = {
    ...region,
    text: "obstacle",
    polygon: [
      [original.x + original.width - 2, original.y - 2],
      [original.x + original.width + 5, original.y - 2],
      [original.x + original.width + 5, original.y + original.height + 2],
      [original.x + original.width - 2, original.y + original.height + 2],
    ],
  };
  const shifted = layout
    .update([region, obstacle], scanners, 800, 600, 1, "photo", 100)
    .placed.find((l) => l.text === region.text);
  assert.ok(shifted);
  assert.equal(shifted.y, original.y);
  assert.ok(Math.abs(shifted.x - original.x) <= 24);
  for (let i = 1; i <= 20; i++) {
    const moved = { ...region, polygon: region.polygon.map(([x, y]) => [x + i * 2, y + i]) };
    const label = layout.update([moved], scanners, 800, 600, 1, "photo", 100 + i * 30).placed[0];
    assert.equal(label.anchor.y - label.y - label.height, 6);
    assert.ok(Math.abs(label.x - (original.x + i * 2)) <= 24);
  }
});
test("a label below a barcode does not flip back for a small top-slot advantage", () => {
  const layout = new LabelLayout();
  const obstacle = {
    ...region,
    text: "obstacle",
    polygon: [
      [150, 170],
      [410, 170],
      [410, 235],
      [150, 235],
    ],
  };
  const below = layout
    .update([region, obstacle], scanners, 800, 600, 1, "photo", 0)
    .placed.find((l) => l.text === region.text);
  assert.ok(below.y > region.polygon[2][1]);
  const next = layout.update([region], scanners, 800, 600, 1, "photo", 100).placed[0];
  assert.equal(next.y, below.y);
});

test("a later larger scanner outline moves the label outward on the same side", () => {
  const layout = new LabelLayout();
  const first = layout.update([region], scanners, 800, 600, 1, "photo", 0).placed[0];
  const larger = {
    ...region,
    scanner: "zxing",
    polygon: [
      [220, 220],
      [340, 220],
      [340, 330],
      [220, 330],
    ],
  };
  const later = layout.update([region, larger], scanners, 800, 600, 1, "photo", 100).placed;
  assert.equal(later.length, 1);
  assert.equal(later[0].id, first.id);
  assert.equal(later[0].y, first.y - 20);
  assert.equal(later[0].x, first.x);
});
test("linear result points share a physical barcode label with rectangular detections", () => {
  const line = {
    ...region,
    scanner: "zxing",
    polygon: [
      [240, 275],
      [320, 275],
    ],
  };
  const separate = { ...region, polygon: region.polygon.map(([x, y]) => [x + 350, y]) };
  const layout = new LabelLayout();
  const { placed } = layout.update(
    [
      region,
      line,
      {
        ...line,
        polygon: [
          [240, 280],
          [320, 280],
        ],
      },
      separate,
    ],
    scanners,
    1000,
    600,
    1,
    "photo",
    0,
  );
  assert.equal(placed.length, 2);
  assert.deepEqual(
    placed[0].rows.map((row) => row.found),
    [true, true, false],
  );
});

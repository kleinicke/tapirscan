import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeLinearDuplicates } from "../dist/multiformat/linear-duplicates.js";

function fixture(gap = false, rotated = false, channels = 1) {
  const width = 460,
    height = 460,
    stride = width * channels + 7;
  const data = new Uint8Array(stride * height).fill(255);
  for (let y = 20; y < 420; y++)
    for (let x = 60; x < 252; x++) {
      if (gap && y >= 200 && y < 202) continue;
      const value = Math.floor((x - 60) / 3) % 3 === 0 ? 20 : 220;
      const px = rotated ? y : x,
        py = rotated ? width - 1 - x : y;
      const offset = py * stride + px * channels;
      data[offset] = value;
      if (channels > 1) {
        data[offset + 1] = value;
        data[offset + 2] = value;
      }
    }
  const quad = (lo, hi) => {
    const q = [
      [60, lo],
      [252, lo],
      [252, hi],
      [60, hi],
    ];
    return rotated ? q.map(([x, y]) => [y, width - 1 - x]) : q;
  };
  const reads = [
    { text: "4006381333931", format: "EAN13", polygon: quad(30, 120), support: 7 },
    { text: "4006381333931", format: "EAN13", polygon: quad(280, 400), support: 4 },
  ];
  const image = { data, width, height, stride, channels };
  return { reads, image };
}
void test("continuous aligned bars merge across height, rotation and padded RGB(A)", () => {
  for (const channels of [1, 3, 4])
    for (const rotated of [false, true]) {
      const { reads, image } = fixture(false, rotated, channels);
      const before = image.data.slice();
      const result = mergeLinearDuplicates(reads, image);
      assert.equal(result.length, 1);
      assert.equal(result[0].support, 7);
      assert.notDeepEqual(result[0].polygon, reads[0].polygon);
      assert.deepEqual(image.data, before);
      assert.equal(reads.length, 2);
    }
});
void test("two-pixel separators preserve separate identical products", () => {
  for (const rotated of [false, true]) {
    const { reads, image } = fixture(true, rotated);
    assert.equal(mergeLinearDuplicates(reads, image).length, 2);
  }
});
void test("text, supplements, lateral displacement and matrix symbols do not establish identity", () => {
  const { reads, image } = fixture();
  assert.equal(
    mergeLinearDuplicates([reads[0], { ...reads[1], text: "5901234123457" }], image).length,
    2,
  );
  assert.equal(
    mergeLinearDuplicates(
      [
        { ...reads[0], eanAddOn: "12" },
        { ...reads[1], eanAddOn: "34" },
      ],
      image,
    ).length,
    2,
  );
  assert.equal(
    mergeLinearDuplicates(
      reads.map((r) => ({ ...r, format: "QRCode" })),
      image,
    ).length,
    2,
  );
  const shifted = reads[1].polygon.map(([x, y]) => [x + 30, y]);
  assert.equal(
    mergeLinearDuplicates([reads[0], { ...reads[1], polygon: shifted }], image).length,
    2,
  );
});
void test("an oblique separator crossing different columns at different heights prevents merging", () => {
  const { reads, image } = fixture();
  for (let x = 60; x < 252; x++) {
    const y = 200 + Math.floor((x - 60) * 0.12);
    image.data[y * image.stride + x] = 255;
    image.data[(y + 1) * image.stride + x] = 255;
  }
  assert.equal(mergeLinearDuplicates(reads, image).length, 2);
});
void test("the same evidence rule covers Common1D formats and retains decoding metadata", () => {
  const { reads, image } = fixture();
  for (const format of ["EAN13", "UPCA", "EAN8", "UPCE", "Code128", "Code39", "ITF"]) {
    assert.equal(
      mergeLinearDuplicates(
        reads.map((r) => ({ ...r, format })),
        image,
      ).length,
      1,
    );
  }
  assert.equal(
    mergeLinearDuplicates(
      [
        { ...reads[0], gs1: true },
        { ...reads[1], gs1: false },
      ],
      image,
    ).length,
    2,
  );
  assert.equal(
    mergeLinearDuplicates([{ ...reads[0], readerInitialization: true }, reads[1]], image).length,
    2,
  );
});

void test("overlapping observations use the existing spatial duplicate rule", () => {
  const { reads, image } = fixture();
  const first = reads[0];
  const overlapping = {
    ...first,
    support: 2,
    polygon: [
      [65, 35],
      [220, 35],
      [220, 110],
      [65, 110],
    ],
  };
  // Widths differ too much for continuous-band joining, but this is a contained
  // observation of the same symbol. Retain the stronger geometry and support.
  const result = mergeLinearDuplicates([first, overlapping], image);
  assert.deepEqual(result, [first]);
  assert.equal(mergeLinearDuplicates([first, { ...overlapping, gs1: true }], image).length, 2);
});

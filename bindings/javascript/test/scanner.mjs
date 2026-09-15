import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile, mkdtemp, writeFile, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { Scanner, scan } from "../dist/index.js";
const loadWasm = async (url) => {
  const b = await readFile(url);
  return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
};
// Independent EAN-13 writer: 4006381333931, first-digit parity LGLLGG.
function fixture() {
  const L = [
    "0001101",
    "0011001",
    "0010011",
    "0111101",
    "0100011",
    "0110001",
    "0101111",
    "0111011",
    "0110111",
    "0001011",
  ];
  const G = [
    "0100111",
    "0110011",
    "0011011",
    "0100001",
    "0011101",
    "0111001",
    "0000101",
    "0010001",
    "0001001",
    "0010111",
  ];
  const text = "4006381333931";
  let bits = "101";
  for (let i = 0; i < 6; i++) bits += ("LGLLGG"[i] === "L" ? L : G)[Number(text[i + 1])];
  bits += "01010";
  for (let i = 7; i < 13; i++)
    bits += L[Number(text[i])].replace(/[01]/g, (x) => (x === "0" ? "1" : "0"));
  bits += "101";
  const width = 480,
    height = 180,
    data = new Uint8Array(width * height).fill(255);
  for (let y = 30; y < 150; y++)
    for (let i = 0; i < bits.length; i++)
      if (bits[i] === "1") data.fill(0, y * width + 50 + i * 4, y * width + 54 + i * 4);
  return { text, image: { data, width, height, channels: 1, stride: width } };
}
for (const mode of ["low", "medium", "high", "very-high"]) {
  test(`${mode}: known EAN, native/WASM parity and lifetime`, async () => {
    const scanner = await Scanner.create({ mode, loadWasm });
    const temp = await mkdtemp(join(tmpdir(), "barcode-parity-"));
    try {
      const { text, image } = fixture();
      const compact = scanner.scan(image);
      assert.deepEqual(compact.image, { width: image.width, height: image.height });
      assert.deepEqual(compact.values, [text]);
      assert.equal(compact.debug, undefined);
      assert.equal("support" in compact.best, false);
      assert.equal("localization" in compact, false);
      assert.equal("searchWindows" in compact, false);
      assert.equal("scan" in compact, false);
      const single = scanner.scan(image, { multiple: false });
      assert.deepEqual(single.barcodes, [scanner.best(compact)]);
      assert.throws(() => scanner.scan(image, null), TypeError);
      assert.throws(() => scanner.scan(image, []), TypeError);
      assert.throws(() => scanner.scan(image, { multiple: 1 }), TypeError);
      assert.throws(() => scanner.scan(image, { includeRegions: "yes" }), TypeError);
      assert.throws(() => scanner.scan(image, { unknown: true }), TypeError);

      const result = scanner.scan(image, { debug: true });
      assert.ok(result.debug.scan.barcodes.some((b) => b.text === text));
      assert.ok(result.debug.scan.barcodes.every((b) => b.text === text));
      assert.equal(result.debug.searchWindows.length, 1);
      const raw = join(temp, "image.gray");
      await writeFile(raw, image.data);
      const binary = fileURLToPath(
        new URL(`../../../build/${mode}/cargo-target/release/examples/scan_raw`, import.meta.url),
      );
      const native = JSON.parse(
        execFileSync(binary, [String(image.width), String(image.height), raw], {
          encoding: "utf8",
        }),
      );
      assert.deepEqual(native.barcodes, result.debug.scan.barcodes);
      assert.equal(native.unfinished, result.debug.scan.unfinished);
      assert.equal(native.candidates.length, result.debug.scan.candidates.length);
      assert.throws(() => scanner.scan({ ...image, data: new Uint8Array(1) }));
      const blank = scanner.scan({ ...image, data: new Uint8Array(image.data.length).fill(255) });
      assert.deepEqual(blank.barcodes, []);
      scanner.dispose();
      scanner.dispose();
      assert.throws(() => scanner.scan(image));
    } finally {
      scanner.dispose();
      await rm(temp, { recursive: true, force: true });
    }
  });
}
test("invalid mode fails before loading", async () => {
  await assert.rejects(
    Scanner.create({
      mode: "invalid",
      loadWasm: () => {
        throw Error("must not load");
      },
    }),
    /Unknown scanner mode/,
  );
});

test("default Node loader, ImageData input and one-shot scan", async () => {
  const { image, text } = fixture();
  const storage = new Uint8ClampedArray(8 + image.width * image.height * 4);
  const data = storage.subarray(8);
  for (let i = 0; i < image.data.length; i++) {
    data.fill(image.data[i], i * 4, i * 4 + 3);
    data[i * 4 + 3] = 255;
  }
  const result = await scan({ data, width: image.width, height: image.height });
  assert.deepEqual(result.values, [text]);
  assert.ok(result.best.rect.width > 0);
  assert.equal(result.best.polygon.length, 4);
});
test("format presets and invalid format validation", async () => {
  for (const formats of ["1D", "2D", "all"]) {
    const scanner = await Scanner.create({ formats });
    try {
      const result = scanner.scan(fixture().image);
      assert.deepEqual(result.values, formats === "2D" ? [] : [fixture().text]);
    } finally {
      scanner.dispose();
    }
  }
  await assert.rejects(
    Scanner.create({
      formats: "typo",
      loadWasm: () => {
        throw Error("must not load");
      },
    }),
    /format/i,
  );
});

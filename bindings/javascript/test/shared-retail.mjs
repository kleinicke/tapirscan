import { test } from "node:test";
import assert from "node:assert/strict";
import { Scanner } from "../dist/index.js";

function ean8(copies = 1, gap = 1, text = "96385074") {
  const digits = [
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
  const bits =
    "101" +
    [...text.slice(0, 4)].map((d) => digits[Number(d)]).join("") +
    "01010" +
    [...text.slice(4)]
      .map((d) => digits[Number(d)].replace(/[01]/g, (bit) => (bit === "0" ? "1" : "0")))
      .join("") +
    "101";
  const width = 340,
    height = 40 + copies * 80 + (copies - 1) * gap;
  const data = new Uint8Array(width * height).fill(255);
  for (let copy = 0; copy < copies; copy++)
    for (let y = 20 + copy * (80 + gap); y < 100 + copy * (80 + gap); y++)
      for (let x = 0; x < bits.length; x++)
        if (bits[x] === "1") data.fill(0, y * width + 36 + x * 4, y * width + 40 + x * 4);
  return { data, width, height, channels: 1 };
}
const reads = (result) =>
  result.barcodes
    .filter((b) => b.format === "EAN8")
    .map((b) => ({
      text: b.text,
      polygon: b.polygon,
      support: b.support,
      bytes: b.bytes,
    }));

test("Medium EAN8-only shares retail recovery and resets selection between scans", async () => {
  const retail = await Scanner.create({ formats: "retail" });
  const only = await Scanner.create({ formats: "EAN8" });
  try {
    assert.deepEqual(retail.scan(ean8(1, 1, "96385075")).values, []);
    for (const image of [ean8(), ean8(2, 1), ean8(2, 20)]) {
      const all = retail.scan(image);
      assert.ok(all.values.includes("96385074"));
      assert.equal(all.values.length, image.height > 120 ? 2 : 1);
      assert.deepEqual(reads(only.scan(image)), reads(all));
      assert.deepEqual(reads(retail.scan(image, { formats: "EAN8" })), reads(all));
      assert.deepEqual(retail.scan(image, { formats: "EAN13" }).values, []);
      assert.deepEqual(reads(retail.scan(image)), reads(all));
      assert.deepEqual(
        reads(only.scan(image, { extendedBudget: true })),
        reads(retail.scan(image, { formats: "EAN8", extendedBudget: true })),
      );
    }
  } finally {
    only.dispose();
    retail.dispose();
  }
});

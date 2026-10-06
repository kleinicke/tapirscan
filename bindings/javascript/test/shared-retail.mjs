import { test } from "node:test";
import assert from "node:assert/strict";
import { Scanner } from "../dist/index.js";

import { ean8 } from "./fixtures.mjs";
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
    assert.deepEqual(retail.inspect(ean8(1, 1, "96385075")).values, []);
    // Copies one pixel apart have continuous bars and read as one symbol.
    for (const [image, count] of [
      [ean8(), 1],
      [ean8(2, 1), 1],
      [ean8(2, 20), 2],
    ]) {
      const all = retail.inspect(image);
      assert.ok(all.values.includes("96385074"));
      assert.equal(all.values.length, count);
      assert.deepEqual(reads(only.inspect(image)), reads(all));
      assert.deepEqual(reads(retail.inspect(image, { formats: "EAN8" })), reads(all));
      assert.deepEqual(retail.inspect(image, { formats: "EAN13" }).values, []);
      assert.deepEqual(reads(retail.inspect(image)), reads(all));
      assert.deepEqual(
        reads(only.inspect(image, { extendedBudget: true })),
        reads(retail.inspect(image, { formats: "EAN8", extendedBudget: true })),
      );
    }
  } finally {
    only.dispose();
    retail.dispose();
  }
});

test("default selection is retail and explicit EAN13 remains available", async () => {
  const scanner = await Scanner.create();
  const ean13 = await Scanner.create({ formats: "EAN13" });
  try {
    assert.deepEqual(scanner.formats, ["EAN13", "UPCA", "EAN8", "UPCE"]);
    assert.deepEqual(scanner.inspect(ean8()).values, ["96385074"]);
    assert.deepEqual(ean13.inspect(ean8()).values, []);
  } finally {
    scanner.dispose();
    ean13.dispose();
  }
});

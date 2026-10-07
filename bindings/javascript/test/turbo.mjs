import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Scanner, inspect } from "../dist/index.js";
import { fixture, ean8 } from "./fixtures.mjs";

const fromAsset = (name) => async () => {
  const data = await readFile(new URL(import.meta.resolve(`tapirscan/wasm/${name}.wasm`)));
  return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
};

for (const preset of [2, 4, 8, 16]) {
  test(`Turbo${preset}: retail reads, subsets, identity and lifetime`, async () => {
    const scanner = await Scanner.create({ experimentalTurbo: preset, formats: "common" });
    const { image, text } = fixture();
    let result;
    try {
      assert.equal(scanner.mode, "low");
      assert.equal(scanner.experimentalTurbo, preset);
      result = scanner.inspect(image, {});
      assert.deepEqual(result.values, [text]);
      assert.equal(result.mode, "low");
      assert.equal(result.experimentalTurbo, preset);
      assert.ok(result.diagnostics);
      assert.ok(Object.isFrozen(result));
      for (const barcode of result.barcodes) {
        assert.equal(barcode.polygon.length, 4);
        assert.ok(barcode.rect.width > 0 && barcode.rect.height > 0);
        for (const [x, y] of barcode.polygon) {
          assert.ok(x >= 0 && x <= image.width && y >= 0 && y <= image.height);
        }
      }
      assert.deepEqual(scanner.inspect(ean8(), { formats: "EAN8" }).values, ["96385074"]);
      // Equal values on two physical labels must remain two results at every preset.
      assert.deepEqual(scanner.inspect(ean8(2, 20), { formats: "EAN8" }).values, [
        "96385074",
        "96385074",
      ]);
      assert.deepEqual(scanner.inspect(ean8(1, 1, "96385075"), { formats: "EAN8" }).values, []);
      assert.deepEqual(scanner.inspect(image, { formats: "QRCode" }).values, []);
      assert.deepEqual(scanner.inspect(image).values, [text]);
      assert.throws(() => scanner.inspect(image, { extendedBudget: true }), /extendedBudget/);
      assert.throws(() => scanner.inspect(image, { experimentalTurbo: 2 }), /Unknown scan option/);
    } finally {
      scanner.dispose();
    }
    scanner.dispose();
    assert.deepEqual(result.values, [text]);
    assert.throws(() => scanner.inspect(image), /disposed/i);
    const oneShot = await inspect(image, {
      experimentalTurbo: preset,
      loadWasm: fromAsset(`experimental-turbo${preset}`),
    });
    assert.deepEqual(oneShot.values, [text]);
    assert.equal(oneShot.experimentalTurbo, preset);
  });
}

test("invalid Turbo selections fail before loading", async () => {
  const loadWasm = () => {
    throw new Error("must not load");
  };
  for (const experimentalTurbo of [0, 1, 3, 32, "4", 0.25, NaN, null, true]) {
    await assert.rejects(Scanner.create({ experimentalTurbo, loadWasm }), /experimentalTurbo must/);
  }
  for (const mode of ["low", "medium", "high", "very-high"]) {
    await assert.rejects(Scanner.create({ mode, experimentalTurbo: 4, loadWasm }), /not both/);
  }
  for (const eanAddOnPolicy of ["Read", "Require"]) {
    await assert.rejects(
      Scanner.create({ experimentalTurbo: 4, eanAddOnPolicy, loadWasm }),
      /Ignore/,
    );
  }
});

test("custom WASM loading cannot silently select a different preset", async () => {
  for (const options of [
    { experimentalTurbo: 4, loadWasm: fromAsset("low") },
    { experimentalTurbo: 4, loadWasm: fromAsset("experimental-turbo2") },
    { mode: "low", loadWasm: fromAsset("experimental-turbo4") },
  ]) {
    await assert.rejects(Scanner.create(options), (error) => error.code === "abi_turbo");
  }
});

test("stable defaults remain Medium without an experimental result field", async () => {
  const result = await inspect(fixture().image);
  assert.equal(result.mode, "medium");
  assert.equal("experimentalTurbo" in result, false);
});

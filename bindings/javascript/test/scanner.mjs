import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile, mkdtemp, writeFile, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Scanner, scan, retailFormats, commonFormats, commonLinearFormats } from "../dist/index.js";
const wasmFile = (mode) =>
  basename(fileURLToPath(import.meta.resolve(`tapirscan/wasm/${mode}.wasm`)));
const loadWasm = async (url) => {
  const b = await readFile(url);
  return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
};
import { fixture } from "./fixtures.mjs";
for (const mode of ["low", "medium", "high", "very-high"]) {
  test(`${mode}: known EAN, native/WASM parity and lifetime`, async () => {
    const scanner = await Scanner.create({ mode, loadWasm, formats: "EAN13" });
    const temp = await mkdtemp(join(tmpdir(), "barcode-parity-"));
    try {
      const { text, image } = fixture();
      const compact = scanner.scan(image);
      assert.ok(Array.isArray(compact.barcodes));
      assert.deepEqual(compact.values, [text]);
      assert.equal(compact.diagnostics, undefined);
      assert.ok(compact.barcodes[0].support > 0);
      assert.ok(Object.isFrozen(compact));
      assert.ok(Object.isFrozen(compact.barcodes[0].polygon));
      assert.throws(() => scanner.inspect(image, null), TypeError);
      assert.throws(() => scanner.inspect(image, []), TypeError);
      assert.throws(() => scanner.inspect(image, { multiple: 1 }), TypeError);
      assert.throws(() => scanner.inspect(image, { includeRegions: "yes" }), TypeError);
      assert.throws(() => scanner.inspect(image, { unknown: true }), TypeError);

      assert.throws(() => scanner.inspect(image, { extendedBudget: true }), /Unknown scan option/);
      const result = scanner.inspect(image, {});
      assert.deepEqual(compact.barcodes, result.barcodes);
      assert.equal(compact.best, compact.barcodes[0]);
      assert.ok(Object.isFrozen(compact.values));
      assert.deepEqual(result.undecoded, result.diagnostics.regions.undecoded);
      assert.ok(result.diagnostics.scan.barcodes.some((b) => b.text === text));
      assert.ok(result.diagnostics.scan.barcodes.every((b) => b.text === text));
      // Medium searches the full frame only on unread barcode evidence.
      assert.ok(Array.isArray(result.diagnostics.searchWindows));
      assert.ok(result.diagnostics.searchWindows.length <= 1);
      const raw = join(temp, "image.gray");
      await writeFile(raw, image.data);
      const binary = fileURLToPath(
        new URL("../../../build/native-target/release/examples/scan_raw", import.meta.url),
      );
      const native = JSON.parse(
        execFileSync(
          binary,
          [
            mode,
            String(image.width),
            String(image.height),
            "1",
            String(image.width),
            raw,
            "1",
            "1",
          ],
          {
            encoding: "utf8",
          },
        ),
      ).debug.scan;
      assert.deepEqual(native.barcodes, result.diagnostics.scan.barcodes);
      assert.equal(native.unfinished, result.diagnostics.scan.unfinished);
      assert.equal(native.candidates.length, result.diagnostics.scan.candidates.length);
      assert.throws(() => scanner.inspect({ ...image, data: new Uint8Array(1) }));
      const blank = scanner.inspect({
        ...image,
        data: new Uint8Array(image.data.length).fill(255),
      });
      assert.deepEqual(blank.barcodes, []);
      scanner.dispose();
      scanner.dispose();
      assert.throws(() => scanner.inspect(image));
    } finally {
      scanner.dispose();
      await rm(temp, { recursive: true, force: true });
    }
  });
}
test("invalid mode fails before loading", async () => {
  // null and objects that stringify to a mode are rejected, not coerced.
  for (const mode of ["invalid", null, { toString: () => "medium" }])
    await assert.rejects(
      Scanner.create({
        mode,
        loadWasm: () => {
          throw Error("must not load");
        },
      }),
      /Unknown scanner mode/,
    );
  await assert.rejects(
    Scanner.create({ eanAddOnPolicy: { toString: () => "ignore" }, loadWasm: () => {} }),
    /eanAddOnPolicy/,
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
  assert.ok(Array.isArray(result.barcodes));
  assert.deepEqual(result.values, [text]);
  assert.ok(result.barcodes[0].rect.width > 0);
  assert.equal(result.barcodes[0].polygon.length, 4);
});
test("format presets and invalid format validation", async () => {
  assert.deepEqual(retailFormats, ["EAN13", "UPCA", "EAN8", "UPCE"]);
  assert.deepEqual(commonLinearFormats, [...retailFormats, "Code128", "Code39", "ITF"]);
  assert.deepEqual(commonFormats, [...commonLinearFormats, "QRCode", "DataMatrix"]);
  for (const formats of ["retail", "common1D", "common", "1D", "2D", "all"]) {
    const scanner = await Scanner.create({ formats });
    try {
      if (formats === "retail") assert.deepEqual(scanner.formats, retailFormats);
      if (formats === "common1D") assert.deepEqual(scanner.formats, commonLinearFormats);
      if (formats === "common") assert.deepEqual(scanner.formats, commonFormats);
      const result = scanner.inspect(fixture().image);
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

test("results are deeply immutable and survive subsequent scans and disposal", async () => {
  for (const formats of [["EAN13"], "all"]) {
    const scanner = await Scanner.create({ mode: "low", formats });
    try {
      const { image, text } = fixture();
      const result = scanner.inspect(image, {});
      for (const mutate of [
        () => {
          result.barcodes.pop();
        },
        () => {
          result.barcodes[0].text = "changed";
        },
        () => {
          result.values[0] = "changed";
        },
        () => {
          result.best.polygon[0][0] = -100;
        },
        () => {
          result.best.rect.left = -100;
        },
        () => {
          result.diagnostics.scan.barcodes[0].polygon[0][0] = -100;
        },
      ])
        assert.throws(mutate, TypeError);
      const snapshot = JSON.stringify(result);
      scanner.inspect({ ...image, data: new Uint8Array(image.data.length).fill(255) });
      scanner.dispose();
      scanner.dispose();
      assert.throws(() => scanner.inspect(image));
      assert.equal(JSON.stringify(result), snapshot);
      assert.deepEqual(result.values, [text]);
      assert.equal(Object.isFrozen(image.data), false);
    } finally {
      scanner.dispose();
    }
  }
});

test("explicit buffers default stride and validate storage across engine paths", async () => {
  const { image, text } = fixture();
  const { stride: _stride, ...packed } = image;
  for (const formats of [["EAN13"], "all"]) {
    const scanner = await Scanner.create({ mode: "low", formats });
    try {
      assert.deepEqual(scanner.inspect(packed).values, [text]);
      for (const invalid of [
        null,
        {},
        { ...image, channels: 2 },
        { ...image, width: 2 },
        { ...image, stride: image.width - 1 },
        { ...image, stride: 128 * 1024 * 1024 },
        { ...image, data: new Uint8Array(1) },
      ])
        assert.throws(() => scanner.inspect(invalid), TypeError);
      for (const options of [{ multiple: false }, { includeRegions: true }, { debug: 1 }])
        assert.throws(() => scanner.inspect(image, options), TypeError);
    } finally {
      scanner.dispose();
    }
  }
});

test("WASM base directory composes with the advanced loader", async () => {
  const { image, text } = fixture();
  const base = new URL("../wasm/", import.meta.url);
  const result = await scan(image, { wasmBaseUrl: base.href.replace(/\/$/, ""), formats: "all" });
  assert.deepEqual(result.values, [text]);
  for (const options of [
    null,
    [],
    { debug: 1 },
    { loadWasm: 1 },
    { wasmBaseUrl: 42 },
    { multiple: false },
  ])
    await assert.rejects(scan(image, options), TypeError);
  const loaded = [];
  const scanner = await Scanner.create({
    mode: "medium",
    formats: "all",
    wasmBaseUrl: base,
    loadWasm: async (url) => {
      assert.equal(new URL(".", url).href, base.href);
      loaded.push(url.pathname.split("/").pop());
      return loadWasm(url);
    },
  });
  scanner.dispose();
  assert.deepEqual(loaded, [wasmFile("medium")]);
});

test("UPC-A selection owns only one primary engine", async () => {
  const instantiate = WebAssembly.instantiate;
  let instances = 0;
  WebAssembly.instantiate = (...args) => {
    instances++;
    return instantiate(...args);
  };
  try {
    for (const formats of [["EAN13"], ["UPCA"]]) {
      instances = 0;
      const scanner = await Scanner.create({ mode: "low", formats });
      try {
        assert.equal(instances, 1);
      } finally {
        scanner.dispose();
      }
    }
  } finally {
    WebAssembly.instantiate = instantiate;
  }
});

test("per-call formats override the scanner defaults for one call", async () => {
  const scanner = await Scanner.create({ mode: "low", formats: ["EAN13", "QRCode"] });
  try {
    const { image, text } = fixture();
    assert.deepEqual(scanner.inspect(image, { formats: "EAN13" }).values, [text]);
    assert.deepEqual(scanner.inspect(image, { formats: "QRCode" }).values, []);
    assert.deepEqual(scanner.inspect(image).values, [text]);
    assert.deepEqual(scanner.inspect(image, { formats: "Code128" }).values, []);
    assert.deepEqual(scanner.inspect(image, { formats: "retail" }).values, [text]);
    assert.throws(() => scanner.inspect(image, { formats: [] }), /format/i);
  } finally {
    scanner.dispose();
  }
  assert.deepEqual((await scan(fixture().image, { formats: "EAN13" })).values, [fixture().text]);
});

test("QR-only creation loads one complete engine", async () => {
  const loaded = [];
  const scanner = await Scanner.create({
    formats: "QRCode",
    loadWasm: async (url) => {
      loaded.push(url.pathname.split("/").pop());
      return loadWasm(url);
    },
  });
  try {
    assert.deepEqual(loaded, [wasmFile("medium")]);
    assert.deepEqual(scanner.inspect(fixture().image).values, []);
    assert.deepEqual(scanner.inspect(fixture().image, { formats: "EAN13" }).values, [
      fixture().text,
    ]);
  } finally {
    scanner.dispose();
  }
});

test("public results preserve semantic metadata independently of diagnostics", async () => {
  const scanner = await Scanner.create({ formats: "QRCode" });
  try {
    // Exercise the public adapter with metadata combinations the engine may return.
    scanner.host.scan = (_image, flags) => ({
      barcodes: [
        {
          text: "part",
          format: "QRCode",
          support: 3,
          polygon: [
            [0, 0],
            [10, 0],
            [10, 10],
            [0, 10],
          ],
          gs1: true,
          readerInitialization: false,
          structuredAppend: { index: 1, count: 2, id: "group", parity: 7 },
          eanAddOn: "12",
          rect: { left: 0, top: 0, width: 10, height: 10 },
        },
      ],
      undecoded: [],
      image: { width: 480, height: 180 },
      mode: "medium",
      elapsedMs: 0,
      ...(flags & 2 ? { debug: { scan: { barcodes: [], unfinished: true } } } : {}),
    });
    {
      const result = scanner.inspect(fixture().image);
      assert.equal(result.best.gs1, true);
      assert.equal(result.best.readerInitialization, false);
      assert.equal(result.best.support, 3);
      assert.equal(result.best.eanAddOn, "12");
      assert.deepEqual(result.best.structuredAppend, {
        index: 1,
        count: 2,
        id: "group",
        parity: 7,
      });
      assert.throws(() => {
        result.best.structuredAppend.index = 2;
      }, TypeError);
      assert.ok(result.diagnostics);
    }
  } finally {
    scanner.dispose();
  }
});

test("inspection reports localization limits", async () => {
  const scanner = await Scanner.create({ mode: "low", formats: "EAN13" });
  try {
    for (const [workLimited, omitted] of [
      [true, 0],
      [false, 1],
      [false, 0],
    ]) {
      scanner.host.scan = (_image, flags) => ({
        barcodes: [],
        undecoded: [],
        image: { width: 480, height: 180 },
        mode: "low",
        elapsedMs: 0,
        ...(flags & 2
          ? {
              debug: {
                scan: { barcodes: [], unfinished: workLimited || omitted > 0 },
                localization: { proposals: [], omitted, workLimited },
              },
            }
          : {}),
      });
      {
        const result = scanner.inspect(fixture().image);
        assert.ok(result.diagnostics);
      }
    }
  } finally {
    scanner.dispose();
  }
});

test("formats are public and immutable", async () => {
  const scanner = await Scanner.create({ mode: "low", formats: ["EAN13", "QRCode"] });
  try {
    assert.deepEqual(scanner.formats, ["EAN13", "QRCode"]);
    assert.throws(() => scanner.formats.push("Code128"), TypeError);
    assert.throws(() => {
      scanner.formats = ["Code128"];
    }, TypeError);
  } finally {
    scanner.dispose();
  }
});

test("EAN evidence stays available when creation enables additional formats", async () => {
  for (const mode of ["low", "medium", "high", "very-high"]) {
    const results = [];
    for (const formats of ["EAN13", "all"]) {
      const scanner = await Scanner.create({ mode, formats });
      try {
        const result = scanner.inspect(fixture().image, { formats: "EAN13" });
        results.push(result);
        assert.ok(Object.isFrozen(result.diagnostics.regions.undecoded));
      } finally {
        scanner.dispose();
      }
    }
    assert.deepEqual(results[0].diagnostics.regions, results[1].diagnostics.regions);
    assert.deepEqual(results[0].values, results[1].values);
  }
  const scanner = await Scanner.create({ formats: "QRCode" });
  try {
    const result = scanner.inspect(fixture().image, {});
    assert.equal(result.diagnostics.regions.proposals, null);
    assert.equal(result.diagnostics.regions.searchWindows, null);
    assert.ok(Array.isArray(result.diagnostics.regions.undecoded));
  } finally {
    scanner.dispose();
  }
});

test("supplement policy is opt-in, validated at creation and fixed for scans", async () => {
  const { image, text } = fixture();
  for (const policy of ["ignore", "read", "require"]) {
    const loaded = [];
    const scanner = await Scanner.create({
      mode: "low",
      eanAddOnPolicy: policy,
      formats: "EAN13",
      loadWasm: async (url) => {
        loaded.push(url.pathname.split("/").pop());
        return loadWasm(url);
      },
    });
    try {
      assert.equal(scanner.eanAddOnPolicy, policy);
      assert.deepEqual(loaded, [wasmFile("low")]);
      assert.deepEqual(scanner.inspect(image).values, policy === "require" ? [] : [text]);
      assert.throws(() => {
        scanner.eanAddOnPolicy = "read";
      }, TypeError);
      assert.throws(() => scanner.inspect(image, { eanAddOnPolicy: "read" }), TypeError);
    } finally {
      scanner.dispose();
    }
  }
  assert.deepEqual((await scan(image, { eanAddOnPolicy: "require" })).values, []);
  for (const policy of [null, "Read", true, 0, {}])
    await assert.rejects(
      Scanner.create({
        eanAddOnPolicy: policy,
        formats: "EAN13",
        loadWasm: () => {
          throw Error("must validate before loading");
        },
      }),
      /eanAddOnPolicy/,
    );
});

test("one-shot accepts creation options", async () => {
  assert.deepEqual((await scan(fixture().image, { mode: "high", loadWasm })).values, [
    fixture().text,
  ]);
});

test("shared format presets cannot be changed by callers", async () => {
  for (const preset of [retailFormats, commonLinearFormats, commonFormats]) {
    assert.ok(Object.isFrozen(preset));
    assert.throws(() => preset.push("QRCode"), TypeError);
  }
  const scanner = await Scanner.create();
  try {
    assert.deepEqual(scanner.formats, ["EAN13", "UPCA", "EAN8", "UPCE"]);
  } finally {
    scanner.dispose();
  }
});

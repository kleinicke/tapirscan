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
test("images above 32 megapixels scan as a reduced copy in source coordinates", async () => {
  const { text, image } = fixture();
  const factor = 20;
  const width = image.width * factor,
    height = image.height * factor,
    data = new Uint8Array(width * height);
  for (let y = 0; y < height; y++)
    for (let x = 0; x < width; x++)
      data[y * width + x] =
        image.data[Math.floor(y / factor) * image.width + Math.floor(x / factor)];
  const result = await scan({ data, width, height, channels: 1 });
  assert.deepEqual(result.values, [text]);
  // The bars start at x = 50 × 20 and end at (50 + 95 × 4) × 20.
  const { left, width: span } = result.barcodes[0].rect;
  assert.ok(Math.abs(left - 1000) <= 40 && Math.abs(left + span - 8600) <= 40, `${left}+${span}`);
});

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
        { ...image, stride: 1024 * 1024 * 1024 },
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

test("barcodes carry the structured content of their text", async () => {
  // GS1-128: start C, FNC1, (01)09506000134352, check character and stop.
  const widths =
    "211232 411131 222122 221213 231131 314111 212222 122132 112331 213311 131141 2331112"
      .replaceAll(" ", "")
      .split("")
      .map(Number);
  const width = 480,
    height = 120,
    data = new Uint8Array(width * height).fill(255);
  let x = 40;
  widths.forEach((modules, i) => {
    if (i % 2 === 0)
      for (let y = 20; y < 100; y++) data.fill(0, y * width + x, y * width + x + modules * 3);
    x += modules * 3;
  });
  const scanner = await Scanner.create({ formats: "Code128" });
  try {
    const [barcode] = scanner.scan({ data, width, height, channels: 1, stride: width }).barcodes;
    assert.equal(barcode.gs1, true);
    assert.deepEqual(barcode.content, {
      kind: "GS1",
      fields: [{ name: "01", value: "09506000134352" }],
    });
    assert.ok(Object.isFrozen(barcode.content.fields[0]));
    const { text, image } = fixture();
    const [plain] = scanner.scan(image, { formats: "EAN13" }).barcodes;
    assert.equal(plain.text, text);
    assert.equal(plain.content, undefined);
  } finally {
    scanner.dispose();
  }
});

test("Codabar reports its start and stop characters beside the stripped text", async () => {
  // Bar/space/bar/space/bar/space/bar per character, 1 marking a wide element.
  const patterns = {
    0: "0000011",
    1: "0000110",
    2: "0001001",
    3: "1100000",
    4: "0010010",
    5: "1000010",
    6: "0100001",
    7: "0100100",
    8: "0110000",
    9: "1001000",
    "-": "0001100",
    $: "0011000",
    ":": "1000101",
    "/": "1010001",
    ".": "1010100",
    "+": "0010101",
    A: "0011010",
    B: "0101001",
    C: "0001011",
    D: "0001110",
  };
  const symbols = ["A123456B", "C40156D", "D7-$:/.+9A"];
  const width = 480,
    band = 120,
    height = band * symbols.length,
    data = new Uint8Array(width * height).fill(255);
  symbols.forEach((symbol, row) => {
    let x = 40;
    for (const character of symbol) {
      // Narrow elements are 2 px, wide 5 px; a narrow space separates characters.
      [...patterns[character], "0"].forEach((wide, element) => {
        const w = wide === "1" ? 5 : 2;
        if (element % 2 === 0 && element < 7)
          for (let y = row * band + 20; y < row * band + 100; y++)
            data.fill(0, y * width + x, y * width + x + w);
        x += w;
      });
    }
  });
  const image = { data, width, height, channels: 1, stride: width };
  for (const mode of ["low", "medium", "high", "very-high"]) {
    const scanner = await Scanner.create({ mode, loadWasm, formats: "Codabar" });
    try {
      const reads = scanner
        .scan(image)
        .barcodes.map((b) => [b.text, b.codabarStartStop])
        .sort();
      assert.deepEqual(
        reads,
        [
          ["123456", "AB"],
          ["40156", "CD"],
          ["7-$:/.+9", "DA"],
        ],
        mode,
      );
      const { barcodes } = scanner.inspect(fixture().image, { formats: "EAN13" });
      assert.ok(barcodes.length === 1 && !("codabarStartStop" in barcodes[0]), mode);
    } finally {
      scanner.dispose();
    }
  }
});

test("MSI Plessey is opt-in and read when selected", async () => {
  // Start bit, 8052344 and its mod-10 check digit 2, stop bit and closing bar.
  let bits = "110";
  for (const digit of "80523442")
    for (const bit of Number(digit).toString(2).padStart(4, "0"))
      bits += bit === "1" ? "110" : "100";
  bits += "1001";
  const width = bits.length * 3 + 120,
    height = 90,
    data = new Uint8Array(width * height).fill(255);
  for (let y = 15; y < 75; y++)
    for (let i = 0; i < bits.length; i++)
      if (bits[i] === "1") data.fill(0, y * width + 60 + i * 3, y * width + 63 + i * 3);
  const image = { data, width, height, channels: 1, stride: width };
  const scanner = await Scanner.create({ formats: "MSIPlessey" });
  try {
    const [barcode] = scanner.scan(image).barcodes;
    assert.deepEqual([barcode.format, barcode.text], ["MSIPlessey", "80523442"]);
    assert.deepEqual(scanner.scan(image, { formats: "all" }).barcodes, []);
  } finally {
    scanner.dispose();
  }
});

test("Industrial 2 of 5 and Pharmacode are opt-in and read when selected", async () => {
  const draw = (bits) => {
    const width = bits.length * 3 + 120,
      height = 90,
      data = new Uint8Array(width * height).fill(255);
    for (let y = 15; y < 75; y++)
      for (let i = 0; i < bits.length; i++)
        if (bits[i] === "1") data.fill(0, y * width + 60 + i * 3, y * width + 63 + i * 3);
    return { data, width, height, channels: 1, stride: width };
  };
  // Industrial 2 of 5 "4711": bars only (wide 111, narrow 1), narrow spaces.
  const digits = [
    "nnwwn",
    "wnnnw",
    "nwnnw",
    "wwnnn",
    "nnwnw",
    "wnwnn",
    "nwwnn",
    "nnnww",
    "wnnwn",
    "nwnwn",
  ];
  const bar = (wide) => (wide ? "1110" : "10");
  let industrial = bar(true) + bar(true) + bar(false);
  for (const digit of "4711") for (const c of digits[Number(digit)]) industrial += bar(c === "w");
  industrial += "1110101110";
  // Pharmacode 1234: narrow bar 1, wide 111, spaces 00, read left to right.
  const bars = [];
  for (let value = 1234; value > 0;) {
    const wide = value % 2 === 0;
    bars.unshift(wide ? "111" : "1");
    value = (value - (wide ? 2 : 1)) / 2;
  }
  for (const [format, image, text] of [
    ["Industrial25", draw(industrial), "4711"],
    ["Pharmacode", draw(bars.join("00")), "1234"],
  ]) {
    const scanner = await Scanner.create({ formats: format });
    try {
      const [barcode] = scanner.scan(image).barcodes;
      assert.deepEqual([barcode.format, barcode.text], [format, text]);
      assert.deepEqual(scanner.scan(image, { formats: "all" }).barcodes, []);
    } finally {
      scanner.dispose();
    }
  }
});

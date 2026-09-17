import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile, mkdtemp, writeFile, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { Scanner, scan, retailFormats, commonFormats, commonLinearFormats } from "../dist/index.js";
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
      assert.ok(compact.best.support > 0);
      assert.equal("confidence" in compact.best, false);
      assert.equal("localization" in compact, false);
      assert.equal("searchWindows" in compact, false);
      assert.equal("scan" in compact, false);
      assert.equal(compact.best, compact.barcodes[0]);
      assert.throws(() => scanner.scan(image, null), TypeError);
      assert.throws(() => scanner.scan(image, []), TypeError);
      assert.throws(() => scanner.scan(image, { multiple: 1 }), TypeError);
      assert.throws(() => scanner.scan(image, { includeRegions: "yes" }), TypeError);
      assert.throws(() => scanner.scan(image, { unknown: true }), TypeError);

      assert.deepEqual(scanner.scan(image, { extendedBudget: false }).values, compact.values);
      assert.deepEqual(scanner.scan(image, { extendedBudget: true }).values, compact.values);
      assert.throws(() => scanner.scan(image, { extendedBudget: "yes" }), /a boolean/);
      const result = scanner.scan(image, { debug: true });
      assert.deepEqual(compact.undecoded, result.undecoded);
      assert.deepEqual(result.undecoded, result.debug.regions.undecoded);
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
  assert.deepEqual(retailFormats, ["EAN13", "UPCA", "EAN8", "UPCE"]);
  assert.deepEqual(commonLinearFormats, [...retailFormats, "Code128", "Code39", "ITF"]);
  assert.deepEqual(commonFormats, [...commonLinearFormats, "QRCode", "DataMatrix"]);
  for (const formats of ["retail", "common1D", "common", "1D", "2D", "all"]) {
    const scanner = await Scanner.create({ formats });
    try {
      if (formats === "retail") assert.deepEqual(scanner.formats, retailFormats);
      if (formats === "common1D") assert.deepEqual(scanner.formats, commonLinearFormats);
      if (formats === "common") assert.deepEqual(scanner.formats, commonFormats);
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

test("results are deeply immutable and survive subsequent scans and disposal", async () => {
  for (const formats of [["EAN13"], "all"]) {
    const scanner = await Scanner.create({ mode: "low", formats });
    try {
      const { image, text } = fixture();
      const result = scanner.scan(image, { debug: true });
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
          result.debug.scan.barcodes[0].polygon[0][0] = -100;
        },
      ])
        assert.throws(mutate, TypeError);
      const snapshot = JSON.stringify(result);
      scanner.scan({ ...image, data: new Uint8Array(image.data.length).fill(255) });
      scanner.dispose();
      scanner.dispose();
      assert.throws(() => scanner.scan(image));
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
      assert.deepEqual(scanner.scan(packed).values, [text]);
      for (const invalid of [
        null,
        {},
        { ...image, channels: 2 },
        { ...image, width: 2 },
        { ...image, stride: image.width - 1 },
        { ...image, stride: 128 * 1024 * 1024 },
        { ...image, data: new Uint8Array(1) },
      ])
        assert.throws(() => scanner.scan(invalid), TypeError);
      for (const options of [{ multiple: false }, { includeRegions: true }, { debug: 1 }])
        assert.throws(() => scanner.scan(image, options), TypeError);
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
  assert.deepEqual(loaded, [
    "medium-complete-release-20260916.wasm",
    "low-complete-release-20260916.wasm",
    "multiformat.wasm",
  ]);
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

test("per-call format subsets reuse engines and preserve defaults", async () => {
  const scanner = await Scanner.create({ mode: "low", formats: ["EAN13", "QRCode"] });
  try {
    const { image, text } = fixture();
    assert.deepEqual(scanner.scan(image, { formats: "EAN13" }).values, [text]);
    assert.deepEqual(scanner.scan(image, { formats: "QRCode" }).values, []);
    assert.deepEqual(scanner.scan(image).values, [text]);
    assert.throws(() => scanner.scan(image, { formats: "Code128" }), /subset/);
    assert.throws(() => scanner.scan(image, { formats: [] }), /format/i);
  } finally {
    scanner.dispose();
  }
  assert.deepEqual((await scan(fixture().image, { formats: "EAN13" })).values, [fixture().text]);
});

test("QR-only creation loads only the additional engine", async () => {
  const loaded = [];
  const scanner = await Scanner.create({
    formats: "QRCode",
    loadWasm: async (url) => {
      loaded.push(url.pathname.split("/").pop());
      return loadWasm(url);
    },
  });
  try {
    assert.deepEqual(loaded, ["multiformat.wasm"]);
    assert.deepEqual(scanner.scan(fixture().image).values, []);
    assert.throws(() => scanner.scan(fixture().image, { formats: "EAN13" }), /subset/);
  } finally {
    scanner.dispose();
  }
});

test("public results preserve semantic metadata independently of diagnostics", async () => {
  const scanner = await Scanner.create({ formats: "QRCode" });
  try {
    // Exercise the public adapter with metadata combinations the engine may return.
    scanner.host.scan = () => ({
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
        },
      ],
      scanMs: 1,
      unfinished: true,
      regions: [],
    });
    for (const debug of [false, true]) {
      const result = scanner.scan(fixture().image, { debug });
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
      assert.equal(result.unfinished, true);
      assert.equal(Boolean(result.debug), debug);
    }
  } finally {
    scanner.dispose();
  }
});

test("localization limits reach compact results without diagnostics", async () => {
  const scanner = await Scanner.create({ mode: "low" });
  try {
    for (const [workLimited, omitted] of [
      [true, 0],
      [false, 1],
      [false, 0],
    ]) {
      scanner.host.scanLocalized = () => ({
        scan: { barcodes: [], unfinished: false },
        scanMs: 1,
        localization: { proposals: [], omitted, workLimited },
      });
      for (const debug of [false, true]) {
        const result = scanner.scan(fixture().image, { debug });
        assert.equal(result.unfinished, workLimited || omitted > 0);
        assert.equal(Boolean(result.debug), debug);
      }
    }
  } finally {
    scanner.dispose();
  }
});

test("formats are public and immutable, with actionable subset errors", async () => {
  const scanner = await Scanner.create({ mode: "low", formats: ["EAN13", "QRCode"] });
  try {
    assert.deepEqual(scanner.formats, ["EAN13", "QRCode"]);
    assert.throws(() => scanner.formats.push("Code128"), TypeError);
    assert.throws(() => {
      scanner.formats = ["Code128"];
    }, TypeError);
    assert.throws(
      () => scanner.scan(fixture().image, { formats: "Code128" }),
      /Requested: Code128; configured: EAN13, QRCode/,
    );
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
        const result = scanner.scan(fixture().image, { formats: "EAN13", debug: true });
        results.push(result);
        assert.ok(Object.isFrozen(result.debug.regions.undecoded));
      } finally {
        scanner.dispose();
      }
    }
    assert.deepEqual(results[0].debug.regions, results[1].debug.regions);
    assert.deepEqual(results[0].values, results[1].values);
  }
  const scanner = await Scanner.create({ formats: "QRCode" });
  try {
    const result = scanner.scan(fixture().image, { debug: true });
    assert.equal(result.debug.regions.proposals, null);
    assert.equal(result.debug.regions.searchWindows, null);
    assert.ok(Array.isArray(result.debug.regions.undecoded));
  } finally {
    scanner.dispose();
  }
});

test("supplement policy is opt-in, validated at creation and fixed for scans", async () => {
  const { image, text } = fixture();
  for (const policy of ["Ignore", "Read", "Require"]) {
    const loaded = [];
    const scanner = await Scanner.create({
      mode: "low",
      eanAddOnPolicy: policy,
      loadWasm: async (url) => {
        loaded.push(url.pathname.split("/").pop());
        return loadWasm(url);
      },
    });
    try {
      assert.equal(scanner.eanAddOnPolicy, policy);
      assert.equal(loaded.includes("multiformat.wasm"), policy !== "Ignore");
      assert.deepEqual(scanner.scan(image).values, policy === "Require" ? [] : [text]);
      assert.throws(() => {
        scanner.eanAddOnPolicy = "Read";
      }, TypeError);
      assert.throws(() => scanner.scan(image, { eanAddOnPolicy: "Read" }), TypeError);
    } finally {
      scanner.dispose();
    }
  }
  assert.deepEqual((await scan(image, { eanAddOnPolicy: "Require" })).values, []);
  for (const policy of [null, "read", true, 0, {}])
    await assert.rejects(
      Scanner.create({
        eanAddOnPolicy: policy,
        loadWasm: () => {
          throw Error("must validate before loading");
        },
      }),
      /eanAddOnPolicy/,
    );
});

test("coverage defers only contained retries and preserves the full-frame bit", async () => {
  const { containsPoint, uncoveredRetryMask } = await import("../dist/multiformat/coverage.js");
  const quad = [
    [0, 0],
    [10, 0],
    [10, 10],
    [0, 10],
  ];
  const adjacent = [
    [9, 0],
    [19, 0],
    [19, 10],
    [9, 10],
  ];
  const proposals = Array.from({ length: 63 }, () => ({ polygon: adjacent }));
  proposals[0] = proposals[62] = { polygon: quad };
  assert.deepEqual(uncoveredRetryMask(proposals, [quad]), [0xfffffffe, 0xbfffffff]);
  assert.deepEqual(uncoveredRetryMask(proposals, [quad], [0, 0x80000000]), [0, 0x80000000]);
  assert.equal(containsPoint([0, 5], quad), true);
  assert.equal(containsPoint([NaN, 0], quad), false);
  assert.equal(
    containsPoint(
      [0, 0],
      Array.from({ length: 4 }, () => [0, 0]),
    ),
    false,
  );
});

test("extended budget accepts formats without the primary reader", async () => {
  const scanner = await Scanner.create({ formats: "QRCode", loadWasm });
  try {
    assert.deepEqual(scanner.scan(fixture().image, { extendedBudget: true }).values, []);
  } finally {
    scanner.dispose();
  }
});
test("one-shot forwards continuation", async () => {
  assert.deepEqual((await scan(fixture().image, { extendedBudget: true, loadWasm })).values, [
    fixture().text,
  ]);
});
test("continuation services later candidates while preserving effort and unfinished status", async () => {
  const { IndependentScanner } = await import("../dist/completion-host.mjs");
  const image = {
    data: new Uint8Array(600 * 300).fill(255),
    width: 600,
    height: 300,
    stride: 600,
    channels: 1,
  };
  const quads = Array.from({ length: 64 }, () => [
    [0, 0],
    [599, 0],
    [599, 299],
    [0, 299],
  ]);
  for (const mode of ["low", "medium", "high", "very-high"]) {
    const scanner = await IndependentScanner.create(
      await loadWasm(new URL(`../wasm/${mode}-complete-release-20260916.wasm`, import.meta.url)),
    );
    try {
      const bounded = scanner.scan(image, quads, { maxRetryPathsPerFrame: 10 });
      const completed = scanner.scan(image, quads, {
        maxRetryPathsPerFrame: 10,
        finishCandidates: true,
      });
      const reference = scanner.scan(image, quads, { maxRetryPathsPerFrame: 65536 });
      const work = (result) => result.candidates.map((c) => c.work.retry_paths);
      assert.equal(
        work(bounded).reduce((a, b) => a + b, 0),
        10,
      );
      assert.deepEqual(work(completed), work(reference));
      assert.ok(work(completed).every((n) => n > 0 && n <= 512));
      assert.equal(completed.unfinished, reference.unfinished);
    } finally {
      scanner.dispose();
    }
  }
});

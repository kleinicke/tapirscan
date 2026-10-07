import assert from "node:assert/strict";
import { test } from "node:test";
import {
  comparisonOptions,
  nextRelease,
  previousReleaseOptions,
  visibleResults,
} from "../src/lib/comparison.ts";
import { releasesFrom } from "./releases.mjs";
test("partial completions retain slow readers and fixed order, but never another source", () => {
  const order = ["fast", "zxing", "zbar"];
  const slow = { id: "zbar", contentRevision: 1, viewRevision: 1, value: "old" };
  const fast = { id: "fast", contentRevision: 1, viewRevision: 2, value: "new" };
  const stale = { id: "zxing", contentRevision: 0, viewRevision: 1 };
  assert.deepEqual(visibleResults([slow, stale, fast], order, order, 1), [fast, slow]);
  const replacement = { ...slow, viewRevision: 2, value: "replacement" };
  assert.deepEqual(visibleResults([fast, replacement], order, order, 1), [fast, replacement]);
  assert.deepEqual(visibleResults([fast, slow], order, ["fast"], 1), [fast]);
  assert.deepEqual(visibleResults([fast, slow], order, order, 2), []);
});

test("basic ZXing readers disable recovery independently of enhanced readers", async () => {
  const { readFile } = await import("node:fs/promises");
  const vm = await import("node:vm");
  const ts = await import("typescript");
  const source = await readFile(new URL("../src/lib/reference.worker.ts", import.meta.url), "utf8");
  const calls = [];
  const context = {
    exports: {},
    require(name) {
      if (name === "./zxing-js")
        return {
          scanZXingJS: (...args) => {
            calls.push(args.at(-1));
            return {};
          },
        };
      if (name === "zxing-wasm/reader")
        return {
          prepareZXingModule: async () => {},
          readBarcodes: async (_image, options) => {
            calls.push(options);
            return [];
          },
          defaultReaderOptions: { maxNumberOfSymbols: 255 },
        };
      return { ZBarSymbolType: {} };
    },
    self: {
      postMessage(message) {
        assert.equal(message.error, undefined);
      },
    },
    fetch: async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) }),
    URL,
    Uint8Array,
    Uint8ClampedArray,
    ImageData: class {},
    performance,
  };
  vm.runInNewContext(
    ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } }).outputText,
    context,
  );
  for (const engine of ["zxingdefault", "zxing", "zxingjsdefault", "zxingjs"]) {
    await context.self.onmessage({
      data: {
        engine,
        formats: ["EAN13"],
        engineBaseUrl: "https://example.com/",
        width: 1,
        height: 1,
        buffer: new ArrayBuffer(4),
      },
    });
  }
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [
    { formats: ["EAN13"], tryHarder: false, tryRotate: false, tryDownscale: false },
    {
      formats: ["EAN13"],
      tryHarder: true,
      tryRotate: true,
      tryDownscale: true,
      maxNumberOfSymbols: 255,
    },
    { harder: false, rotate: false, downscale: false, invert: false },
    { harder: true, rotate: true, downscale: false, invert: false },
  ]);
});

test("main readers follow the latest release; next readers and Turbo use the current build", () => {
  for (const [id, mode, label] of [
    ["ts-low-next", "low", "TS-Low-next"],
    ["ts-med-next", "medium", "TS-Med-next"],
    ["ts-high-next", "high", "TS-High-next"],
    ["ts-vhigh-next", "very-high", "TS-VHigh-next"],
  ]) {
    const spec = comparisonOptions.find((entry) => entry.id === id);
    assert.equal(spec.label, label);
    assert.equal(spec.version, mode);
    assert.equal(spec.releaseVersion, nextRelease);
  }
  for (const id of ["turbo2", "turbo4", "turbo8", "turbo16"])
    assert.equal(comparisonOptions.find((entry) => entry.id === id).releaseVersion, nextRelease);
  for (const id of ["turbo", "fast", "quality", "veryhigh"])
    assert.equal(comparisonOptions.find((entry) => entry.id === id).releaseVersion, undefined);
  assert.equal(new Set(comparisonOptions.map((entry) => entry.id)).size, comparisonOptions.length);
});

test("earlier published versions get one reader per effort level", () => {
  const previous = previousReleaseOptions(["1.2.1", "1.2.0"]);
  assert.equal(previous.length, 8);
  assert.deepEqual(
    previous.filter((entry) => entry.releaseVersion === "1.2.1").map((entry) => entry.version),
    ["low", "medium", "high", "very-high"],
  );
  const ids = previous.map((entry) => entry.id);
  assert.equal(
    new Set([...ids, ...comparisonOptions.map((entry) => entry.id)]).size,
    8 + comparisonOptions.length,
  );
  assert.equal(previous[1].label, "TS-Med 1.2.1");
});

test("published versions derive from package.json aliases", () => {
  assert.deepEqual(
    releasesFrom({
      "tapirscan-1-10-0": "npm:tapirscan@1.10.0",
      "tapirscan-1-2-2": "npm:tapirscan@1.2.2",
      tapirscan: "link:../bindings/javascript",
      svelte: "^5.0.0",
    }),
    [
      { version: "1.2.2", alias: "tapirscan-1-2-2" },
      { version: "1.10.0", alias: "tapirscan-1-10-0" },
    ],
  );
  assert.throws(() => releasesFrom({ "tapirscan-1-2-2": "npm:tapirscan@1.2.1" }), /must be/);
});

import assert from "node:assert/strict";
import { test } from "node:test";
import { visibleResults } from "../src/lib/comparison.ts";
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

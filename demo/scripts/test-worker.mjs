// Execute the actual production worker with a static-host loader under a subpath.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const root = new URL("../../", import.meta.url);
const dist = new URL("demo/dist/", root);
const worker = (await readdir(new URL("assets/", dist))).find((p) => p.startsWith("scan.worker-"));
assert.ok(worker, "Build the demo before testing its worker");
const source = await readFile(new URL(`assets/${worker}`, dist), "utf8");
const fixture = JSON.parse(
  execFileSync(
    "python3",
    [
      "-c",
      [
        "import sys,json,base64",
        "sys.path.insert(0,'scripts')",
        "from fixture_data import fixtures",
        "_,pixels,w,h,_,_,text=next(fixtures())",
        "print(json.dumps(dict(width=w,height=h,pixels=base64.b64encode(pixels).decode())))",
      ].join("\n"),
    ],
    { cwd: fileURLToPath(root), encoding: "utf8" },
  ),
);
const gray = Buffer.from(fixture.pixels, "base64");
const rgba = new Uint8Array(gray.length * 4);
for (let i = 0; i < gray.length; i++) rgba.set([gray[i], gray[i], gray[i], 255], i * 4);
const base = "https://example.test/tapirscan/";
const messages = [];
const loaded = new Set();
const context = {
  self: {
    location: { href: base + "assets/" + worker },
    postMessage: (message) => messages.push(message),
  },
  URL,
  WebAssembly,
  TextDecoder,
  TextEncoder,
  Uint8Array,
  Uint32Array,
  Int32Array,
  Float64Array,
  ArrayBuffer,
  performance,
  console,
  fetch: async (input) => {
    const url = new URL(input, base + "assets/" + worker);
    assert.ok(url.href.startsWith(base + "engines/"), `Incorrect worker asset URL: ${url}`);
    const relative = url.href.slice(base.length);
    loaded.add(relative);
    return new Response(await readFile(new URL(relative, dist)), {
      headers: { "Content-Type": "application/wasm" },
    });
  },
};
vm.createContext(context);
vm.runInContext(source, context);
const supportedModes = ["low", "medium", "high", "very-high"];
const modes = [...new Set(process.env.TAPIRSCAN_TEST_MODES?.split(",") || supportedModes)];
assert.ok(modes.length && modes.every((mode) => supportedModes.includes(mode)));
for (const mode of modes) {
  messages.length = 0;
  await context.self.onmessage({
    data: {
      width: fixture.width,
      height: fixture.height,
      buffer: rgba.slice().buffer,
      scannerVersion: mode,
      formats: ["EAN13", "QRCode"],
      engineBaseUrl: base + "engines/",
    },
  });
  const message = messages.at(-1);
  assert.ok(message.result, JSON.stringify(message));
  assert.ok(message.result.regions.some((b) => b.text === "4006381333931"));
  console.log(`${mode}: production worker loads and scans under a hosting subpath`);
}
assert.equal(
  loaded.size,
  modes.length + 1,
  "Selected EAN engines and the additional reader must load",
);

const reference = (await readdir(new URL("assets/", dist))).find((p) =>
  p.startsWith("reference.worker-"),
);
assert.ok(reference);
const referenceSource = await readFile(new URL(`assets/${reference}`, dist), "utf8");
for (const engine of ["zxing", "zbar"]) {
  messages.length = 0;
  const referenceContext = {
    ...context,
    self: { ...context.self, location: { href: base + "assets/" + reference } },
    ImageData: class {
      constructor(data, width, height) {
        Object.assign(this, { data, width, height });
      }
    },
    Uint8ClampedArray,
    Int8Array,
    Uint16Array,
    Int16Array,
    Float32Array,
    BigInt64Array,
    BigUint64Array,
    setTimeout,
    clearTimeout,
  };
  vm.createContext(referenceContext);
  vm.runInContext(referenceSource, referenceContext);
  await referenceContext.self.onmessage({
    data: {
      engine,
      formats: ["EAN13"],
      engineBaseUrl: base + "engines/",
      width: fixture.width,
      height: fixture.height,
      buffer: rgba.slice().buffer,
    },
  });
  const message = messages.at(-1);
  assert.ok(message.result, JSON.stringify(message));
  assert.ok(message.result.regions.some((b) => b.text === "4006381333931"));
  console.log(`${engine}: production comparison worker loads and decodes EAN13`);
}

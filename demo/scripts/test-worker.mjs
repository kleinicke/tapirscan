// Execute the actual production worker with a static-host loader under a subpath.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { prepareZXingModule, writeBarcode } from "zxing-wasm/writer";
import {
  retailFormats,
  commonFormats,
  linearFormats,
  matrixFormats,
} from "../../bindings/javascript/dist/index.js";

const selections = [
  ["EAN13"],
  retailFormats,
  commonFormats,
  linearFormats,
  matrixFormats,
  ["QRCode"],
  ["EAN13"],
];

const root = new URL("../../", import.meta.url);
const turboPins = JSON.parse(await readFile(new URL("demo/src/lib/turbo.json", root), "utf8"));
const experimentalEngines = ["turbo", ...turboPins.variants.map((entry) => entry.key)];
const registry = JSON.parse(
  await readFile(new URL("demo/src/lib/scanner-versions.json", root), "utf8"),
);
const releaseVersions = registry.versions.map((entry) => entry.version);
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
  crypto: globalThis.crypto,
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
  for (const formats of selections) {
    messages.length = 0;
    await context.self.onmessage({
      data: {
        width: fixture.width,
        height: fixture.height,
        buffer: rgba.slice().buffer,
        scannerVersion: mode,
        finishCandidates: formats.includes("EAN13"),
        formats,
        engineBaseUrl: base + "engines/",
      },
    });
    const message = messages.at(-1);
    assert.ok(message.result, JSON.stringify(message));
    assert.equal(
      message.result.regions.some((b) => b.text === "4006381333931"),
      formats.includes("EAN13"),
    );
  }
  console.log(
    `${mode}: production worker switches EAN13, retail, common, all, QR-only, and back under a hosting subpath`,
  );
}
assert.equal(
  loaded.size,
  modes.length,
  "Each selected mode must load exactly one complete scanner",
);

// Version changes must replace the worker's cached session, including switching back.
for (const releaseVersion of [...releaseVersions.toReversed(), ...releaseVersions]) {
  for (const mode of modes) {
    messages.length = 0;
    await context.self.onmessage({
      data: {
        width: fixture.width,
        height: fixture.height,
        buffer: rgba.slice().buffer,
        scannerVersion: mode,
        releaseVersion,
        formats: retailFormats,
        engineBaseUrl: base + "engines/",
      },
    });
    const message = messages.at(-1);
    assert.ok(message.result, JSON.stringify(message));
    assert.ok(message.result.regions.some((read) => read.text === "4006381333931"));
  }
}
assert.equal(
  loaded.size,
  modes.length * releaseVersions.length,
  "Every immutable version must be loaded",
);
console.log("Version switching and switching back passed for every effort");

// Independent test-only encoder: ensure non-EAN13 results survive the demo adapter.
await prepareZXingModule({
  overrides: {
    wasmBinary: await readFile(
      new URL("../node_modules/zxing-wasm/dist/writer/zxing_writer.wasm", import.meta.url),
    ),
  },
  fireImmediately: true,
});
let qrFixture;
for (const [format, text, formats] of [
  ["EAN8", "96385074", retailFormats],
  ["QRCode", "Tapirscan QR camera test", commonFormats],
]) {
  const encoded = await writeBarcode(text, { format });
  assert.equal(encoded.error, "");
  const { width, height, data } = encoded.symbol;
  const scale = 6,
    padding = 60,
    w = width * scale + padding * 2,
    h = height * scale + padding * 2;
  const pixels = new Uint8Array(w * h * 4).fill(255);
  for (let y = 0; y < height * scale; y++)
    for (let x = 0; x < width * scale; x++) {
      const value = data[Math.floor(y / scale) * width + Math.floor(x / scale)];
      pixels.set([value, value, value, 255], ((y + padding) * w + x + padding) * 4);
    }
  if (format === "QRCode") qrFixture = { width: w, height: h, pixels, text };
  for (const mode of modes) {
    messages.length = 0;
    await context.self.onmessage({
      data: {
        width: w,
        height: h,
        buffer: pixels.slice().buffer,
        scannerVersion: mode,
        formats,
        engineBaseUrl: base + "engines/",
      },
    });
    const message = messages.at(-1);
    assert.ok(message.result, JSON.stringify(message));
    assert.ok(
      message.result.regions.some((region) => region.text === text),
      `${mode} ${format} must reach the demo: ${JSON.stringify(message)}`,
    );
  }
  for (const engine of experimentalEngines) {
    messages.length = 0;
    await context.self.onmessage({
      data: {
        engine,
        scannerVersion: "medium",
        releaseVersion: "ignored-for-turbo",
        width: w,
        height: h,
        buffer: pixels.slice().buffer,
        formats,
        finishCandidates: true,
        engineBaseUrl: base + "engines/",
      },
    });
    const turboMessage = messages.at(-1);
    assert.ok(turboMessage.result, JSON.stringify(turboMessage));
    assert.equal(turboMessage.result.unfinished, true);
    assert.equal(
      turboMessage.result.regions.some((region) => region.text === text),
      true,
      `${engine} ${format}: ${JSON.stringify(turboMessage)}`,
    );
  }
  console.log(`${format}: decoded value reaches demo in all modes`);
}

for (const engine of experimentalEngines) {
  for (const formats of [["QRCode"], ["EAN13"], ["Code128", "QRCode"]]) {
    messages.length = 0;
    await context.self.onmessage({
      data: {
        engine,
        scannerVersion: "very-high",
        formats,
        width: qrFixture.width,
        height: qrFixture.height,
        buffer: qrFixture.pixels.slice().buffer,
        engineBaseUrl: base + "engines/",
        finishCandidates: true,
      },
    });
    const message = messages.at(-1);
    assert.ok(message.result, JSON.stringify(message));
    assert.equal(
      message.result.regions.some((region) => region.text === qrFixture.text),
      formats.includes("QRCode"),
    );
  }
}
for (const entry of [turboPins, ...turboPins.variants]) {
  assert.ok(loaded.has(`engines/${entry.file}`), `${entry.label} must load its own pinned asset`);
}
messages.length = 0;
await context.self.onmessage({
  data: { engine: "turbo-unknown", scannerVersion: "low", formats: commonFormats },
});
assert.match(messages.at(-1).error, /Unknown Tapirscan/);
console.log(
  "Turbo tiers: independent assets and QR-only, excluded QR, mixed format coverage passed",
);

const reference = (await readdir(new URL("assets/", dist))).find((p) =>
  p.startsWith("reference.worker-"),
);
assert.ok(reference);
const referenceSource = await readFile(new URL(`assets/${reference}`, dist), "utf8");
for (const engine of [
  "zxing",
  "zxingdefault",
  "zbar",
  "jsqr",
  "native",
  "zxingjs",
  "zxingjsdefault",
  "quagga",
]) {
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
  let workerSource = referenceSource;
  if (engine === "quagga") {
    const file = (await readdir(new URL("assets/", dist))).find((p) =>
      p.startsWith("quagga.worker-"),
    );
    assert.ok(file);
    workerSource = await readFile(new URL(`assets/${file}`, dist), "utf8");
  }
  vm.runInContext(workerSource, referenceContext);
  if (engine === "jsqr" || engine === "native") {
    messages.length = 0;
    await referenceContext.self.onmessage({
      data: {
        engine,
        formats: commonFormats,
        width: qrFixture.width,
        height: qrFixture.height,
        buffer: qrFixture.pixels.slice().buffer,
      },
    });
    if (engine === "native") {
      assert.match(messages.at(-1).error, /unavailable/);
    } else {
      assert.equal(messages.at(-1).result.regions[0].text, qrFixture.text);
      assert.equal(messages.at(-1).result.unfinished, true);
      messages.length = 0;
      await referenceContext.self.onmessage({
        data: {
          engine,
          formats: retailFormats,
          width: qrFixture.width,
          height: qrFixture.height,
          buffer: qrFixture.pixels.slice().buffer,
        },
      });
      assert.match(messages.at(-1).error, /QR Code only/);
    }
    console.log(`${engine}: optional reader decoding or explicit unsupported status verified`);
    continue;
  }
  for (const formats of selections) {
    messages.length = 0;
    await referenceContext.self.onmessage({
      data: {
        engine,
        formats,
        engineBaseUrl: base + "engines/",
        width: fixture.width,
        height: fixture.height,
        buffer: rgba.slice().buffer,
      },
    });
    const message = messages.at(-1);
    if (engine === "quagga" && !formats.includes("EAN13")) {
      assert.match(message.error, /supports none/);
      continue;
    }
    assert.ok(message.result, JSON.stringify(message));
    assert.equal(
      message.result.regions.some((b) => b.text === "4006381333931"),
      formats.includes("EAN13"),
    );
  }
  if (engine === "zxingjs") {
    const w = fixture.height,
      h = fixture.width;
    const rotated = new Uint8Array(w * h * 4);
    for (let y = 0; y < fixture.height; y++)
      for (let x = 0; x < fixture.width; x++) {
        const src = (y * fixture.width + x) * 4,
          dest = (x * w + fixture.height - 1 - y) * 4;
        for (let c = 0; c < 3; c++) rotated[dest + c] = 255 - rgba[src + c];
        rotated[dest + 3] = 255;
      }
    messages.length = 0;
    await referenceContext.self.onmessage({
      data: {
        engine,
        formats: retailFormats,
        width: w,
        height: h,
        buffer: rotated.buffer,
        zxingJSSettings: { harder: true, rotate: true, downscale: true, invert: true },
      },
    });
    const result = messages.at(-1).result;
    assert.ok(result, JSON.stringify(messages.at(-1)));
    assert.ok(result.regions.some((region) => region.text === "4006381333931"));
    for (const region of result.regions)
      for (const [x, y] of region.polygon) assert.ok(x >= 0 && x < w && y >= 0 && y < h);
    messages.length = 0;
    await referenceContext.self.onmessage({
      data: {
        engine,
        formats: commonFormats,
        width: qrFixture.width,
        height: qrFixture.height,
        buffer: qrFixture.pixels.slice().buffer,
        zxingJSSettings: { harder: false, rotate: false, downscale: false, invert: false },
      },
    });
    assert.ok(messages.at(-1).result.regions.some((region) => region.text === qrFixture.text));
    console.log("ZXing-JS: rotated inverted EAN and basic QR decoding verified");
  }
  console.log(`${engine}: production comparison worker switches formats and enforces coverage`);
}

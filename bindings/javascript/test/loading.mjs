import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Scanner } from "../dist/index.js";

const bytes = await readFile(new URL(import.meta.resolve("tapirscan/wasm/medium.wasm")));
const image = { data: new Uint8Array(100).fill(255), width: 10, height: 10, channels: 1 };

async function createAndDispose(wasmBaseUrl) {
  const scanner = await Scanner.create({ wasmBaseUrl });
  try {
    assert.deepEqual(scanner.scan(image).values, []);
  } finally {
    scanner.dispose();
  }
}

test("stable exports resolve to a working scanner for each mode", async () => {
  for (const mode of ["low", "medium", "high", "very-high"]) {
    const scanner = await Scanner.create({
      mode,
      loadWasm: async () => {
        const data = await readFile(new URL(import.meta.resolve(`tapirscan/wasm/${mode}.wasm`)));
        return data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
      },
    });
    try {
      assert.deepEqual(scanner.scan(image).values, []);
    } finally {
      scanner.dispose();
    }
  }
});

test("default loader shares concurrent and completed loads, with independent sessions", async (t) => {
  let calls = 0;
  t.mock.method(globalThis, "fetch", async () => {
    calls++;
    return new Response(bytes);
  });
  const options = { wasmBaseUrl: "https://cache.test/shared/" };
  const scanners = await Promise.all([Scanner.create(options), Scanner.create(options)]);
  try {
    assert.equal(calls, 1);
    scanners[0].dispose();
    assert.deepEqual(scanners[1].scan(image).values, []);
    await createAndDispose(options.wasmBaseUrl);
    assert.equal(calls, 1);
    await createAndDispose("https://cache.test/other/");
    assert.equal(calls, 2);
  } finally {
    for (const scanner of scanners) scanner.dispose();
  }
});

test("failed HTTP and network loads can be retried", async (t) => {
  let calls = 0;
  t.mock.method(globalThis, "fetch", async () => {
    calls++;
    if (calls === 1) return new Response("unavailable", { status: 503 });
    if (calls === 2) throw new Error("offline");
    return new Response(bytes);
  });
  const wasmBaseUrl = "https://cache.test/retry/";
  await assert.rejects(Scanner.create({ wasmBaseUrl }), /503/);
  await assert.rejects(Scanner.create({ wasmBaseUrl }), /offline/);
  await createAndDispose(wasmBaseUrl);
  assert.equal(calls, 3);
});

test("cache retention is bounded across changing asset URLs", async (t) => {
  const urls = [];
  t.mock.method(globalThis, "fetch", async (url) => {
    urls.push(url.href);
    return new Response(bytes);
  });
  for (let i = 0; i < 5; i++) await createAndDispose(`https://cache.test/bounded/${i}/`);
  await createAndDispose("https://cache.test/bounded/4/");
  assert.equal(urls.length, 5);
  await createAndDispose("https://cache.test/bounded/0/");
  assert.equal(urls.length, 6);
  assert.equal(urls[0], urls[5]);
});

test("custom loaders retain control over repeated loads", async () => {
  let calls = 0;
  for (let i = 0; i < 2; i++) {
    const scanner = await Scanner.create({
      loadWasm: () => {
        calls++;
        return Promise.resolve(
          bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
        );
      },
    });
    scanner.dispose();
  }
  assert.equal(calls, 2);
});

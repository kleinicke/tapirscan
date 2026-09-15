// Compare a relocated WASM build against the exact research artifact.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
const [reference, candidate, file, w, h, mask] = process.argv.slice(2);
async function scan(path) {
  const { instance } = await WebAssembly.instantiate(await readFile(path), {});
  const e = instance.exports;
  const handle = e.multi_new();
  try {
    assert.equal(e.multi_prepare(handle, +w, +h), 0);
    new Uint8Array(e.memory.buffer, e.multi_input(handle), +w * +h).set(await readFile(file));
    assert.equal(e.multi_scan(handle, +mask, 1), 0);
    return JSON.parse(
      new TextDecoder().decode(
        new Uint8Array(e.memory.buffer, e.multi_output(handle), e.multi_output_len(handle)),
      ),
    );
  } finally {
    e.multi_free(handle);
  }
}
assert.deepEqual(await scan(candidate), await scan(reference));

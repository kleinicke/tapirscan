// CLI used by the cross-language parity suite; not an additional node:test case.
// usage: mode width height channels stride pixels [debug formats [extendedBudget]]
// Prints the same typed JSON as the C++, Java and Rust scan_raw examples.
import { readFile } from "node:fs/promises";
import { Scanner } from "../dist/index.js";
const [mode, width, height, channels, stride, file, debug, formats, extendedBudget] =
  process.argv.slice(2);
const scanner = await Scanner.create({
  mode,
  ...(formats ? { formats: formats.split(",") } : {}),
  loadWasm: async (url) => {
    const b = await readFile(url);
    return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
  },
});
try {
  const result = scanner.inspect(
    {
      data: new Uint8Array(await readFile(file)),
      width: +width,
      height: +height,
      channels: +channels,
      stride: +stride,
    },
    { extendedBudget: extendedBudget === "1" },
  );
  const best = result.best === undefined ? null : result.barcodes.indexOf(result.best);
  console.log(
    JSON.stringify({
      mode: result.mode,
      best,
      barcodes: result.barcodes.map(({ text, format, support, polygon }) => ({
        text,
        format,
        support,
        polygon,
      })),
      undecoded: result.undecoded.map(({ format, polygon }) => ({ format, polygon })),
      // Drop the JS-only regions view so the raw engine JSON is comparable.
      debug: debug === "1" ? (({ regions: _, ...raw }) => raw)(result.diagnostics) : null,
    }),
  );
} finally {
  scanner.dispose();
}

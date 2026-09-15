// CLI used by the cross-language parity suite; not an additional node:test case.
import { readFile } from "node:fs/promises";
import { Scanner } from "../dist/index.js";
const [mode, width, height, channels, stride, file, multiple, includeRegions, formats] =
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
  const result = scanner.scan(
    {
      data: new Uint8Array(await readFile(file)),
      width: +width,
      height: +height,
      channels: +channels,
      stride: +stride,
    },
    {
      multiple: multiple === undefined ? true : multiple === "1",
      debug: true,
    },
  );
  const raw = result.debug;
  if (includeRegions !== "1") {
    for (const key of ["localization", "recovery", "detailRegions", "searchWindows"])
      delete raw[key];
    raw.scan = { barcodes: raw.scan.barcodes, unfinished: raw.scan.unfinished };
  }
  console.log(JSON.stringify(raw));
} finally {
  scanner.dispose();
}

import * as zx from "zxing-wasm/reader";
import * as zb from "@undecaf/zbar-wasm";
import type { Format } from "tapirscan";
import type { Region } from "./types";

let ready = false;
let zbar: zb.ZBarScanner;
const zbarFormats: Partial<Record<Format, zb.ZBarSymbolType>> = {
  EAN13: zb.ZBarSymbolType.ZBAR_EAN13,
  UPCA: zb.ZBarSymbolType.ZBAR_UPCA,
  EAN8: zb.ZBarSymbolType.ZBAR_EAN8,
  UPCE: zb.ZBarSymbolType.ZBAR_UPCE,
  Code128: zb.ZBarSymbolType.ZBAR_CODE128,
  Code39: zb.ZBarSymbolType.ZBAR_CODE39,
  Code93: zb.ZBarSymbolType.ZBAR_CODE93,
  ITF: zb.ZBarSymbolType.ZBAR_I25,
  Codabar: zb.ZBarSymbolType.ZBAR_CODABAR,
  QRCode: zb.ZBarSymbolType.ZBAR_QRCODE,
  DataBar: zb.ZBarSymbolType.ZBAR_DATABAR,
  DataBarExpanded: zb.ZBarSymbolType.ZBAR_DATABAR_EXP,
};
self.onmessage = async ({
  data,
}: MessageEvent<{
  engine: "zxing" | "zbar";
  formats: Format[];
  zxingEnhanced?: boolean;
  engineBaseUrl: string;
  width: number;
  height: number;
  buffer: ArrayBuffer;
}>) => {
  try {
    if (!ready) {
      self.postMessage({ type: "initializing" });
      if (data.engine === "zxing") {
        const response = await fetch(new URL("zxing_reader.wasm", data.engineBaseUrl));
        if (!response.ok) throw Error("ZXing engine could not be loaded");
        await zx.prepareZXingModule({
          overrides: { wasmBinary: new Uint8Array(await response.arrayBuffer()) },
          fireImmediately: true,
        });
      } else {
        zb.setModuleArgs({
          locateFile: (name) =>
            new URL(name.endsWith(".wasm") ? "zbar.wasm" : name, data.engineBaseUrl).href,
        });
        zbar = await zb.ZBarScanner.create();
        zbar.enableCache(false);
      }
      ready = true;
    }
    if (data.engine === "zbar") {
      if (!data.formats.some((format) => zbarFormats[format] !== undefined))
        throw Error("ZBar does not support any of the selected formats.");
      zbar.setConfig(zb.ZBarSymbolType.ZBAR_NONE, zb.ZBarConfigType.ZBAR_CFG_ENABLE, 0);
      for (const format of data.formats) {
        const symbol = zbarFormats[format];
        if (symbol !== undefined) zbar.setConfig(symbol, zb.ZBarConfigType.ZBAR_CFG_ENABLE, 1);
      }
    }
    const start = performance.now();
    let regions: Region[];
    let unfinished = false;
    if (data.engine === "zxing") {
      const reads = await zx.readBarcodes(
        new ImageData(new Uint8ClampedArray(data.buffer), data.width, data.height),
        {
          formats: data.formats,
          tryHarder: data.zxingEnhanced ?? true,
          tryRotate: data.zxingEnhanced ?? true,
          tryDownscale: data.zxingEnhanced ?? true,
          maxNumberOfSymbols: 255,
        },
      );
      regions = reads
        .filter((read) => read.isValid)
        .map((read) => ({
          text: read.text,
          polygon: [
            read.position.topLeft,
            read.position.topRight,
            read.position.bottomRight,
            read.position.bottomLeft,
          ].map((p) => [p.x, p.y] as const),
        }));
      unfinished = reads.length === 255;
    } else {
      const reads = await zb.scanRGBABuffer(data.buffer, data.width, data.height, zbar);
      regions = reads.map((read) => ({
        text: read.decode(),
        polygon: hull(read.points.map((p) => [p.x, p.y] as const)),
      }));
    }
    self.postMessage({
      result: {
        regions,
        proposals: [],
        width: data.width,
        height: data.height,
        scanMs: performance.now() - start,
        unfinished,
      },
    });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};

// ZBar reports sample points; their hull preserves the reported source geometry.
function hull(input: readonly (readonly [number, number])[]): (readonly [number, number])[] {
  const points = [...input].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  if (points.length < 3) return points;
  const cross = (a: readonly number[], b: readonly number[], c: readonly number[]) =>
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
  const half = (values: typeof points) => {
    const result: typeof points = [];
    for (const point of values) {
      while (result.length > 1) {
        const previous = result.at(-2);
        const last = result.at(-1);
        if (!previous || !last || cross(previous, last, point) > 0) break;
        result.pop();
      }
      result.push(point);
    }
    result.pop();
    return result;
  };
  return [...half(points), ...half([...points].reverse())];
}

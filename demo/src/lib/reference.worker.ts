import { scanZXingJS, type ZXingJSSettings } from "./zxing-js";
import jsQR from "jsqr";
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
  engine: "zxing" | "zxingdefault" | "zbar" | "jsqr" | "native" | "zxingjs" | "zxingjsdefault";
  zxingJSSettings?: ZXingJSSettings;
  formats: Format[];
  zxingEnhanced?: boolean;
  benchmark?: boolean;
  engineBaseUrl: string;
  width: number;
  height: number;
  buffer: ArrayBuffer;
}>) => {
  try {
    if (data.engine === "zxingjs" || data.engine === "zxingjsdefault") {
      self.postMessage({
        result: scanZXingJS(
          new Uint8ClampedArray(data.buffer),
          data.width,
          data.height,
          data.formats,
          data.engine === "zxingjsdefault"
            ? { harder: false, rotate: false, downscale: false, invert: false }
            : (data.zxingJSSettings ?? {
                harder: true,
                rotate: true,
                downscale: false,
                invert: false,
              }),
        ),
      });
      return;
    }
    if (!ready) {
      self.postMessage({ type: "initializing" });
      if (data.engine === "zxing" || data.engine === "zxingdefault") {
        const response = await fetch(new URL("zxing_reader.wasm", data.engineBaseUrl));
        if (!response.ok) throw Error("ZXing engine could not be loaded");
        await zx.prepareZXingModule({
          overrides: { wasmBinary: new Uint8Array(await response.arrayBuffer()) },
          fireImmediately: true,
        });
      } else if (data.engine === "zbar") {
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
      if (data.benchmark) {
        zbar.destroy();
        zbar = await zb.ZBarScanner.create();
        zbar.enableCache(false);
      }
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
    if (data.engine === "zxing" || data.engine === "zxingdefault") {
      const reads = await zx.readBarcodes(
        new ImageData(new Uint8ClampedArray(data.buffer), data.width, data.height),
        data.engine === "zxingdefault"
          ? { formats: data.formats, tryHarder: false, tryRotate: false, tryDownscale: false }
          : {
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
      unfinished = reads.length === zx.defaultReaderOptions.maxNumberOfSymbols;
    } else if (data.engine === "jsqr") {
      if (!data.formats.includes("QRCode"))
        throw Error("jsQR supports QR Code only. Select Common or All.");
      const read = jsQR(new Uint8ClampedArray(data.buffer), data.width, data.height);
      regions = read
        ? [
            {
              text: read.data,
              polygon: [
                read.location.topLeftCorner,
                read.location.topRightCorner,
                read.location.bottomRightCorner,
                read.location.bottomLeftCorner,
              ].map((p) => [p.x, p.y] as const),
            },
          ]
        : [];
      // jsQR returns at most one symbol; do not imply exhaustive multi-code coverage.
      unfinished = !!read;
    } else if (data.engine === "native") {
      const Detector = (self as unknown as { BarcodeDetector?: NativeDetectorConstructor })
        .BarcodeDetector;
      if (!Detector)
        throw Error("Native BarcodeDetector is unavailable in this browser. No fallback is used.");
      const supported = await Detector.getSupportedFormats();
      const formats = data.formats
        .map((format) => nativeFormats[format])
        .filter((format): format is string => !!format && supported.includes(format));
      if (!formats.length)
        throw Error("Native BarcodeDetector supports none of the selected formats on this device.");
      const detector = new Detector({ formats });
      const reads = await detector.detect(
        new ImageData(new Uint8ClampedArray(data.buffer), data.width, data.height),
      );
      regions = reads.map((read) => ({
        text: read.rawValue,
        polygon: read.cornerPoints.map((p) => [p.x, p.y] as const),
      }));
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

interface NativeDetectorConstructor {
  new (options: { formats: string[] }): {
    detect(
      image: ImageData,
    ): Promise<{ rawValue: string; cornerPoints: { x: number; y: number }[] }[]>;
  };
  getSupportedFormats(): Promise<string[]>;
}
const nativeFormats: Partial<Record<Format, string>> = {
  EAN13: "ean_13",
  EAN8: "ean_8",
  UPCA: "upc_a",
  UPCE: "upc_e",
  Code128: "code_128",
  Code39: "code_39",
  Code93: "code_93",
  Codabar: "codabar",
  ITF: "itf",
  QRCode: "qr_code",
  DataMatrix: "data_matrix",
  PDF417: "pdf417",
  Aztec: "aztec",
};

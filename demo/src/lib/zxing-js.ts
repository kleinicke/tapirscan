import {
  MultiFormatReader,
  BarcodeFormat,
  DecodeHintType,
  RGBLuminanceSource,
  HybridBinarizer,
  BinaryBitmap,
  NotFoundException,
  ChecksumException,
  FormatException,
} from "@zxing/library";
import type { Format } from "tapirscan";
import type { Result, Region } from "./types";
export interface ZXingJSSettings {
  harder: boolean;
  rotate: boolean;
  downscale: boolean;
  invert: boolean;
}
const mapping: Partial<Record<Format, BarcodeFormat>> = {
  EAN13: BarcodeFormat.EAN_13,
  EAN8: BarcodeFormat.EAN_8,
  UPCA: BarcodeFormat.UPC_A,
  UPCE: BarcodeFormat.UPC_E,
  Code128: BarcodeFormat.CODE_128,
  Code39: BarcodeFormat.CODE_39,
  Code93: BarcodeFormat.CODE_93,
  Codabar: BarcodeFormat.CODABAR,
  ITF: BarcodeFormat.ITF,
  QRCode: BarcodeFormat.QR_CODE,
  DataMatrix: BarcodeFormat.DATA_MATRIX,
  PDF417: BarcodeFormat.PDF_417,
  Aztec: BarcodeFormat.AZTEC,
  DataBar: BarcodeFormat.RSS_14,
  DataBarExpanded: BarcodeFormat.RSS_EXPANDED,
  MaxiCode: BarcodeFormat.MAXICODE,
};
export function scanZXingJS(
  data: Uint8ClampedArray,
  width: number,
  height: number,
  formats: readonly Format[],
  settings: ZXingJSSettings,
): Result {
  const selected = formats
    .map((f) => mapping[f])
    .filter((f): f is BarcodeFormat => f !== undefined);
  if (!selected.length) throw new Error("ZXing-JS supports none of the selected formats.");
  const start = performance.now();
  const reader = new MultiFormatReader();
  const hints = new Map<DecodeHintType, unknown>([[DecodeHintType.POSSIBLE_FORMATS, selected]]);
  if (settings.harder) hints.set(DecodeHintType.TRY_HARDER, true);
  const regions: Region[] = [];
  // At most 16 full-image passes. Each returns at most one symbol.
  for (const step of settings.downscale ? [1, 2] : [1]) {
    const w = Math.ceil(width / step),
      h = Math.ceil(height / step);
    for (let turn = 0; turn < (settings.rotate ? 4 : 1); turn++) {
      const rw = turn % 2 ? h : w,
        rh = turn % 2 ? w : h;
      const gray = new Uint8ClampedArray(rw * rh);
      const sourcePoint = (x: number, y: number): readonly [number, number] => {
        const point =
          turn === 0
            ? [x, y]
            : turn === 1
              ? [y, h - 1 - x]
              : turn === 2
                ? [w - 1 - x, h - 1 - y]
                : [w - 1 - y, x];
        return [Math.min(width - 1, point[0] * step), Math.min(height - 1, point[1] * step)];
      };
      for (let y = 0; y < rh; y++)
        for (let x = 0; x < rw; x++) {
          const [sx, sy] = sourcePoint(x, y),
            i = (sy * width + sx) * 4;
          gray[y * rw + x] = (data[i] + 2 * data[i + 1] + data[i + 2]) / 4;
        }
      const source = new RGBLuminanceSource(gray, rw, rh);
      for (const inverted of settings.invert ? [false, true] : [false]) {
        try {
          const result = reader.decode(
            new BinaryBitmap(new HybridBinarizer(inverted ? source.invert() : source)),
            hints,
          );
          const polygon = result
            .getResultPoints()
            .map((point) => sourcePoint(point.getX(), point.getY()));
          const text = result.getText();
          const center = (points: Region["polygon"]) =>
            points.reduce(
              (sum, p) => [sum[0] + p[0] / points.length, sum[1] + p[1] / points.length],
              [0, 0],
            );
          const c = center(polygon);
          if (
            !regions.some((region) => {
              const r = center(region.polygon);
              return region.text === text && Math.hypot(c[0] - r[0], c[1] - r[1]) < 24;
            })
          )
            regions.push({ text, polygon });
        } catch (error) {
          if (!(
            error instanceof NotFoundException ||
            error instanceof ChecksumException ||
            error instanceof FormatException
          ))
            throw error;
        } finally {
          reader.reset();
        }
      }
    }
  }
  return {
    regions,
    proposals: [],
    width,
    height,
    scanMs: performance.now() - start,
    unfinished: true,
  };
}

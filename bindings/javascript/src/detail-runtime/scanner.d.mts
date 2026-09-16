import type { Image, Policy, Quad } from "../host.js";
import type { DetailResult } from "../detail-20260914/scanner.mjs";
export { detailRegions } from "../detail-20260914/scanner.mjs";
export type { DetailResult, Recovery } from "../detail-20260914/scanner.mjs";

export class DetailScanner {
  static create(
    primaryBytes: ArrayBuffer,
    recoveryBytes: ArrayBuffer,
    directions: 1 | 2 | 3,
  ): Promise<DetailScanner>;
  scanLocalized(
    image: Image,
    policy: Policy,
    fitLimit: number,
    fullFrame: boolean,
    coverage?: readonly Quad[],
  ): DetailResult;
  dispose(): void;
}

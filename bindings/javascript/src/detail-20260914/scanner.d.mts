import type { Image, Policy, ScanFrame, Quad, IndependentScanner } from "../host.js";
type Localized = ReturnType<IndependentScanner["scanLocalized"]>;
export interface DetailRegion {
  text: string;
  polygon: Quad;
  support?: number;
  score?: number;
}
export interface RecoveryAttempt {
  x: number;
  y: number;
  w: number;
  h: number;
  factor: number;
  frame: ScanFrame;
  reads: ScanFrame["barcodes"];
  deferredReads: ScanFrame["barcodes"];
  proposals: DetailRegion[];
  unfinished: boolean;
}
export interface Recovery {
  barcodes: ScanFrame["barcodes"];
  additions: ScanFrame["barcodes"];
  attempts: RecoveryAttempt[];
  proposals: DetailRegion[];
  extraMs: number;
  searchLimited: boolean;
}
export type DetailResult = Localized & { recovery: Recovery; detailRegions: DetailRegion[] };
export class DetailScanner {
  static create(
    primaryBytes: ArrayBuffer,
    recoveryBytes: ArrayBuffer,
    directions: 1 | 2,
  ): Promise<DetailScanner>;
  scanLocalized(image: Image, policy: Policy, fitLimit: number, fullFrame: boolean): DetailResult;
  dispose(): void;
}

/** Independent EAN-13 supplied-region scanner runtime. */
export type Point = readonly [number, number];
export type Quad = readonly [Point, Point, Point, Point];
export interface Image {
  data: Uint8Array;
  width: number;
  height: number;
  channels: 1 | 3 | 4;
  stride: number;
}
export interface Policy {
  transitionCleanup?: boolean;
  sourceIdentity?: boolean;
  interiorNormalization?: boolean;
  guardBias?: boolean;
  allowSingleRow?: boolean;
  finishCandidates?: boolean;
  retryMask?: readonly [number, number];
  retailMask?: number;
  maxRetryPathsPerCandidate?: number;
  maxRetryPathsPerFrame?: number;
  maxAssociationChecks?: number;
  maxAssociationPixels?: number;
  maxResults?: number;
}
export interface Detection {
  text: string;
  polygon: Quad;
  support: number;
  axis: number;
}
export interface Barcode extends Detection {
  candidate_indices: number[];
}
export interface Observation {
  text: string;
  axis: number;
  fraction: number;
  left: number;
  right: number;
  cost: number;
  gap: number;
}
export interface Candidate {
  candidate_index: number;
  coverage: readonly (readonly [number | null, number | null])[];
  error: boolean;
  error_detail: "invalid_geometry" | "sampling_error" | null;
  unfinished: boolean;
  ms: number;
  work: Record<string, number>;
  detections: Detection[];
  observations: Observation[];
}
export interface ScanFrame {
  unfinished: boolean;
  candidates: Candidate[];
  barcodes: Barcode[];
  reconciliation: {
    comparisons: number;
    merged: number;
    ambiguous: number;
    conflicting: number;
    pending_observations: number;
    truncated: boolean;
    source_pairs: number;
    source_matches: number;
    source_pixels: number;
    source_capped: number;
    pending_coverage_checks: number;
    pending_quarantined: number;
  };
  retail?: {
    barcodes: Barcode[];
    unfinished: boolean;
  };
}
export interface ScanResult extends ScanFrame {
  candidateTimingsAvailable: false;
  elapsedMs: number;
}
export interface Localization {
  proposals: { polygon: Quad; score: number; text: string }[];
  omitted: number;
  workLimited: boolean;
  retryMask?: readonly [number, number];
  [key: string]: unknown;
}
export interface LocalizedResult {
  localization: Localization;
  searchWindows: { kind: string; polygon: number[][]; candidateIndex: number }[];
  scan: ScanResult;
  localizationMs: number;
  decodingMs: number;
  scanMs: number;
}
export interface RuntimeHostOptions {
  readonly maxProposals: number;
  readonly scanAbi: "basic" | "masked";
  readonly completion: boolean;
  readonly retail: boolean;
}
export const runtimeHostOptions: Readonly<{
  base: RuntimeHostOptions;
  completion: RuntimeHostOptions;
  wide: RuntimeHostOptions;
  detail: RuntimeHostOptions;
}>;
export function isQuadShape(value: unknown): value is Quad;
export class ScannerError extends Error {
  readonly code: string;
}
export function snapshotImage(image: Image): Image;
export function snapshotPolicy(policy: Policy): Policy;
export function rankBarcodes(barcodes: readonly Barcode[]): Barcode[];
export class IndependentScanner {
  static create(bytes: BufferSource, options?: RuntimeHostOptions): Promise<IndependentScanner>;
  scan(image: Image, quads: readonly Quad[], policy?: Policy): ScanResult;
  scanLocalized(
    image: Image,
    policy?: Policy,
    fitLimit?: number,
    fullFrame?: boolean,
    transform?: (localization: Localization) => Localization,
  ): LocalizedResult;
  best(result: ScanFrame): Barcode | undefined;
  dispose(): void;
}

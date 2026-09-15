import type { Recovery, DetailRegion } from "./detail-20260914/scanner.mjs";
import { ReleaseDetailScanner, fitLimits } from "./detail.js";
import { policy } from "./policy.js";
import { MediumMultiformatScanner, type Barcode as FormatBarcode } from "./multiformat/scanner.js";
import { resolveFormats, type Format, type FormatSelection } from "./multiformat/formats.js";
export { formatBits, linearFormats, matrixFormats, retailFormats } from "./multiformat/formats.js";
export type { Format, FormatSelection } from "./multiformat/formats.js";
import { IndependentScanner as WideScanner } from "./host64.js";
import { IndependentScanner, type Image, type ScanFrame, type Quad } from "./host.js";
export type { Image, Quad } from "./host.js";
export { ScannerError } from "./host.js";
export type DiagnosticBarcode =
  FormatBarcode | (ScanFrame["barcodes"][number] & { format: "EAN13" });
export type Mode = "low" | "medium" | "high" | "very-high";
export interface ScanOptions {
  multiple?: boolean;
  debug?: boolean;
  /** Compatibility alias; use debug. Decoded polygons are always returned. */
  includeRegions?: boolean;
}
export interface Diagnostics {
  schemaVersion: 2;
  mode: Mode;
  multiple: boolean;
  elapsedMs: number;
  localizationLimited: boolean;
  scan: {
    barcodes: DiagnosticBarcode[];
    unfinished: boolean;
    regions?: FormatBarcode[];
  } & Partial<Omit<ScanFrame, "barcodes" | "unfinished">>;
  localization?: {
    proposals: { polygon: Quad; score: number; text: string }[];
    omitted: number;
    workLimited: boolean;
    trace?: Record<string, number>;
  };
  recovery?: Recovery;
  detailRegions?: DetailRegion[];
  searchWindows?: { kind: string; polygon: number[][]; candidateIndex: number }[];
}
export interface Barcode {
  text: string;
  format: Format | "Unknown";
  polygon: Quad;
  rect: { left: number; top: number; width: number; height: number };
}
export interface ScanResult {
  barcodes: Barcode[];
  values: string[];
  best: Barcode | undefined;
  image: { width: number; height: number };
  mode: Mode;
  elapsedMs: number;
  unfinished: boolean;
  debug?: Diagnostics;
}
export interface ScannerOptions {
  mode?: Mode;
  formats?: FormatSelection;
  loadWasm?: (url: URL) => Promise<ArrayBuffer>;
}
export type PixelImage = Image | Pick<ImageData, "data" | "width" | "height">;
function pixels(image: PixelImage): Image {
  if ("channels" in image) return image;
  if (!(image.data instanceof Uint8ClampedArray))
    throw new TypeError("Use ImageData or an explicit buffer with channels and stride");
  return {
    data: new Uint8Array(image.data.buffer, image.data.byteOffset, image.data.byteLength),
    width: image.width,
    height: image.height,
    channels: 4,
    stride: image.width * 4,
  };
}
async function loadDefault(url: URL): Promise<ArrayBuffer> {
  if (url.protocol === "file:") {
    const nodeFs = "node:fs/promises";
    const fs = (await import(/* @vite-ignore */ nodeFs)) as {
      readFile: (url: URL) => Promise<Uint8Array>;
    };
    return new Uint8Array(await fs.readFile(url)).buffer;
  }
  const response = await fetch(url);
  if (!response.ok) throw new Error(`WASM load failed: ${String(response.status)}`);
  return response.arrayBuffer();
}
function publicResult(raw: Diagnostics, image: Image, debug: boolean): ScanResult {
  const barcodes = raw.scan.barcodes.map(({ text, format, polygon }) => {
    const left = Math.floor(Math.min(...polygon.map((p) => p[0])));
    const top = Math.floor(Math.min(...polygon.map((p) => p[1])));
    return {
      text,
      format,
      polygon,
      rect: {
        left,
        top,
        width: Math.ceil(Math.max(...polygon.map((p) => p[0]))) - left,
        height: Math.ceil(Math.max(...polygon.map((p) => p[1]))) - top,
      },
    };
  });
  let bestIndex = -1;
  for (let i = 0; i < barcodes.length; i++)
    if (bestIndex < 0 || raw.scan.barcodes[i].support > raw.scan.barcodes[bestIndex].support)
      bestIndex = i;
  return {
    barcodes,
    values: barcodes.map((b) => b.text),
    best: barcodes[bestIndex],
    image: { width: image.width, height: image.height },
    mode: raw.mode,
    elapsedMs: raw.elapsedMs,
    unfinished: raw.scan.unfinished,
    ...(debug ? { debug: raw } : {}),
  };
}
const modes = {
  low: "low-release-20260915.wasm",
  medium: "medium-release-20260915.wasm",
  high: "high-release-20260915.wasm",
  "very-high": "very-high-release-20260915.wasm",
} as const;
/** Mode selects a compiled implementation. Create another instance to switch. */
export class Scanner {
  private constructor(
    private readonly host: IndependentScanner | WideScanner | ReleaseDetailScanner,
    readonly mode: Mode,
    private readonly additional?: MediumMultiformatScanner,
    private readonly formats: readonly Format[] = ["EAN13"],
  ) {}
  static async create(options: ScannerOptions = {}) {
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scanner options");
    for (const key of Object.keys(options))
      if (!["mode", "formats", "loadWasm"].includes(key))
        throw new TypeError(`Unknown scanner option: ${key}`);
    const mode = options.mode ?? "medium";
    if (!Object.hasOwn(modes, mode)) throw new TypeError("Unknown scanner mode");
    const extraAsset = "../wasm/multiformat.wasm";
    const formats = resolveFormats(options.formats);
    const url = new URL("../wasm/" + modes[mode], import.meta.url);
    const load = options.loadWasm ?? loadDefault;
    const bytes = await load(url);
    const recovery =
      mode === "low" ? undefined : await load(new URL("../wasm/" + modes.low, import.meta.url));
    const host = recovery
      ? await ReleaseDetailScanner.create(bytes, recovery, mode as Exclude<Mode, "low">)
      : await IndependentScanner.create(bytes);
    try {
      const extra = formats.some((f) => f !== "EAN13" && f !== "UPCA")
        ? await load(new URL(extraAsset, import.meta.url))
        : undefined;
      const additional =
        formats.length !== 1 || formats[0] !== "EAN13"
          ? await MediumMultiformatScanner.create(bytes, extra, mode, recovery)
          : undefined;
      return new Scanner(host, mode, additional, formats);
    } catch (error) {
      host.dispose();
      throw error;
    }
  }
  scan(inputImage: PixelImage, options: ScanOptions = {}): ScanResult {
    const image = pixels(inputImage);
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scan options");
    for (const key of Object.keys(options)) {
      if (key !== "multiple" && key !== "includeRegions" && key !== "debug")
        throw new TypeError(`Unknown scan option: ${key}`);
    }
    for (const key of ["multiple", "includeRegions", "debug"] as const) {
      if (options[key] !== undefined && typeof options[key] !== "boolean")
        throw new TypeError(`Invalid ${key}`);
    }
    const multiple = options.multiple ?? true;
    if (
      options.debug !== undefined &&
      options.includeRegions !== undefined &&
      options.debug !== options.includeRegions
    )
      throw new TypeError("debug and includeRegions disagree");
    const includeRegions = options.debug ?? options.includeRegions ?? false;
    if (this.additional) {
      const frame = this.additional.scan(image, this.formats);
      const barcodes = multiple ? frame.barcodes : frame.barcodes.slice(0, 1);
      return publicResult(
        {
          schemaVersion: 2,
          mode: this.mode,
          multiple,
          elapsedMs: frame.scanMs,
          localizationLimited: frame.unfinished,
          scan: {
            barcodes,
            unfinished: frame.unfinished,
            ...(includeRegions ? { regions: frame.regions } : {}),
          },
        },
        image,
        includeRegions,
      );
    }
    const full = this.host.scanLocalized(image, policy, fitLimits[this.mode], true);
    // Isolate the imported host's JSON result at the public ABI type boundary.
    const localization = full.localization as NonNullable<Diagnostics["localization"]>;
    const best = multiple ? undefined : this.host.best(full.scan);
    const barcodes = (multiple ? full.scan.barcodes : best ? [best] : []).map((b) => ({
      ...b,
      format: "EAN13" as const,
    }));
    return publicResult(
      {
        schemaVersion: 2,
        mode: this.mode,
        multiple,
        elapsedMs: full.scanMs,
        localizationLimited: localization.workLimited,
        scan: includeRegions
          ? { ...full.scan, barcodes }
          : { barcodes, unfinished: full.scan.unfinished },
        ...(includeRegions
          ? {
              localization,
              searchWindows: full.searchWindows,
              ...("recovery" in full && "detailRegions" in full
                ? {
                    recovery: full.recovery as Recovery,
                    detailRegions: full.detailRegions as DetailRegion[],
                  }
                : {}),
            }
          : {}),
      },
      image,
      includeRegions,
    );
  }
  /** Convenience alias for result.best. */
  best(result: ScanResult) {
    return result.best;
  }
  dispose() {
    this.additional?.dispose();
    this.host.dispose();
  }
}

/** Scan one image with automatic cleanup. Reuse Scanner for a stream of images. */
export async function scan(
  image: PixelImage,
  options: ScannerOptions & ScanOptions = {},
): Promise<ScanResult> {
  const { multiple, debug, includeRegions, ...creation } = options;
  const scanner = await Scanner.create(creation);
  try {
    return scanner.scan(image, { multiple, debug, includeRegions });
  } finally {
    scanner.dispose();
  }
}

import type { Recovery, DetailRegion } from "./detail-20260914/scanner.mjs";
import { ReleaseDetailScanner, fitLimits } from "./detail.js";
import { policy } from "./policy.js";
import { MediumMultiformatScanner, type Barcode as FormatBarcode } from "./multiformat/scanner.js";
import { resolveFormats, type Format, type FormatSelection } from "./multiformat/formats.js";
export {
  commonFormats,
  commonLinearFormats,
  formatBits,
  linearFormats,
  matrixFormats,
  retailFormats,
} from "./multiformat/formats.js";
export type { Format, FormatSelection } from "./multiformat/formats.js";
import {
  IndependentScanner,
  type Image as HostImage,
  type ScanFrame,
  type Quad as HostQuad,
} from "./host.js";
/** Source-image corners, in pixels. */
export type Quad = readonly [
  readonly [number, number],
  readonly [number, number],
  readonly [number, number],
  readonly [number, number],
];
/** Decoded pixels; stride defaults to width * channels. Alpha is ignored. */
export interface Image {
  readonly data: Uint8Array;
  readonly width: number;
  readonly height: number;
  readonly channels: 1 | 3 | 4;
  readonly stride?: number;
}
export { ScannerError } from "./host.js";
type RawDiagnosticBarcode = FormatBarcode | (ScanFrame["barcodes"][number] & { format: "EAN13" });
export type Mode = "low" | "medium" | "high" | "very-high";
export interface ScanOptions {
  /** Per-call subset of the formats configured at creation. */
  formats?: FormatSelection;
  debug?: boolean;
}
interface RawDiagnostics {
  schemaVersion: 2;
  mode: Mode;
  multiple: boolean;
  elapsedMs: number;
  localizationLimited: boolean;
  scan: {
    barcodes: RawDiagnosticBarcode[];
    unfinished: boolean;
    regions?: FormatBarcode[];
  } & Partial<Omit<ScanFrame, "barcodes" | "unfinished">>;
  localization?: {
    proposals: { polygon: HostQuad; score: number; text: string }[];
    omitted: number;
    workLimited: boolean;
    trace?: Record<string, number>;
  };
  recovery?: Recovery;
  detailRegions?: DetailRegion[];
  searchWindows?: { kind: string; polygon: number[][]; candidateIndex: number }[];
}
/** Deeply immutable scan evidence, independent of the scanner lifetime. */
type ReadonlyDeep<T> = T extends object ? { readonly [K in keyof T]: ReadonlyDeep<T[K]> } : T;
export type DiagnosticBarcode = ReadonlyDeep<RawDiagnosticBarcode>;
/** Source geometry with no accepted decode; format is a reader hint. */
export interface UndecodedRegion {
  readonly format: Format | "Unknown";
  readonly polygon: Quad;
}
/** Stable region evidence. null means this reader did not expose that evidence. */
export interface RegionEvidence {
  readonly proposals: ReadonlyDeep<NonNullable<RawDiagnostics["localization"]>["proposals"]> | null;
  readonly searchWindows: ReadonlyDeep<NonNullable<RawDiagnostics["searchWindows"]>> | null;
  readonly undecoded: readonly UndecodedRegion[];
}
export type Diagnostics = ReadonlyDeep<RawDiagnostics> & { readonly regions: RegionEvidence };
export interface StructuredAppend {
  /** One-based symbol index; symbols are not automatically assembled. */
  readonly index: number;
  readonly count: number;
  readonly id?: string;
  readonly parity?: number;
}
export interface Barcode {
  /** Payload bytes before character-set interpretation; absent when unavailable. */
  readonly payloadBytes?: readonly number[];
  readonly text: string;
  readonly format: Format | "Unknown";
  /** Reader-specific ranking evidence, not a probability or cross-reader confidence. */
  readonly support: number;
  readonly gs1?: boolean;
  readonly readerInitialization?: boolean;
  readonly structuredAppend?: StructuredAppend;
  readonly eanAddOn?: string;
  readonly polygon: Quad;
  readonly rect: {
    readonly left: number;
    readonly top: number;
    readonly width: number;
    readonly height: number;
  };
}
export interface ScanResult {
  readonly barcodes: readonly Barcode[];
  readonly values: readonly string[];
  /** Largest reader-specific support; not a cross-format confidence comparison. */
  readonly best: Barcode | undefined;
  readonly image: { readonly width: number; readonly height: number };
  readonly mode: Mode;
  readonly elapsedMs: number;
  readonly unfinished: boolean;
  readonly debug?: Diagnostics;
}
export type EanAddOnPolicy = "Ignore" | "Read" | "Require";
export interface ScannerOptions {
  /** Optional EAN/UPC supplement policy, fixed at creation. */
  eanAddOnPolicy?: EanAddOnPolicy;
  mode?: Mode;
  formats?: FormatSelection;
  /** Directory containing the packaged WASMs; relative to the page in browsers. */
  wasmBaseUrl?: string | URL;
  /** Advanced loader, receiving URLs resolved against wasmBaseUrl. */
  loadWasm?: (url: URL) => Promise<ArrayBuffer>;
}
export type PixelImage = Image | Pick<ImageData, "data" | "width" | "height">;
function pixels(image: PixelImage): HostImage {
  const input: unknown = image;
  if (input === null || typeof input !== "object")
    throw new TypeError("Expected ImageData or decoded pixels");
  const explicit = "channels" in image;
  if (!explicit && !(image.data instanceof Uint8ClampedArray))
    throw new TypeError("Use ImageData or an explicit buffer with channels");
  const channels = explicit ? image.channels : 4;
  const stride = explicit ? (image.stride ?? image.width * channels) : image.width * 4;
  const required = (image.height - 1) * stride + image.width * channels;
  if (
    !Number.isSafeInteger(image.width) ||
    !Number.isSafeInteger(image.height) ||
    image.width < 3 ||
    image.height < 3 ||
    image.width * image.height > 32 * 1024 * 1024 ||
    ![1, 3, 4].includes(channels) ||
    !Number.isSafeInteger(stride) ||
    stride < image.width * channels ||
    required > 128 * 1024 * 1024 ||
    !(image.data instanceof Uint8Array || image.data instanceof Uint8ClampedArray) ||
    image.data.byteLength < required
  )
    throw new TypeError("Invalid image dimensions, channels, stride or buffer (maximum 128 MiB)");
  return {
    data: new Uint8Array(image.data.buffer, image.data.byteOffset, image.data.byteLength),
    width: image.width,
    height: image.height,
    channels,
    stride,
  };
}
/** Freeze only plain result data, never caller-owned input buffers. */
function freeze<T extends object>(value: T): ReadonlyDeep<T> {
  for (const child of Object.values(value))
    if (child !== null && typeof child === "object") freeze(child);
  return Object.freeze(value) as ReadonlyDeep<T>;
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
function regionEvidence(raw: RawDiagnostics): RegionEvidence {
  const undecoded: UndecodedRegion[] = [];
  if (raw.scan.regions) {
    for (const region of raw.scan.regions) {
      if (!raw.scan.barcodes.includes(region))
        undecoded.push({ format: region.format, polygon: region.polygon });
    }
  } else {
    const decoded = new Set(
      raw.scan.barcodes.flatMap((b) => ("candidate_indices" in b ? b.candidate_indices : [])),
    );
    raw.localization?.proposals.forEach((p, i) => {
      if (!decoded.has(i)) undecoded.push({ format: "Unknown", polygon: p.polygon });
    });
    for (const attempt of raw.recovery?.attempts ?? []) {
      const decoded = new Set(attempt.reads.flatMap((b) => b.candidate_indices));
      attempt.proposals.forEach((p, i) => {
        if (!decoded.has(i)) undecoded.push({ format: "Unknown", polygon: p.polygon });
      });
    }
  }
  return {
    proposals: raw.localization?.proposals ?? null,
    searchWindows: raw.searchWindows ?? null,
    undecoded,
  };
}

function publicResult(raw: RawDiagnostics, image: HostImage, debug: boolean): ScanResult {
  const barcodes = raw.scan.barcodes.map((read) => {
    const { text, format, polygon, support } = read;
    const left = Math.floor(Math.min(...polygon.map((p) => p[0])));
    const top = Math.floor(Math.min(...polygon.map((p) => p[1])));
    return {
      text,
      format,
      polygon,
      support,
      ...("bytes" in read ? { payloadBytes: read.bytes } : {}),
      ...("gs1" in read ? { gs1: read.gs1 } : {}),
      ...("readerInitialization" in read
        ? { readerInitialization: read.readerInitialization }
        : {}),
      ...("structuredAppend" in read ? { structuredAppend: read.structuredAppend } : {}),
      ...("eanAddOn" in read ? { eanAddOn: read.eanAddOn } : {}),
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
  return freeze({
    barcodes,
    values: barcodes.map((b) => b.text),
    best: barcodes[bestIndex],
    image: { width: image.width, height: image.height },
    mode: raw.mode,
    elapsedMs: raw.elapsedMs,
    unfinished:
      raw.scan.unfinished || raw.localizationLimited || (raw.localization?.omitted ?? 0) > 0,
    ...(debug ? { debug: { ...raw, regions: regionEvidence(raw) } } : {}),
  });
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
    private readonly host: IndependentScanner | ReleaseDetailScanner | MediumMultiformatScanner,
    readonly mode: Mode,
    private readonly configuredFormats: readonly Format[] = ["EAN13"],
    private readonly addOnPolicy: EanAddOnPolicy = "Ignore",
  ) {
    Object.freeze(configuredFormats);
  }

  /** Formats available for scanning, fixed at creation. */
  get formats(): readonly Format[] {
    return this.configuredFormats;
  }
  get eanAddOnPolicy(): EanAddOnPolicy {
    return this.addOnPolicy;
  }
  static async create(options: ScannerOptions = {}) {
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scanner options");
    for (const key of Object.keys(options))
      if (!["mode", "formats", "wasmBaseUrl", "loadWasm", "eanAddOnPolicy"].includes(key))
        throw new TypeError(`Unknown scanner option: ${key}`);
    const mode = options.mode ?? "medium";
    if (!Object.hasOwn(modes, mode)) throw new TypeError("Unknown scanner mode");
    const formats = resolveFormats(options.formats);
    const addonPolicy = options.eanAddOnPolicy === undefined ? "Ignore" : options.eanAddOnPolicy;
    if (!["Ignore", "Read", "Require"].includes(addonPolicy))
      throw new TypeError("eanAddOnPolicy must be Ignore, Read or Require");
    if (options.loadWasm !== undefined && typeof options.loadWasm !== "function")
      throw new TypeError("loadWasm must be a function");
    if (
      options.wasmBaseUrl !== undefined &&
      typeof options.wasmBaseUrl !== "string" &&
      !(options.wasmBaseUrl instanceof URL)
    )
      throw new TypeError("wasmBaseUrl must be a string or URL");
    const base =
      options.wasmBaseUrl === undefined
        ? new URL(/* @vite-ignore */ "../wasm/", import.meta.url)
        : new URL(
            options.wasmBaseUrl,
            typeof location === "undefined" ? import.meta.url : location.href,
          );
    if (!base.pathname.endsWith("/")) base.pathname += "/";
    const load = options.loadWasm ?? loadDefault;
    const needsPrimary = formats.some((f) => f === "EAN13" || f === "UPCA");
    const bytes = needsPrimary ? await load(new URL(modes[mode], base)) : undefined;
    const recovery =
      !needsPrimary || mode === "low" ? undefined : await load(new URL(modes.low, base));
    const multiformat = addonPolicy !== "Ignore" || formats.length !== 1 || formats[0] !== "EAN13";
    const extra =
      addonPolicy !== "Ignore" || formats.some((f) => f !== "EAN13" && f !== "UPCA")
        ? await load(new URL("multiformat.wasm", base))
        : undefined;
    if (multiformat)
      return new Scanner(
        await MediumMultiformatScanner.create(bytes, extra, mode, recovery),
        mode,
        formats,
        addonPolicy,
      );
    if (!bytes) throw new Error("EAN13 engine was not loaded");
    const host = recovery
      ? await ReleaseDetailScanner.create(bytes, recovery, mode as Exclude<Mode, "low">)
      : await IndependentScanner.create(bytes);
    return new Scanner(host, mode, formats);
  }

  scan(inputImage: PixelImage, options: ScanOptions = {}): ScanResult {
    const image = pixels(inputImage);
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scan options");
    for (const key of Object.keys(options))
      if (key !== "debug" && key !== "formats") throw new TypeError(`Unknown scan option: ${key}`);
    if (options.debug !== undefined && typeof options.debug !== "boolean")
      throw new TypeError("debug must be a boolean");
    const debug = options.debug ?? false;
    const formats = options.formats === undefined ? this.formats : resolveFormats(options.formats);
    if (formats.some((format) => !this.formats.includes(format)))
      throw new TypeError(
        `Scan formats must be a subset of configured formats. Requested: ${formats.join(", ")}; configured: ${this.formats.join(", ")}`,
      );
    if (this.host instanceof MediumMultiformatScanner) {
      const frame = this.host.scan(image, formats, { eanAddOnSymbol: this.eanAddOnPolicy });
      const barcodes = frame.barcodes;
      const primary = frame.primary;
      const localization = primary?.localization as RawDiagnostics["localization"];
      return publicResult(
        {
          schemaVersion: 2,
          mode: this.mode,
          multiple: true,
          elapsedMs: frame.scanMs,
          // Preserve the aggregate limit flag when additional readers cannot separate causes.
          localizationLimited: frame.unfinished,
          scan: {
            ...(debug ? primary?.scan : {}),
            barcodes,
            unfinished: frame.unfinished,
            ...(debug ? { regions: frame.regions } : {}),
          },
          ...(debug && primary
            ? {
                localization,
                searchWindows: primary.searchWindows,
                ...("recovery" in primary ? { recovery: primary.recovery } : {}),
                ...("detailRegions" in primary ? { detailRegions: primary.detailRegions } : {}),
              }
            : {}),
        },
        image,
        debug,
      );
    }
    const full = this.host.scanLocalized(image, policy, fitLimits[this.mode], true);
    // Isolate the imported host's JSON result at the public ABI type boundary.
    const localization = full.localization as NonNullable<RawDiagnostics["localization"]>;
    const barcodes = full.scan.barcodes.map((b) => ({
      ...b,
      format: "EAN13" as const,
    }));
    return publicResult(
      {
        schemaVersion: 2,
        mode: this.mode,
        multiple: true,
        elapsedMs: full.scanMs,
        localizationLimited: localization.workLimited || localization.omitted > 0,
        scan: debug ? { ...full.scan, barcodes } : { barcodes, unfinished: full.scan.unfinished },
        ...(debug
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
      debug,
    );
  }
  dispose() {
    this.host.dispose();
  }
}

/** Scan one image with automatic cleanup. Reuse Scanner for a stream of images. */
export async function scan(
  image: PixelImage,
  options: ScannerOptions & ScanOptions = {},
): Promise<ScanResult> {
  const input: unknown = options;
  if (input === null || typeof input !== "object" || Array.isArray(input))
    throw new TypeError("Invalid scan options");
  const { debug, ...creation } = options;
  if (debug !== undefined && typeof debug !== "boolean")
    throw new TypeError("debug must be a boolean");
  const scanner = await Scanner.create(creation);
  try {
    return scanner.scan(image, { debug });
  } finally {
    scanner.dispose();
  }
}

import {
  maskFor,
  resolveFormats,
  type Format,
  type FormatSelection,
} from "./multiformat/formats.js";
import { freeze, type ReadonlyDeep } from "./freeze.js";
import { RustScannerSession, ScannerError } from "./rust-session.js";

export {
  commonFormats,
  commonLinearFormats,
  formatBits,
  linearFormats,
  matrixFormats,
  retailFormats,
} from "./multiformat/formats.js";
export type { Format, FormatSelection } from "./multiformat/formats.js";
export { ScannerError } from "./rust-session.js";

export type Quad = readonly [
  readonly [number, number],
  readonly [number, number],
  readonly [number, number],
  readonly [number, number],
];
export interface Image {
  readonly data: Uint8Array;
  readonly width: number;
  readonly height: number;
  readonly channels: 1 | 3 | 4;
  readonly stride?: number;
}
export type Mode = "low" | "medium" | "high" | "very-high";
export interface ScanOptions {
  /** Allow reader-specific extra work. Supported for every format; exact budgets may evolve. */
  extendedBudget?: boolean;
  /** Per-call subset of the formats configured at creation. */
  formats?: FormatSelection;
}
interface RawDiagnostics {
  scan: { barcodes: RawDiagnosticBarcode[]; unfinished: boolean; candidates?: unknown[] };
  localization?: { proposals: { polygon: Quad; score?: number; text?: string }[] };
  searchWindows?: { kind: string; polygon: number[][]; candidateIndex: number }[];
  [key: string]: unknown;
}
interface RawDiagnosticBarcode {
  text: string;
  format: Format | "Unknown";
  polygon: Quad;
  support: number;
  bytes?: number[];
  candidate_indices?: number[];
  gs1?: boolean;
  readerInitialization?: boolean;
  structuredAppend?: StructuredAppend;
  eanAddOn?: string;
  [key: string]: unknown;
}
export type DiagnosticBarcode = ReadonlyDeep<RawDiagnosticBarcode>;
export interface UndecodedRegion {
  readonly format: Format | "Unknown";
  readonly polygon: Quad;
}
export interface RegionEvidence {
  readonly proposals: ReadonlyDeep<NonNullable<RawDiagnostics["localization"]>["proposals"]> | null;
  readonly searchWindows: ReadonlyDeep<NonNullable<RawDiagnostics["searchWindows"]>> | null;
  readonly undecoded: readonly UndecodedRegion[];
}
export type Diagnostics = ReadonlyDeep<RawDiagnostics> & { readonly regions: RegionEvidence };
export interface StructuredAppend {
  readonly index: number;
  readonly count: number;
  readonly id?: string;
  readonly parity?: number;
}
export interface Barcode {
  /** Payload bytes before character-set interpretation; absent when unavailable. */
  readonly payloadBytes?: readonly number[];
  readonly text: string;
  /** Decoded barcodes always have a known format; only undecoded regions report "Unknown". */
  readonly format: Format;
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
  /** @experimental Selected Turbo preset, when requested. */
  readonly experimentalTurbo?: ExperimentalTurbo;
  /** Whole synchronous WASM call time measured by the JavaScript host. */
  readonly elapsedMs: number;
  readonly unfinished: boolean;
  readonly undecoded: readonly UndecodedRegion[];
  readonly diagnostics: Diagnostics;
}
export type EanAddOnPolicy = "Ignore" | "Read" | "Require";
export type ExperimentalTurbo = 2 | 4 | 8 | 16;
export interface ScannerOptions {
  mode?: Mode;
  /** @experimental Targets faster 1D scanning, not 2D speedups. May change in minor releases. */
  experimentalTurbo?: ExperimentalTurbo;
  /** Optional EAN/UPC supplement policy, fixed at creation. */
  eanAddOnPolicy?: EanAddOnPolicy;
  formats?: FormatSelection;
  /** Directory containing the packaged WASMs; relative to the page in browsers. */
  wasmBaseUrl?: string | URL;
  /** Advanced loader, receiving URLs resolved against wasmBaseUrl. */
  loadWasm?: (url: URL) => Promise<ArrayBuffer>;
}
export type PixelImage = Image | Pick<ImageData, "data" | "width" | "height">;

interface PreparedImage {
  data: Uint8Array;
  width: number;
  height: number;
  channels: 1 | 3 | 4;
  stride: number;
}
type WireBarcode = Barcode;
interface WireResult {
  barcodes: WireBarcode[];
  undecoded: { format?: Format | null; polygon: Quad }[];
  image: { width: number; height: number };
  mode: Mode;
  elapsedMs: number;
  unfinished: boolean;
  debug?: RawDiagnostics;
}

const modes: Record<Mode, { id: number; file: string }> = {
  low: { id: 0, file: "low-vote-sweep-20261006.wasm" },
  medium: { id: 1, file: "medium-vote-sweep-20261006.wasm" },
  high: { id: 2, file: "high-vote-sweep-20261006.wasm" },
  "very-high": { id: 3, file: "very-high-vote-sweep-20261006.wasm" },
};
const turboFiles: Record<ExperimentalTurbo, string> = {
  2: "experimental-turbo2-vote-sweep-20261006.wasm",
  4: "experimental-turbo4-vote-sweep-20261006.wasm",
  8: "experimental-turbo8-vote-sweep-20261006.wasm",
  16: "experimental-turbo16-vote-sweep-20261006.wasm",
};
const addOnPolicies: Record<EanAddOnPolicy, number> = { Ignore: 0, Read: 1, Require: 2 };

function pixels(image: PixelImage): PreparedImage {
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

// Bound retained WASM bytes across stable modes, experimental presets and custom URLs.
const wasmLoads = new Map<string, Promise<ArrayBuffer>>();
function loadDefault(url: URL): Promise<ArrayBuffer> {
  const key = url.href;
  const cached = wasmLoads.get(key);
  if (cached) return cached;
  const pending = readWasm(url).catch((error: unknown) => {
    if (wasmLoads.get(key) === pending) wasmLoads.delete(key);
    throw error;
  });
  wasmLoads.set(key, pending);
  if (wasmLoads.size > 4) {
    const oldest = wasmLoads.keys().next().value;
    if (oldest !== undefined) wasmLoads.delete(oldest);
  }
  return pending;
}
async function readWasm(url: URL): Promise<ArrayBuffer> {
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
function wireResult(value: unknown): WireResult {
  if (
    value === null ||
    typeof value !== "object" ||
    !("barcodes" in value) ||
    !Array.isArray(value.barcodes) ||
    !("undecoded" in value) ||
    !Array.isArray(value.undecoded) ||
    !("image" in value) ||
    value.image === null ||
    typeof value.image !== "object" ||
    !("mode" in value) ||
    !Object.hasOwn(modes, String(value.mode)) ||
    !("unfinished" in value) ||
    typeof value.unfinished !== "boolean"
  )
    throw new ScannerError("invalid_output", "Scanner returned an invalid result");
  return value as WireResult;
}
function publicResult(
  raw: WireResult,
  elapsedMs: number,
  experimentalTurbo?: ExperimentalTurbo,
): ScanResult {
  if (!raw.debug) throw new ScannerError("invalid_output", "Scanner omitted requested diagnostics");
  const barcodes = raw.barcodes;
  const undecoded = raw.undecoded.map(({ format, polygon }) => ({
    format: format ?? ("Unknown" as const),
    polygon,
  }));
  const regions = {
    proposals: raw.debug.localization?.proposals ?? null,
    searchWindows: raw.debug.searchWindows ?? null,
    undecoded,
  };
  return freeze({
    barcodes,
    values: barcodes.map((b) => b.text),
    best: best(barcodes),
    image: raw.image,
    mode: raw.mode,
    ...(experimentalTurbo === undefined ? {} : { experimentalTurbo }),
    elapsedMs,
    unfinished: raw.unfinished,
    undecoded,
    diagnostics: { ...raw.debug, regions },
  });
}

/** A reusable scanner. Results own their data and remain valid after later scans or disposal. */
export class Scanner {
  private constructor(
    private readonly host: RustScannerSession,
    readonly mode: Mode,
    private readonly configuredFormats: readonly Format[],
    private readonly addOnPolicy: EanAddOnPolicy,
    /** @experimental Selected Turbo preset, fixed at creation. */
    readonly experimentalTurbo?: ExperimentalTurbo,
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

  static async create(options: ScannerOptions = {}): Promise<Scanner> {
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scanner options");
    for (const key of Object.keys(options))
      if (
        ![
          "mode",
          "experimentalTurbo",
          "formats",
          "wasmBaseUrl",
          "loadWasm",
          "eanAddOnPolicy",
        ].includes(key)
      )
        throw new TypeError(`Unknown scanner option: ${key}`);
    const turbo = options.experimentalTurbo;
    if (turbo !== undefined) {
      if (typeof turbo !== "number" || !Object.hasOwn(turboFiles, turbo))
        throw new TypeError("experimentalTurbo must be 2, 4, 8 or 16");
      if (options.mode !== undefined)
        throw new TypeError("Choose mode or experimentalTurbo, not both");
      if (options.eanAddOnPolicy !== undefined && options.eanAddOnPolicy !== "Ignore")
        throw new TypeError("experimentalTurbo requires eanAddOnPolicy: Ignore");
    }
    const mode = turbo === undefined ? (options.mode ?? "medium") : "low";
    if (!Object.hasOwn(modes, mode)) throw new TypeError("Unknown scanner mode");
    const formats = resolveFormats(options.formats);
    const addOnPolicy = options.eanAddOnPolicy === undefined ? "Ignore" : options.eanAddOnPolicy;
    if (!Object.hasOwn(addOnPolicies, addOnPolicy))
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
    const selected = modes[mode];
    const bytes = await (options.loadWasm ?? loadDefault)(
      new URL(turbo === undefined ? selected.file : turboFiles[turbo], base),
    );
    const host = await RustScannerSession.create(
      bytes,
      selected.id,
      maskFor(formats),
      addOnPolicies[addOnPolicy],
      turbo,
    );
    return new Scanner(host, mode, formats, addOnPolicy, turbo);
  }

  /** Decode barcodes with positions. Use inspect() for diagnostic evidence. */
  scan(inputImage: PixelImage, options: ScanOptions = {}): readonly Barcode[] {
    const raw = this.run(inputImage, options, false);
    if (
      raw === null ||
      typeof raw !== "object" ||
      !("barcodes" in raw) ||
      !Array.isArray(raw.barcodes)
    )
      throw new ScannerError("invalid_output", "Scanner returned an invalid barcode list");
    return freeze(raw.barcodes as Barcode[]);
  }

  /** Inspect barcodes, unread regions, work status and engine diagnostics. */
  inspect(inputImage: PixelImage, options: ScanOptions = {}): ScanResult {
    const start = performance.now();
    const raw = wireResult(this.run(inputImage, options, true));
    return publicResult(raw, performance.now() - start, this.experimentalTurbo);
  }

  private run(inputImage: PixelImage, options: ScanOptions, inspect: boolean): unknown {
    const image = pixels(inputImage);
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scan options");
    for (const key of Object.keys(options))
      if (!["formats", "extendedBudget"].includes(key))
        throw new TypeError(`Unknown scan option: ${key}`);
    if (options.extendedBudget !== undefined && typeof options.extendedBudget !== "boolean")
      throw new TypeError("extendedBudget must be a boolean");
    if (this.experimentalTurbo !== undefined && options.extendedBudget === true)
      throw new TypeError("experimentalTurbo cannot be combined with extendedBudget: true");
    const formats = options.formats === undefined ? this.formats : resolveFormats(options.formats);
    if (formats.some((format) => !this.formats.includes(format)))
      throw new TypeError(
        `Scan formats must be a subset of configured formats. Requested: ${formats.join(", ")}; configured: ${this.formats.join(", ")}`,
      );
    const flags = (options.extendedBudget ? 1 : 0) | (inspect ? 2 : 0);
    return this.host.scan(image, flags, options.formats === undefined ? 0 : maskFor(formats));
  }

  /** Release the WASM session. Repeated disposal is safe; scanning afterward fails. */
  dispose(): void {
    this.host.dispose();
  }
}

/**
 * Highest support, keeping the first read on ties; undefined when empty. Works on
 * `scan` output and inspection results alike. Support is uncalibrated evidence, so
 * select by format, payload or position when the application knows them.
 */
export function best(barcodes: readonly Barcode[]): Barcode | undefined {
  let winner: Barcode | undefined;
  for (const barcode of barcodes) if (!winner || barcode.support > winner.support) winner = barcode;
  return winner;
}

/** Scan one image with automatic cleanup. Reuse Scanner for a stream of images. */
export async function scan(
  image: PixelImage,
  options: ScannerOptions & ScanOptions = {},
): Promise<readonly Barcode[]> {
  const input: unknown = options;
  if (input === null || typeof input !== "object" || Array.isArray(input))
    throw new TypeError("Invalid scan options");
  const { extendedBudget, ...creation } = options;
  const scanner = await Scanner.create(creation);
  try {
    return scanner.scan(image, { extendedBudget });
  } finally {
    scanner.dispose();
  }
}

/** Inspect one image with automatic cleanup. */
export async function inspect(
  image: PixelImage,
  options: ScannerOptions & ScanOptions = {},
): Promise<ScanResult> {
  const input: unknown = options;
  if (input === null || typeof input !== "object" || Array.isArray(input))
    throw new TypeError("Invalid scan options");
  const { extendedBudget, ...creation } = options;
  const scanner = await Scanner.create(creation);
  try {
    return scanner.inspect(image, { extendedBudget });
  } finally {
    scanner.dispose();
  }
}

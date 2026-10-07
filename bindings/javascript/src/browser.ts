// tapirscan/browser: the core API, run in a bundled worker, for browser image sources.
import type { WorkerRequest, WorkerResponse, WorkerSource } from "./browser-worker.js";
import { freeze } from "./freeze.js";
import { pixelLayout } from "./layout.js";
import type {
  EanAddOnPolicy,
  ExperimentalTurbo,
  FormatSelection,
  Mode,
  PixelImage,
  ScanOptions,
  InspectionResult,
  ScanResult,
} from "./index.js";
import { ScannerError } from "./rust-session.js";

export { ScannerError } from "./rust-session.js";
export type {
  Barcode,
  DiagnosticBarcode,
  Diagnostics,
  EanAddOnPolicy,
  ExperimentalTurbo,
  Format,
  FormatSelection,
  Image,
  Mode,
  PixelImage,
  Quad,
  RegionEvidence,
  ScanOptions,
  InspectionResult,
  ScanResult,
  StructuredAppend,
  UndecodedRegion,
} from "./index.js";

/**
 * A file or blob (JPEG, PNG and other browser-decodable images), an `<img>`,
 * `<video>` (its current frame), canvas, `ImageBitmap`, `VideoFrame`, `ImageData`
 * or decoded pixels.
 */
export type ImageSource = ImageBitmapSource | PixelImage;

export interface ScannerOptions {
  /** Effort; defaults to "medium". */
  mode?: Mode;
  /** Formats to read; defaults to retail EAN/UPC. */
  formats?: FormatSelection;
  /** EAN/UPC supplement policy; defaults to "ignore". */
  eanAddOnPolicy?: EanAddOnPolicy;
  /** @experimental Faster 1D preset instead of a mode; may change in minor releases. */
  experimentalTurbo?: ExperimentalTurbo;
  /** Serve the packaged WASM files from another directory, such as a CDN. */
  wasmBaseUrl?: string | URL;
}

const scannerOptions = ["mode", "formats", "eanAddOnPolicy", "experimentalTurbo", "wasmBaseUrl"];
interface Pending {
  resolve(value: unknown): void;
  reject(reason: Error): void;
}

function revive({ name, message, code }: { name: string; message: string; code?: string }): Error {
  if (code !== undefined) return new ScannerError(code, message);
  return name === "TypeError" ? new TypeError(message) : new Error(message);
}

/** Snapshot mutable inputs before the first await; Blobs are immutable. */
async function prepare(source: ImageSource): Promise<[WorkerSource, Transferable[]]> {
  const input: unknown = source;
  if (input === null || typeof input !== "object") throw new TypeError("Expected an image source");
  if (source instanceof Blob) return [source, []];
  if ("data" in source) {
    // Validate first, then copy only the addressed rows of a larger backing buffer.
    const { data, width, height, channels, stride, addressed } = pixelLayout(source);
    const copy = new Uint8Array(data.subarray(0, addressed));
    return [{ data: copy, width, height, channels, stride }, [copy.buffer]];
  }
  // A copy, so a caller's ImageBitmap stays usable after it is transferred.
  const bitmap = await createImageBitmap(source);
  return [bitmap, [bitmap]];
}

/**
 * A reusable scanner running in a dedicated worker, so scanning never blocks the
 * page. Construction is synchronous and starts loading in the background; during
 * server rendering it does nothing. Inputs and options are captured when a call
 * starts, and calls reach the worker in call order. Results are deeply frozen and
 * survive disposal. Call dispose() when done to stop the worker.
 */
export class Scanner {
  /** Resolves once the scanner is loaded; rejects with the loading error. Optional. */
  readonly ready: Promise<void>;
  readonly #worker: Worker | undefined;
  readonly #pending = new Map<number, Pending>();
  #nextId = 0;
  #closed: Error | undefined;
  /** Rejecters of calls still waiting; each removes itself when its wait ends. */
  readonly #waiting = new Set<(reason: Error) => void>();
  /** Settles once the previous call has been sent, keeping submissions in call order. */
  #submitted: Promise<void> = Promise.resolve();

  constructor(options: ScannerOptions = {}) {
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scanner options");
    for (const key of Object.keys(options))
      if (!scannerOptions.includes(key))
        throw new TypeError(
          `Unknown scanner option: ${key}. Use the core "tapirscan" entry for loadWasm.`,
        );
    if (typeof Worker === "undefined") {
      this.#closed = new Error(
        "tapirscan/browser scans in browsers; this environment has no Worker",
      );
      this.ready = Promise.reject(this.#closed);
    } else {
      const { wasmBaseUrl, ...rest } = options;
      if (
        wasmBaseUrl !== undefined &&
        typeof wasmBaseUrl !== "string" &&
        !(wasmBaseUrl instanceof URL)
      )
        throw new TypeError("wasmBaseUrl must be a string or URL");
      const base = typeof document === "undefined" ? location.href : document.baseURI;
      const workerOptions =
        wasmBaseUrl === undefined
          ? rest
          : { ...rest, wasmBaseUrl: new URL(wasmBaseUrl, base).href };
      const worker = new Worker(new URL("./browser-worker.js", import.meta.url), {
        type: "module",
      });
      this.#worker = worker;
      worker.onmessage = ({ data }: MessageEvent<WorkerResponse>) => {
        const pending = this.#pending.get(data.id);
        if (!pending) return;
        this.#pending.delete(data.id);
        if ("error" in data) pending.reject(revive(data.error));
        else pending.resolve(data.value);
      };
      worker.onerror = (event) => {
        event.preventDefault();
        // A worker script that cannot load gives no message; name the usual cause.
        this.#close(
          new Error(
            event.message ||
              'Scanner worker failed to start. With the Vite dev server, add optimizeDeps: { exclude: ["tapirscan"] } to vite.config.',
          ),
        );
      };
      worker.onmessageerror = () => {
        this.#close(new Error("Could not read a scanner worker message"));
      };
      this.ready = this.#request({ type: "create", options: workerOptions }, []).then(
        () => undefined,
        (error: unknown) => {
          this.#close(error instanceof Error ? error : new Error(String(error)));
          throw error;
        },
      );
    }
    // Loading errors also reject every scan; awaiting ready is optional.
    this.ready.catch(() => undefined);
  }

  /** Decode barcodes with source-image positions. No detection returns empty values and barcodes. */
  async scan(source: ImageSource, options: ScanOptions = {}): Promise<ScanResult> {
    return freeze((await this.#scan("scan", source, options)) as ScanResult);
  }

  /** Scan with unread regions, timing and engine diagnostics. */
  async inspect(source: ImageSource, options: ScanOptions = {}): Promise<InspectionResult> {
    return freeze((await this.#scan("inspect", source, options)) as InspectionResult);
  }

  /** Stop the worker and reject queued scans. Repeated disposal is safe. */
  dispose(): void {
    // Same error as the core scanner, so one handler covers both entries.
    this.#close(new ScannerError("disposed", "Scanner was disposed"));
  }

  async #scan(type: "scan" | "inspect", source: ImageSource, options: ScanOptions) {
    if (this.#closed) throw this.#closed;
    // Capture the options and the frame before any await, so later changes by the
    // caller (a reused options object or buffer, the next video frame) cannot leak in.
    // Plain copies (scan options hold at most a format selection); invalid values
    // pass through unchanged for the core to reject.
    const input: unknown = options;
    const scanOptions: ScanOptions =
      input !== null && typeof input === "object" && !Array.isArray(input)
        ? {
            ...options,
            ...(typeof options.formats === "object" ? { formats: [...options.formats] } : {}),
          }
        : options;
    const preparing = prepare(source);
    const previous = this.#submitted;
    let submitted: () => void = () => undefined;
    this.#submitted = new Promise<void>((resolve) => {
      submitted = resolve;
    });
    try {
      const [prepared, transfer] = await this.#untilClosed(preparing);
      await this.#untilClosed(previous);
      await this.#untilClosed(this.ready);
      const result = this.#request({ type, source: prepared, options: scanOptions }, transfer);
      submitted();
      return await result;
    } finally {
      // A failed call releases its successor only once its own predecessor was
      // sent, so later calls never overtake earlier ones.
      void previous.then(submitted);
      // Release a bitmap snapshot that was not sent, including one still being prepared.
      void preparing.then(
        ([prepared]) => {
          if (prepared instanceof ImageBitmap) prepared.close();
        },
        () => undefined,
      );
    }
  }

  /** Wait for `promise`, but reject at once on dispose. Leaves nothing behind. */
  #untilClosed<T>(promise: Promise<T>): Promise<T> {
    if (this.#closed) return Promise.reject(this.#closed);
    return new Promise<T>((resolve, reject) => {
      this.#waiting.add(reject);
      promise
        .then(resolve, reject)
        .finally(() => this.#waiting.delete(reject))
        .catch(() => undefined);
    });
  }

  #request(message: DistributiveOmit<WorkerRequest, "id">, transfer: Transferable[]) {
    const worker = this.#worker;
    // Without a worker, the constructor has already recorded why.
    if (this.#closed || !worker)
      return Promise.reject(this.#closed ?? new Error("No scanner worker"));
    const id = this.#nextId++;
    return new Promise<unknown>((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
      try {
        worker.postMessage({ ...message, id }, transfer);
      } catch (error) {
        this.#pending.delete(id);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  #close(reason: Error): void {
    if (this.#closed) return;
    this.#closed = reason;
    for (const reject of this.#waiting) reject(reason);
    this.#waiting.clear();
    this.#worker?.terminate();
    for (const pending of this.#pending.values()) pending.reject(reason);
    this.#pending.clear();
  }
}
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** Scan one image with a temporary scanner. Reuse a Scanner for several images. */
export async function scan(source: ImageSource, options: ScannerOptions = {}): Promise<ScanResult> {
  const scanner = new Scanner(options);
  try {
    return await scanner.scan(source);
  } finally {
    scanner.dispose();
  }
}

/** Inspect one image with a temporary scanner. */
export async function inspect(
  source: ImageSource,
  options: ScannerOptions = {},
): Promise<InspectionResult> {
  const scanner = new Scanner(options);
  try {
    return await scanner.inspect(source);
  } finally {
    scanner.dispose();
  }
}

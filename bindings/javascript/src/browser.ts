// tapirscan/browser: the core API, run in a bundled worker, for browser image sources.
import type { WorkerRequest, WorkerResponse, WorkerSource } from "./browser-worker.js";
import { freeze } from "./freeze.js";
import type {
  Barcode,
  EanAddOnPolicy,
  FormatSelection,
  Mode,
  PixelImage,
  ScanOptions,
  ScanResult,
} from "./index.js";
import { ScannerError } from "./rust-session.js";

export { best } from "./index.js";
export { ScannerError } from "./rust-session.js";
export type {
  Barcode,
  EanAddOnPolicy,
  Format,
  FormatSelection,
  Image,
  Mode,
  PixelImage,
  Quad,
  ScanOptions,
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
  /** EAN/UPC supplement policy; defaults to "Ignore". */
  eanAddOnPolicy?: EanAddOnPolicy;
  /** Serve the packaged WASM files from another directory, such as a CDN. */
  wasmBaseUrl?: string | URL;
}

const scannerOptions = ["mode", "formats", "eanAddOnPolicy", "wasmBaseUrl"];
interface Pending {
  resolve(value: unknown): void;
  reject(reason: Error): void;
}

function revive({ name, message, code }: { name: string; message: string; code?: string }): Error {
  if (code !== undefined) return new ScannerError(code, message);
  return name === "TypeError" ? new TypeError(message) : new Error(message);
}

/** Send Blobs and pixels as they are; snapshot elements and bitmaps now. */
async function prepare(source: ImageSource): Promise<[WorkerSource, Transferable[]]> {
  const input: unknown = source;
  if (input === null || typeof input !== "object") throw new TypeError("Expected an image source");
  if (source instanceof Blob || "data" in source) return [source, []];
  // A copy, so a caller's ImageBitmap stays usable after it is transferred.
  const bitmap = await createImageBitmap(source);
  return [bitmap, [bitmap]];
}

/**
 * A reusable scanner running in a dedicated worker, so scanning never blocks the
 * page. Construction is synchronous and starts loading in the background; during
 * server rendering it does nothing. Calls are queued, and results are deeply
 * frozen and survive disposal. Call dispose() when done to stop the worker.
 */
export class Scanner {
  /** Resolves once the scanner is loaded; rejects with the loading error. Optional. */
  readonly ready: Promise<void>;
  readonly #worker: Worker | undefined;
  readonly #pending = new Map<number, Pending>();
  #nextId = 0;
  #closed: Error | undefined;

  constructor(options: ScannerOptions = {}) {
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scanner options");
    for (const key of Object.keys(options))
      if (!scannerOptions.includes(key))
        throw new TypeError(
          `Unknown scanner option: ${key}. Use the core "tapirscan" entry for loadWasm and experimentalTurbo.`,
        );
    if (typeof Worker === "undefined") {
      this.#closed = new Error(
        "tapirscan/browser scans in browsers; this environment has no Worker",
      );
      this.ready = Promise.reject(this.#closed);
    } else {
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
      const { wasmBaseUrl, ...rest } = options;
      const base = typeof document === "undefined" ? location.href : document.baseURI;
      this.ready = this.#request(
        {
          type: "create",
          options:
            wasmBaseUrl === undefined
              ? rest
              : { ...rest, wasmBaseUrl: new URL(wasmBaseUrl, base).href },
        },
        [],
      ).then(() => undefined);
    }
    // Loading errors also reject every scan; awaiting ready is optional.
    this.ready.catch(() => undefined);
  }

  /** Decode barcodes with source-image positions. No detection resolves to []. */
  async scan(source: ImageSource, options: ScanOptions = {}): Promise<readonly Barcode[]> {
    return freeze((await this.#scan("scan", source, options)) as Barcode[]);
  }

  /** Scan with unread regions, work status, timing and engine diagnostics. */
  async inspect(source: ImageSource, options: ScanOptions = {}): Promise<ScanResult> {
    return freeze((await this.#scan("inspect", source, options)) as ScanResult);
  }

  /** Stop the worker and reject queued scans. Repeated disposal is safe. */
  dispose(): void {
    this.#close(new Error("Scanner was disposed"));
  }

  async #scan(type: "scan" | "inspect", source: ImageSource, options: ScanOptions) {
    if (this.#closed) throw this.#closed;
    // Capture the frame first, so video scans read the frame current at the call.
    const [prepared, transfer] = await prepare(source);
    await this.ready;
    return this.#request({ type, source: prepared, options }, transfer);
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
    this.#worker?.terminate();
    for (const pending of this.#pending.values()) pending.reject(reason);
    this.#pending.clear();
  }
}
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/** Scan one image with a temporary scanner. Reuse a Scanner for several images. */
export async function scan(
  source: ImageSource,
  options: ScannerOptions & ScanOptions = {},
): Promise<readonly Barcode[]> {
  const { extendedBudget, ...creation } = options;
  const scanner = new Scanner(creation);
  try {
    return await scanner.scan(source, { extendedBudget });
  } finally {
    scanner.dispose();
  }
}

/** Inspect one image with a temporary scanner. */
export async function inspect(
  source: ImageSource,
  options: ScannerOptions & ScanOptions = {},
): Promise<ScanResult> {
  const { extendedBudget, ...creation } = options;
  const scanner = new Scanner(creation);
  try {
    return await scanner.inspect(source, { extendedBudget });
  } finally {
    scanner.dispose();
  }
}

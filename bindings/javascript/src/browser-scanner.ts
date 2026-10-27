// Shared by the tapirscan/browser entries: the core API, run in a bundled worker,
// for browser image sources. Each entry supplies the worker that bundles its WASM.
import type {
  WorkerRequest,
  WorkerResponse,
  WorkerScannerOptions,
  WorkerSource,
} from "./browser-worker-core.js";
import { freeze } from "./freeze.js";
import { takeOver } from "./handover.js";
import { pixelLayout } from "./layout.js";
import type {
  EanAddOnPolicy,
  ExperimentalTurbo,
  FormatSelection,
  Mode,
  PixelImage,
  ScanOptions as CoreScanOptions,
  InspectionResult,
  ScanResult,
} from "./index.js";
import { ScannerError } from "./rust-session.js";
import { browserSupported } from "./support.js";

/**
 * A file or blob (JPEG, PNG and other browser-decodable images), an `<img>`,
 * `<video>` (its current frame), canvas, `ImageBitmap`, `VideoFrame`, `ImageData`
 * or decoded pixels.
 */
export type ImageSource = ImageBitmapSource | PixelImage;

export interface ScanOptions extends CoreScanOptions {
  /**
   * Hand decoded pixels' `ArrayBuffer` to the worker instead of copying it. The
   * caller's buffer is detached (length 0) when the call returns its promise, unless
   * the scanner was already disposed. Applies only when the pixels span their whole,
   * non-shared buffer; other inputs are copied as usual.
   */
  transfer?: boolean;
}

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
  /**
   * Start the scanner's worker yourself, for bundlers that do not emit workers from
   * packages (Angular). It must run an entry's worker module, such as
   * `tapirscan/browser/medium/worker`; pair it with `wasmBaseUrl`.
   */
  worker?: () => Worker;
}

const singleModeOptions = ["formats", "eanAddOnPolicy", "wasmBaseUrl", "worker"];
const scannerOptions = ["mode", "experimentalTurbo", ...singleModeOptions];
interface Pending {
  resolve(value: unknown): void;
  reject(reason: Error): void;
}

function revive({ name, message, code }: { name: string; message: string; code?: string }): Error {
  if (code !== undefined) return new ScannerError(code, message);
  const error = name === "TypeError" ? new TypeError(message) : new Error(message);
  // Keeps names such as EncodingError, for images the browser cannot decode.
  error.name = name;
  return error;
}

/** Snapshot mutable inputs before the first await; Blobs are immutable. */
async function prepare(
  source: ImageSource,
  transfer: boolean,
): Promise<[WorkerSource, Transferable[]]> {
  const input: unknown = source;
  if (input === null || typeof input !== "object") throw new TypeError("Expected an image source");
  if (source instanceof Blob) return [source, []];
  if ("data" in source) {
    // Validate first, then copy only the addressed rows of a larger backing buffer.
    const { data, width, height, channels, stride, addressed } = pixelLayout(source);
    const { buffer } = data;
    if (
      transfer &&
      buffer instanceof ArrayBuffer &&
      data.byteOffset === 0 &&
      data.byteLength === buffer.byteLength
    ) {
      // The caller opted in: move the buffer instead of copying it. Detach it now,
      // not at postMessage, so the call captures the pixels when it starts.
      const moved = structuredClone(buffer, { transfer: [buffer] });
      return [{ data: new Uint8Array(moved), width, height, channels, stride }, [moved]];
    }
    // A same-type view makes this a block copy; copying from ImageData's
    // Uint8ClampedArray into a Uint8Array would convert element by element.
    const copy = new Uint8Array(data.buffer, data.byteOffset, addressed).slice();
    return [{ data: copy, width, height, channels, stride }, [copy.buffer]];
  }
  if (source instanceof ImageBitmap && takeOver(source)) return [source, [source]];
  const frame = videoFrame(source);
  if (frame) return [frame, [frame]];
  // A copy, so a caller's ImageBitmap stays usable after it is transferred.
  const bitmap = await createImageBitmap(source);
  return [bitmap, [bitmap]];
}

/**
 * The current frame of a video, or a clone of a caller's VideoFrame, where the
 * browser has VideoFrame: it reaches the worker without the copy that
 * createImageBitmap makes, and draws there to the same pixels.
 */
function videoFrame(source: ImageBitmapSource): VideoFrame | undefined {
  if (typeof VideoFrame === "undefined") return undefined;
  if (source instanceof VideoFrame) return source.clone();
  if (typeof HTMLVideoElement === "undefined" || !(source instanceof HTMLVideoElement))
    return undefined;
  try {
    return new VideoFrame(source);
  } catch {
    // Before the first frame, for example; createImageBitmap reports it.
    return undefined;
  }
}

/** Options of a single-mode entry such as tapirscan/browser/medium. */
export type SingleModeOptions = Omit<ScannerOptions, "mode" | "experimentalTurbo">;

/** Every entry's `Scanner`, documented on tapirscan/browser's `Scanner`. */
export class BrowserScanner {
  /**
   * Resolves once the scanner is loaded; rejects with the loading error. Optional.
   * After a worker failure it is the replacement worker's load.
   */
  get ready(): Promise<void> {
    return this.#ready;
  }
  #ready: Promise<void>;
  /** Undefined until the first worker starts and after a loaded worker failed. */
  #worker: Worker | undefined;
  /** Starts a worker with the creation options; undefined where none can run. */
  readonly #spawn: (() => Worker) | undefined;
  readonly #pending = new Map<number, Pending>();
  #nextId = 0;
  #closed: Error | undefined;
  /** Whether the current worker has loaded the scanner. */
  #loaded = false;
  /** Why the last loaded worker failed; calls fail with it until a replacement is sent. */
  #failure: Error | undefined;
  #options: WorkerScannerOptions = {};
  /** Rejecters of calls still waiting; each removes itself when its wait ends. */
  readonly #waiting = new Set<(reason: Error) => void>();
  /** Settles once the previous call has been sent, keeping submissions in call order. */
  #submitted: Promise<void> = Promise.resolve();

  /**
   * `spawn` starts the entry's worker; it must contain the literal
   * `new Worker(new URL(...), ...)` so bundlers emit the worker. A `fixedMode`
   * entry bundles only that mode's WASM.
   */
  constructor(options: ScannerOptions, spawn: () => Worker, fixedMode?: Mode) {
    const input: unknown = options;
    if (input === null || typeof input !== "object" || Array.isArray(input))
      throw new TypeError("Invalid scanner options");
    for (const key of Object.keys(options)) {
      if (fixedMode !== undefined && (key === "mode" || key === "experimentalTurbo"))
        throw new TypeError(
          `tapirscan/browser/${fixedMode} has no ${key} option; use "tapirscan/browser"`,
        );
      if (!scannerOptions.includes(key))
        throw new TypeError(
          key === "loadWasm"
            ? `Unknown scanner option: ${key}. Use the core "tapirscan" entry for loadWasm.`
            : `Unknown scanner option: ${key}`,
        );
    }
    if (fixedMode !== undefined) options = { ...options, mode: fixedMode };
    const { wasmBaseUrl, worker: spawnCustom, ...rest } = options;
    if (spawnCustom !== undefined && typeof spawnCustom !== "function")
      throw new TypeError("worker must be a function that returns a Worker");
    if (
      wasmBaseUrl !== undefined &&
      typeof wasmBaseUrl !== "string" &&
      !(wasmBaseUrl instanceof URL)
    )
      throw new TypeError("wasmBaseUrl must be a string or URL");
    // Resolve now, so an invalid URL throws here. A server has no base to resolve
    // against, and Deno without --location throws on reading location.
    let resolved: string | undefined;
    if (wasmBaseUrl !== undefined) {
      let base: string | undefined;
      if (typeof document !== "undefined") base = document.baseURI;
      else
        try {
          base = location.href;
        } catch {
          base = undefined;
        }
      if (base !== undefined) resolved = new URL(wasmBaseUrl, base).href;
    }
    if (typeof window === "undefined" && typeof Worker === "undefined") {
      this.#closed = new Error(
        "tapirscan/browser scans in browsers; this environment has no Worker",
      );
      this.#ready = Promise.reject(this.#closed);
    } else if (!browserSupported()) {
      // No worker is started, so nothing is downloaded.
      this.#closed = new ScannerError(
        "unsupported",
        "Tapirscan needs WebAssembly SIMD, module workers and OffscreenCanvas, which this browser lacks",
      );
      this.#ready = Promise.reject(this.#closed);
    } else {
      const workerOptions = resolved === undefined ? rest : { ...rest, wasmBaseUrl: resolved };
      const start = spawnCustom ?? spawn;
      this.#spawn = () => {
        const worker = start();
        this.#watch(worker);
        return worker;
      };
      this.#ready = this.#start(workerOptions);
    }
    // Loading errors also reject every scan; awaiting ready is optional.
    this.#ready.catch(() => undefined);
  }

  /** Start a worker and load the scanner in it; resolves once it is loaded. */
  #start(options: WorkerScannerOptions): Promise<void> {
    const worker = this.#spawn?.();
    this.#worker = worker;
    this.#options = options;
    const loading = this.#request({ type: "create", options }, []).then(
      () => {
        if (this.#worker === worker) this.#loaded = true;
      },
      (error: unknown) => {
        // A worker that never loaded stays failed: retrying would repeat the download.
        this.#close(error instanceof Error ? error : new Error(String(error)));
        throw error;
      },
    );
    loading.catch(() => undefined);
    return loading;
  }

  #watch(worker: Worker): void {
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
      this.#fail(
        worker,
        new Error(
          event.message ||
            'Scanner worker failed to start: its script was not served. Vite 6/7 dev server: add optimizeDeps: { exclude: ["tapirscan"] }. Other setups: see docs/BROWSER_DEPLOYMENT.md.',
        ),
      );
    };
    worker.onmessageerror = () => {
      this.#fail(worker, new Error("Could not read a scanner worker message"));
    };
  }

  /**
   * A worker failed. Before it loaded, the scanner fails for good; afterwards,
   * its calls reject and the next scan starts a replacement worker.
   */
  #fail(worker: Worker, reason: Error): void {
    if (worker !== this.#worker) return;
    if (!this.#loaded) {
      this.#close(reason);
      return;
    }
    worker.terminate();
    this.#worker = undefined;
    this.#loaded = false;
    this.#failure = reason;
    for (const pending of this.#pending.values()) pending.reject(reason);
    this.#pending.clear();
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
    const plain = input !== null && typeof input === "object" && !Array.isArray(input);
    // `transfer` is handled here; the worker's core receives only scan options.
    const { transfer = false, ...rest } = plain ? options : {};
    const scanOptions: CoreScanOptions = plain
      ? {
          ...rest,
          ...(typeof rest.formats === "object" ? { formats: [...rest.formats] } : {}),
        }
      : options;
    if (typeof transfer !== "boolean") throw new TypeError("transfer must be a boolean");
    const preparing = prepare(source, transfer);
    const previous = this.#submitted;
    let submitted: () => void = () => undefined;
    this.#submitted = new Promise<void>((resolve) => {
      submitted = resolve;
    });
    try {
      const [prepared, transferList] = await this.#untilClosed(preparing);
      await this.#untilClosed(previous);
      // Replace a worker that failed after loading.
      if (!this.#worker && !this.#closed) this.#ready = this.#start(this.#options);
      await this.#untilClosed(this.#ready);
      const result = this.#request({ type, source: prepared, options: scanOptions }, transferList);
      submitted();
      return await result;
    } finally {
      // A failed call releases its successor only once its own predecessor was
      // sent, so later calls never overtake earlier ones.
      void previous.then(submitted);
      // Release a snapshot that was not sent, including one still being prepared.
      void preparing.then(
        ([prepared]) => {
          if ("close" in prepared) prepared.close();
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
    // Without a worker, the constructor or a worker failure has recorded why.
    if (this.#closed || !worker)
      return Promise.reject(this.#closed ?? this.#failure ?? new Error("No scanner worker"));
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

/** Options of the one-shot helpers: the scanner options plus the per-call `transfer`. */
export type OneShotOptions = ScannerOptions & Pick<ScanOptions, "transfer">;
/** `OneShotOptions` of a single-mode entry. */
export type SingleModeOneShotOptions = SingleModeOptions & Pick<ScanOptions, "transfer">;

/** An entry's one-shot `scan` and `inspect`: a temporary scanner per call. */
export function oneShot<O extends SingleModeOptions>(Scanner: new (options: O) => BrowserScanner) {
  async function run<R>(
    { transfer, ...options }: O & Pick<ScanOptions, "transfer">,
    call: (scanner: BrowserScanner, options: ScanOptions) => Promise<R>,
  ): Promise<R> {
    const scanner = new Scanner(options as unknown as O);
    try {
      return await call(scanner, transfer === undefined ? {} : { transfer });
    } finally {
      scanner.dispose();
    }
  }
  type Options = O & Pick<ScanOptions, "transfer">;
  return {
    scan: (source: ImageSource, options = {} as Options): Promise<ScanResult> =>
      run(options, (scanner, call) => scanner.scan(source, call)),
    inspect: (source: ImageSource, options = {} as Options): Promise<InspectionResult> =>
      run(options, (scanner, call) => scanner.inspect(source, call)),
  };
}

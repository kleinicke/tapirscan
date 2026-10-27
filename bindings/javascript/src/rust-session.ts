// Stateless for complete inputs, so one decoder serves every scan.
const outputDecoder = new TextDecoder();

export class ScannerError extends Error {
  constructor(
    readonly code: string,
    message: string,
    options?: ErrorOptions,
  ) {
    super(message, options);
    this.name = "ScannerError";
  }
}

/** Pixels the engine scans directly; larger images are reduced (Rust `WORKING_PIXELS`). */
export const workingPixels = 32 * 1024 * 1024;

/** Write `count` packed rows starting at row `y` into `target`. */
export type RowReader = (y: number, count: number, target: Uint8Array) => void;
/** Compiled modules per loaded byte buffer: scanners from cached bytes skip compiling. */
const compiled = new WeakMap<ArrayBuffer, Promise<WebAssembly.Module>>();
/** Row chunk size for images above the working size. */
const chunkBytes = 4 * 1024 * 1024;

interface RustExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  tapirscan_abi_version: () => number;
  tapirscan_mode: () => number;
  tapirscan_experimental_turbo: () => number;
  tapirscan_create: (mode: number, formats: number, addOnPolicy: number) => number;
  tapirscan_destroy: (handle: number) => number;
  tapirscan_prepare: (
    handle: number,
    width: number,
    height: number,
    channels: number,
    stride: number,
  ) => number;
  tapirscan_prepare_rows: (
    handle: number,
    width: number,
    height: number,
    channels: number,
    chunkRows: number,
  ) => number;
  tapirscan_push_rows: (handle: number, count: number) => number;
  tapirscan_input_ptr: (handle: number) => number;
  tapirscan_input_len: (handle: number) => number;
  tapirscan_scan: (handle: number, flags: number, formats: number) => number;
  tapirscan_output_ptr: (handle: number) => number;
  tapirscan_output_len: (handle: number) => number;
}

const exportNames = [
  "tapirscan_abi_version",
  "tapirscan_mode",
  "tapirscan_create",
  "tapirscan_destroy",
  "tapirscan_prepare",
  "tapirscan_prepare_rows",
  "tapirscan_push_rows",
  "tapirscan_input_ptr",
  "tapirscan_input_len",
  "tapirscan_scan",
  "tapirscan_output_ptr",
  "tapirscan_output_len",
] as const;

function status(code: number, operation: string): void {
  if (code === 0) return;
  const names: Record<number, string> = {
    1: "invalid_input",
    2: "disposed",
    4: "engine",
    5: "capacity",
  };
  throw new ScannerError(
    names[code] ?? `core_${String(code)}`,
    `${operation} failed (${String(code)})`,
  );
}

/** What a session is opened with, kept to reopen it after an engine trap. */
interface Settings {
  mode: number;
  formats: number;
  addOnPolicy: number;
  experimentalTurbo: number;
}

/** Check the instance's ABI and open a scanner session in it. */
function open(instance: WebAssembly.Instance, settings: Settings): [RustExports, number] {
  const { mode, formats, addOnPolicy, experimentalTurbo } = settings;
  const exports = instance.exports as RustExports;
  if (!(exports.memory instanceof WebAssembly.Memory))
    throw new ScannerError("abi_shape", "Missing WASM memory");
  for (const name of exportNames)
    if (typeof exports[name] !== "function")
      throw new ScannerError("abi_shape", `Missing WASM export: ${name}`);
  if (exports.tapirscan_abi_version() !== 3)
    throw new ScannerError("abi_version", "Unsupported scanner ABI");
  if (exports.tapirscan_mode() !== mode)
    throw new ScannerError("abi_mode", "Scanner WASM mode does not match the requested mode");
  const tier = exports.tapirscan_experimental_turbo;
  const actualTurbo = typeof tier === "function" ? tier() : 0;
  if (actualTurbo !== experimentalTurbo)
    throw new ScannerError(
      "abi_turbo",
      "Scanner WASM does not match the requested experimentalTurbo preset",
    );
  const handle = exports.tapirscan_create(mode, formats, addOnPolicy);
  if (!handle) throw new ScannerError("capacity", "Could not create scanner session");
  return [exports, handle];
}

export class RustScannerSession {
  /** Set when a trap stopped the engine; the next scan opens a fresh instance. */
  private stopped = false;

  private constructor(
    private readonly module: WebAssembly.Module,
    private readonly settings: Settings,
    private exports: RustExports | undefined,
    private handle: number,
  ) {}

  static async create(
    bytes: ArrayBuffer,
    mode: number,
    formats: number,
    addOnPolicy: number,
    experimentalTurbo = 0,
  ): Promise<RustScannerSession> {
    let pending = compiled.get(bytes);
    if (!pending) {
      pending = WebAssembly.compile(bytes);
      compiled.set(bytes, pending);
      // A failed compile is retried with the next scanner.
      pending.catch(() => compiled.delete(bytes));
    }
    const module = await pending;
    const settings = { mode, formats, addOnPolicy, experimentalTurbo };
    const [exports, handle] = open(await WebAssembly.instantiate(module, {}), settings);
    return new RustScannerSession(module, settings, exports, handle);
  }

  scan(
    image: { data: Uint8Array; width: number; height: number; channels: number; stride: number },
    flags: number,
    formats: number,
  ): unknown {
    return this.guarded(() => this.scanImage(image, flags, formats));
  }

  /**
   * Scan an image above the working size whose packed rows `read` writes on
   * demand, top to bottom, so no full-size copy has to exist anywhere.
   */
  scanRows(
    image: { width: number; height: number; channels: number },
    read: RowReader,
    flags: number,
    formats: number,
  ): unknown {
    return this.guarded(() => this.scanStream(image, read, flags, formats));
  }

  private scanImage(
    image: { data: Uint8Array; width: number; height: number; channels: number; stride: number },
    flags: number,
    formats: number,
  ): unknown {
    const exports = this.activeExports();
    if (image.width * image.height > workingPixels) {
      const row = image.width * image.channels;
      this.pushRows(exports, image, (y, count, target) => {
        if (image.stride === row) target.set(image.data.subarray(y * row, (y + count) * row));
        else
          for (let i = 0; i < count; i++) {
            const start = (y + i) * image.stride;
            target.set(image.data.subarray(start, start + row), i * row);
          }
      });
    } else {
      status(
        exports.tapirscan_prepare(
          this.handle,
          image.width,
          image.height,
          image.channels,
          image.stride,
        ),
        "Image preparation",
      );
      const length = exports.tapirscan_input_len(this.handle);
      const expected = (image.height - 1) * image.stride + image.width * image.channels;
      if (length !== expected) throw new ScannerError("abi_shape", "Input allocation mismatch");
      new Uint8Array(exports.memory.buffer, exports.tapirscan_input_ptr(this.handle), length).set(
        image.data.subarray(0, length),
      );
    }
    return this.finish(exports, flags, formats);
  }

  private scanStream(
    image: { width: number; height: number; channels: number },
    read: RowReader,
    flags: number,
    formats: number,
  ): unknown {
    const exports = this.activeExports();
    this.pushRows(exports, image, read);
    return this.finish(exports, flags, formats);
  }

  private finish(exports: RustExports, flags: number, formats: number): unknown {
    const result = exports.tapirscan_scan(this.handle, flags, formats);
    const output = new Uint8Array(
      exports.memory.buffer,
      exports.tapirscan_output_ptr(this.handle),
      exports.tapirscan_output_len(this.handle),
    );
    let decoded: unknown;
    try {
      decoded = JSON.parse(outputDecoder.decode(output));
    } catch {
      throw new ScannerError("invalid_output", "Scanner returned invalid JSON");
    }
    if (result !== 0) {
      const message =
        decoded && typeof decoded === "object" && "error" in decoded
          ? String(decoded.error)
          : `Scan failed (${String(result)})`;
      throw new ScannerError(result === 4 ? "engine" : `core_${String(result)}`, message);
    }
    return decoded;
  }

  /**
   * Stream an image above the working size into WASM in chunks of rows, so WASM
   * memory holds a chunk and the reduced image instead of the whole image.
   */
  private pushRows(
    exports: RustExports,
    image: { width: number; height: number; channels: number },
    read: RowReader,
  ): void {
    const row = image.width * image.channels;
    const chunkRows = Math.max(1, Math.floor(chunkBytes / row));
    status(
      exports.tapirscan_prepare_rows(
        this.handle,
        image.width,
        image.height,
        image.channels,
        chunkRows,
      ),
      "Image preparation",
    );
    const capacity = exports.tapirscan_input_len(this.handle);
    if (capacity !== row * Math.min(chunkRows, image.height))
      throw new ScannerError("abi_shape", "Input allocation mismatch");
    for (let y = 0; y < image.height; y += chunkRows) {
      const count = Math.min(chunkRows, image.height - y);
      // Pushing rows may grow WASM memory, so take a fresh view for every chunk.
      const target = new Uint8Array(
        exports.memory.buffer,
        exports.tapirscan_input_ptr(this.handle),
        count * row,
      );
      read(y, count, target);
      status(exports.tapirscan_push_rows(this.handle, count), "Image rows");
    }
  }

  dispose(): void {
    this.stopped = false;
    if (!this.handle || !this.exports) return;
    const exports = this.exports;
    const handle = this.handle;
    // WASM memory never shrinks: drop the instance so a retained scanner frees it.
    this.exports = undefined;
    this.handle = 0;
    status(exports.tapirscan_destroy(handle), "Scanner disposal");
  }

  /**
   * Run `scan` against the live instance. A trap (out of memory, stack overflow)
   * can leave the instance's memory inconsistent, so it is dropped and the next
   * call opens a fresh instance of the same compiled module.
   */
  private guarded<T>(scan: () => T): T {
    try {
      // Reopening can fail as the trap did, for example out of memory again.
      if (this.stopped) {
        [this.exports, this.handle] = open(
          new WebAssembly.Instance(this.module, {}),
          this.settings,
        );
        this.stopped = false;
      }
      return scan();
    } catch (error) {
      if (!(error instanceof WebAssembly.RuntimeError || error instanceof RangeError)) throw error;
      this.exports = undefined;
      this.handle = 0;
      this.stopped = true;
      throw new ScannerError(
        "engine",
        `The scanner engine stopped (${error.message}); the next scan starts a fresh one`,
        { cause: error },
      );
    }
  }

  private activeExports(): RustExports {
    if (!this.handle || !this.exports) throw new ScannerError("disposed", "Scanner is disposed");
    return this.exports;
  }
}

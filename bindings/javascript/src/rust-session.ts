export class ScannerError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ScannerError";
  }
}

interface RustExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  tapirscan_abi_version: () => number;
  tapirscan_mode: () => number;
  tapirscan_create: (mode: number, formats: number, addOnPolicy: number) => number;
  tapirscan_destroy: (handle: number) => number;
  tapirscan_prepare: (
    handle: number,
    width: number,
    height: number,
    channels: number,
    stride: number,
  ) => number;
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

export class RustScannerSession {
  private constructor(
    private readonly exports: RustExports,
    private handle: number,
  ) {}

  static async create(
    bytes: ArrayBuffer,
    mode: number,
    formats: number,
    addOnPolicy: number,
  ): Promise<RustScannerSession> {
    const instance = await WebAssembly.instantiate(bytes, {});
    const exports = instance.instance.exports as RustExports;
    if (!(exports.memory instanceof WebAssembly.Memory))
      throw new ScannerError("abi_shape", "Missing WASM memory");
    for (const name of exportNames)
      if (typeof exports[name] !== "function")
        throw new ScannerError("abi_shape", `Missing WASM export: ${name}`);
    if (exports.tapirscan_abi_version() !== 1)
      throw new ScannerError("abi_version", "Unsupported scanner ABI");
    if (exports.tapirscan_mode() !== mode)
      throw new ScannerError("abi_mode", "Scanner WASM mode does not match the requested mode");
    const handle = exports.tapirscan_create(mode, formats, addOnPolicy);
    if (!handle) throw new ScannerError("capacity", "Could not create scanner session");
    return new RustScannerSession(exports, handle);
  }

  scan(
    image: { data: Uint8Array; width: number; height: number; channels: number; stride: number },
    flags: number,
    formats: number,
  ): unknown {
    const exports = this.activeExports();
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
    const result = exports.tapirscan_scan(this.handle, flags, formats);
    const output = new Uint8Array(
      exports.memory.buffer,
      exports.tapirscan_output_ptr(this.handle),
      exports.tapirscan_output_len(this.handle),
    );
    let decoded: unknown;
    try {
      decoded = JSON.parse(new TextDecoder().decode(output));
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

  dispose(): void {
    if (!this.handle) return;
    status(this.exports.tapirscan_destroy(this.handle), "Scanner disposal");
    this.handle = 0;
  }

  private activeExports(): RustExports {
    if (!this.handle) throw new ScannerError("disposed", "Scanner is disposed");
    return this.exports;
  }
}

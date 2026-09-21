import type { Barcode, EanAddOnSymbol } from "./scanner.js";
import { maskFor, type Format } from "./formats.js";
import { eanAddOnReadFlag, eanAddOnRequireFlag } from "./format-registry.js";

interface ExtraExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  multi_capabilities: () => number;
  multi_prepare_rgba: (handle: number, width: number, height: number) => number;
  multi_input_rgba: (handle: number) => number;
  multi_scan_rgba: (handle: number, mask: number, effort: number) => number;
  multi_new: () => number;
  multi_free: (handle: number) => void;
  multi_prepare: (handle: number, width: number, height: number) => number;
  multi_input: (handle: number) => number;
  multi_scan: (handle: number, mask: number, effort: number) => number;
  multi_capture_source: (handle: number) => number;
  multi_crop_transform: (handle: number) => number;
  multi_crop: (handle: number, width: number, height: number) => number;
  multi_output: (handle: number) => number;
  multi_output_len: (handle: number) => number;
}

export interface ExtraScanResult {
  barcodes: Barcode[];
  regions?: Barcode[];
  unfinished: boolean;
}

export class ExtraReaderSession {
  private constructor(
    private readonly exports: ExtraExports,
    private handle: number,
    readonly supportsRgba: boolean,
    readonly supportsCrop: boolean,
  ) {}

  static async create(bytes: ArrayBuffer): Promise<ExtraReaderSession> {
    const instance = await WebAssembly.instantiate(bytes, {});
    const exports = instance.instance.exports as ExtraExports;
    for (const name of [
      "multi_new",
      "multi_free",
      "multi_prepare",
      "multi_input",
      "multi_scan",
      "multi_output",
      "multi_output_len",
    ] as const) {
      if (typeof exports[name] !== "function")
        throw Error(`Invalid multiformat WASM export: ${name}`);
    }
    if (!(exports.memory instanceof WebAssembly.Memory))
      throw Error("Invalid multiformat WASM memory.");
    const capabilities =
      typeof exports.multi_capabilities === "function" ? exports.multi_capabilities() : 0;
    const supportsRgba =
      (capabilities & 4) !== 0 &&
      [exports.multi_prepare_rgba, exports.multi_input_rgba, exports.multi_scan_rgba].every(
        (fn) => typeof fn === "function",
      );
    const supportsCrop =
      (capabilities & 16) !== 0 &&
      [exports.multi_capture_source, exports.multi_crop_transform, exports.multi_crop].every(
        (fn) => typeof fn === "function",
      );
    const handle = exports.multi_new();
    if (!handle) throw Error("Could not create additional reader session.");
    return new ExtraReaderSession(exports, handle, supportsRgba, supportsCrop);
  }

  scan(
    pixels: Uint8Array,
    width: number,
    height: number,
    formats: Format[],
    effort: number,
    eanAddOnSymbol: EanAddOnSymbol,
    rgba = false,
  ): ExtraScanResult {
    const exports = this.activeExports();
    if (
      (rgba ? exports.multi_prepare_rgba : exports.multi_prepare)(this.handle, width, height) !== 0
    )
      throw Error("Additional reader rejected image size.");
    new Uint8Array(
      exports.memory.buffer,
      (rgba ? exports.multi_input_rgba : exports.multi_input)(this.handle),
      width * height * (rgba ? 4 : 1),
    ).set(pixels);
    return this.scanPrepared(formats, effort, eanAddOnSymbol, rgba);
  }

  scanPrepared(
    formats: Format[],
    effort: number,
    eanAddOnSymbol: EanAddOnSymbol,
    rgba = false,
  ): ExtraScanResult {
    const exports = this.activeExports();
    const mask =
      maskFor(formats) |
      (eanAddOnSymbol === "Read"
        ? eanAddOnReadFlag
        : eanAddOnSymbol === "Require"
          ? eanAddOnRequireFlag
          : 0);
    if ((rgba ? exports.multi_scan_rgba : exports.multi_scan)(this.handle, mask, effort) !== 0)
      throw Error("Additional scanner failed.");
    const bytes = new Uint8Array(
      exports.memory.buffer,
      exports.multi_output(this.handle),
      exports.multi_output_len(this.handle),
    );
    const found = JSON.parse(new TextDecoder().decode(bytes)) as ExtraScanResult;
    for (const barcode of [...found.barcodes, ...(found.regions ?? [])])
      if (!formats.includes(barcode.format as Format))
        throw Error("Scanner returned a disabled format.");
    return found;
  }

  captureSource(gray: Uint8Array, width: number, height: number): void {
    const exports = this.activeExports();
    exports.multi_prepare(this.handle, width, height);
    new Uint8Array(exports.memory.buffer, exports.multi_input(this.handle), width * height).set(
      gray,
    );
    if (exports.multi_capture_source(this.handle) !== 0)
      throw Error("Could not retain source image.");
  }

  sampleCrop(transform: ArrayLike<number>, width: number, height: number): void {
    const exports = this.activeExports();
    new Float64Array(exports.memory.buffer, exports.multi_crop_transform(this.handle), 8).set(
      transform,
    );
    if (exports.multi_crop(this.handle, width, height) !== 0)
      throw Error("Could not sample barcode crop.");
  }

  dispose(): void {
    if (!this.handle) return;
    this.exports.multi_free(this.handle);
    this.handle = 0;
  }

  private activeExports(): ExtraExports {
    if (!this.handle) throw Error("Additional readers have not been loaded.");
    return this.exports;
  }
}

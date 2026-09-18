import { mergeLinearDuplicates } from "./linear-duplicates.js";
import { ReleaseDetailScanner, fitLimits } from "../detail.js";
import { IndependentScanner, type Image, type Quad } from "../completion-host.mjs";
import { IndependentScanner as WideScanner } from "../host64.js";
import type { Mode } from "../index.js";
import { policy } from "../policy.js";
import { rectify, rectificationPlan, project, distinctReads, polygonOverlap } from "./geometry.js";
import { toGray } from "./pixels.js";
import {
  maskFor,
  resolveFormats,
  linearFormats as supportedLinearFormats,
  type Format,
} from "./formats.js";

export interface Barcode {
  bytes?: number[];
  format: Format | "Unknown";
  text: string;
  polygon: Quad;
  support: number;
  localizationScore?: number;
  gs1?: boolean;
  readerInitialization?: boolean;
  structuredAppend?: { index: number; count: number; id?: string; parity?: number };
  eanAddOn?: string;
  error?: number;
  rank?: number;
}
export type EanAddOnSymbol = "Ignore" | "Read" | "Require";
export interface Frame {
  primary?:
    | ReturnType<ReleaseDetailScanner["scanLocalized"]>
    | ReturnType<IndependentScanner["scanLocalized"]>;
  eanAddOnSymbol: EanAddOnSymbol;
  barcodes: Barcode[];
  regions: Barcode[];
  formats: Format[];
  unfinished: boolean;
  scanMs: number;
  mediumMs: number;
  additionalMs: number;
  preparationMs: number;
  localizationMs: number;
  linearStrategy: "scanlines" | "medium-localized";
}
interface Localization {
  proposals: { polygon: Quad; score?: number }[];
  workLimited?: boolean;
  omitted?: number;
}
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
function scanGray(image: Image): Uint8Array {
  return image.channels === 1 && image.stride === image.width
    ? image.data.subarray(0, image.width * image.height)
    : toGray(image);
}
/** Shared retail evidence over primary profiles, plus independent additional readers. */
export class MediumMultiformatScanner {
  private medium?: IndependentScanner | WideScanner | ReleaseDetailScanner;
  private mode: Mode = "medium";
  private extra?: ExtraExports;
  private handle = 0;
  private rgba = false;
  private crop = false;
  private effort = 1;
  private qrEffort = 1;
  private disposed = false;
  private constructor() {}
  static async create(
    mediumBytes: ArrayBuffer | undefined,
    extraBytes?: ArrayBuffer,
    mode: Mode = "medium",
    recoveryBytes?: ArrayBuffer,
  ) {
    const scanner = new MediumMultiformatScanner();
    try {
      scanner.mode = mode;
      scanner.effort = { low: 0, medium: 1, high: 2, "very-high": 2 }[mode];
      scanner.qrEffort = { low: 0, medium: 1, high: 2, "very-high": 3 }[mode];
      if (mediumBytes)
        scanner.medium =
          mode !== "low" && recoveryBytes
            ? await ReleaseDetailScanner.create(mediumBytes, recoveryBytes, mode)
            : await IndependentScanner.create(mediumBytes);
      if (extraBytes) {
        const instance = await WebAssembly.instantiate(extraBytes, {});
        const e = instance.instance.exports as ExtraExports;
        for (const name of [
          "multi_new",
          "multi_free",
          "multi_prepare",
          "multi_input",
          "multi_scan",
          "multi_output",
          "multi_output_len",
        ] as const) {
          if (typeof e[name] !== "function")
            throw Error(`Invalid multiformat WASM export: ${name}`);
        }
        if (!(e.memory instanceof WebAssembly.Memory))
          throw Error("Invalid multiformat WASM memory.");
        scanner.rgba =
          typeof e.multi_capabilities === "function" &&
          (e.multi_capabilities() & 4) !== 0 &&
          [e.multi_prepare_rgba, e.multi_input_rgba, e.multi_scan_rgba].every(
            (f) => typeof f === "function",
          );
        scanner.crop =
          typeof e.multi_capabilities === "function" &&
          (e.multi_capabilities() & 16) !== 0 &&
          [e.multi_capture_source, e.multi_crop_transform, e.multi_crop].every(
            (f) => typeof f === "function",
          );
        scanner.extra = e;
        scanner.handle = e.multi_new();
        if (!scanner.handle) throw Error("Could not create additional reader session.");
      }
      return scanner;
    } catch (error) {
      scanner.dispose();
      throw error;
    }
  }
  scan(
    image: Image,
    inputFormats?: readonly string[],
    options: {
      linearStrategy?: "scanlines" | "medium-localized";
      eanAddOnSymbol?: EanAddOnSymbol;
      finishCandidates?: boolean;
    } = {},
  ): Frame {
    if (this.disposed) throw Error("Scanner is disposed.");
    const medium = this.medium;
    const scanPolicy = { ...policy, finishCandidates: options.finishCandidates ?? false };
    const formats = resolveFormats(inputFormats);
    const eanAddOnSymbol = options.eanAddOnSymbol ?? "Ignore";
    if (!["Ignore", "Read", "Require"].includes(eanAddOnSymbol))
      throw Error("Unknown EAN add-on policy.");
    const requestedStrategy = options.linearStrategy ?? "scanlines";
    if (!["scanlines", "medium-localized"].includes(requestedStrategy))
      throw Error("Unknown linear strategy.");
    // Supplements extend beyond the frozen Medium crop, so they require the
    // full-frame supplemental pass. Default EAN13 remains untouched.
    const linearStrategy = eanAddOnSymbol === "Ignore" ? requestedStrategy : "scanlines";
    const localized =
      linearStrategy === "medium-localized" &&
      formats.some((f) => f !== "EAN13" && f !== "UPCA" && supportedLinearFormats.includes(f));
    let proposals: { polygon: Quad; score?: number }[] = [];
    const start = performance.now();
    const { width, height, channels, stride, data } = image;
    if (
      !Number.isSafeInteger(width) ||
      !Number.isSafeInteger(height) ||
      width < 3 ||
      height < 3 ||
      width * height > 32 * 1024 * 1024 ||
      ![1, 3, 4].includes(channels) ||
      !Number.isSafeInteger(stride) ||
      stride < width * channels ||
      !(data instanceof Uint8Array) ||
      data.length < (height - 1) * stride + width * channels
    )
      throw Error("Invalid image dimensions or buffer.");
    const barcodes: Barcode[] = [],
      regions: Barcode[] = [];
    let primary: Frame["primary"];
    let unfinished = false,
      mediumMs = 0,
      additionalMs = 0,
      preparationMs = 0,
      localizationMs = 0;
    const sharedRetail =
      this.mode === "medium" &&
      eanAddOnSymbol === "Ignore" &&
      formats.some((f) => f === "EAN8" || f === "UPCE");
    const retailPolicy = { ...scanPolicy, ...(sharedRetail ? { retailMask: 15 } : {}) };
    const extraFormats = formats.filter(
      (f) =>
        (!sharedRetail || (f !== "EAN8" && f !== "UPCE")) &&
        (eanAddOnSymbol !== "Ignore" || (f !== "EAN13" && f !== "UPCA")),
    );
    const conservative =
      medium instanceof ReleaseDetailScanner &&
      !localized &&
      eanAddOnSymbol === "Ignore" &&
      formats.some((f) => f === "EAN13" || f === "UPCA") &&
      extraFormats.some((f) => supportedLinearFormats.includes(f));
    let preparedGray: Uint8Array | undefined;
    let preLinear: ReturnType<MediumMultiformatScanner["scanExtra"]> | undefined;
    const coverage: Quad[] = [];
    if (conservative) {
      const prepareStart = performance.now();
      preparedGray = scanGray(image);
      preparationMs += performance.now() - prepareStart;
      const extraStart = performance.now();
      preLinear = this.scanExtra(
        preparedGray,
        width,
        height,
        extraFormats.filter((f) => supportedLinearFormats.includes(f)),
        this.effort,
        eanAddOnSymbol,
      );
      additionalMs += performance.now() - extraStart;
      for (const read of preLinear.barcodes) {
        const error = read.error ?? Infinity;
        const checked =
          ["EAN8", "UPCE", "Code128"].includes(read.format) && read.support >= 3 && error <= 0.08;
        const unchecked =
          ["Code39", "ITF"].includes(read.format) &&
          read.support >= 8 &&
          error <= 0.035 &&
          read.text.length >= 8;
        if (checked || unchecked) coverage.push(read.polygon);
      }
    }
    if (sharedRetail || formats.includes("EAN13") || formats.includes("UPCA")) {
      if (!medium) throw Error("EAN13 engine has not been initialized.");
      const begin = performance.now();

      const found =
        medium instanceof ReleaseDetailScanner
          ? medium.scanLocalized(image, retailPolicy, fitLimits[this.mode], true, coverage)
          : medium.scanLocalized(image, scanPolicy, fitLimits[this.mode], true);
      primary = found;
      const localization = found.localization as unknown as Localization;
      proposals = localization.proposals;
      localizationMs = found.localizationMs;
      for (const b of found.scan.barcodes) {
        if (formats.includes("UPCA") && b.text.startsWith("0"))
          barcodes.push({
            format: "UPCA",
            text: b.text.slice(1),
            polygon: b.polygon,
            support: b.support,
          });
        else if (formats.includes("EAN13"))
          barcodes.push({ format: "EAN13", text: b.text, polygon: b.polygon, support: b.support });
      }
      if (sharedRetail) {
        const evidence = found as unknown as {
          scan: { retail?: { barcodes: Barcode[]; unfinished: boolean } };
          recovery?: {
            attempts: {
              x: number;
              y: number;
              factor: number;
              frame: { retail?: { barcodes: Barcode[]; unfinished: boolean } };
            }[];
          };
        };
        const shared: Barcode[] = [...(evidence.scan.retail?.barcodes ?? [])];
        unfinished ||= evidence.scan.retail?.unfinished ?? false;
        for (const attempt of evidence.recovery?.attempts ?? []) {
          unfinished ||= attempt.frame.retail?.unfinished ?? false;
          for (const b of attempt.frame.retail?.barcodes ?? []) {
            const polygon = b.polygon.map(([x, y]) => [
              attempt.x + x / attempt.factor,
              attempt.y + y / attempt.factor,
            ]) as unknown as Quad;
            shared.push({ ...b, polygon });
          }
        }
        barcodes.push(...shared.filter((b) => formats.includes(b.format as Format)));
      }
      const decoded = new Set(found.scan.barcodes.flatMap((b) => b.candidate_indices));
      localization.proposals.forEach((p: { polygon: Quad }, i: number) => {
        if (!decoded.has(i))
          regions.push({ format: "Unknown", text: "", polygon: p.polygon, support: 0 });
      });
      if ("recovery" in found) {
        const recovery = found.recovery as import("../detail-20260914/scanner.mjs").Recovery;
        for (const attempt of recovery.attempts) {
          const accepted = new Set(attempt.reads.flatMap((b) => b.candidate_indices));
          attempt.proposals.forEach((p, i) => {
            if (!accepted.has(i))
              regions.push({ format: "Unknown", text: "", polygon: p.polygon, support: 0 });
          });
        }
      }
      unfinished ||=
        found.scan.unfinished ||
        Boolean(localization.workLimited) ||
        (localization.omitted ?? 0) > 0;
      mediumMs = performance.now() - begin;
    }
    if (!sharedRetail && localized && !formats.includes("EAN13") && !formats.includes("UPCA")) {
      if (!medium) throw Error("EAN13 localization engine has not been initialized.");
      const begin = performance.now();
      const found = medium.scanLocalized(image, scanPolicy, fitLimits[this.mode], true)
        .localization as Localization;
      proposals = found.proposals;
      unfinished ||= Boolean(found.workLimited) || (found.omitted ?? 0) > 0;
      localizationMs = performance.now() - begin;
    }
    // UPC-A has the same optical structure as zero-prefixed EAN13, so the
    // frozen Medium reader handles it without a second optical search.
    if (extraFormats.length) {
      if (!this.extra || !this.handle) throw Error("Additional readers have not been loaded.");
      const begin = performance.now();
      const rgba =
        this.rgba &&
        !localized &&
        formats.length === 1 &&
        formats[0] === "QRCode" &&
        channels === 4 &&
        stride === width * 4;
      const gray = preparedGray ?? (rgba ? data.subarray(0, width * height * 4) : scanGray(image));
      preparationMs += performance.now() - begin;
      const matrixFormats = extraFormats.filter((f) => !supportedLinearFormats.includes(f));
      const linearFormats = extraFormats.filter((f) => supportedLinearFormats.includes(f));
      const run = (pixels: Uint8Array, w: number, h: number, enabled: Format[], effort: number) => {
        const start = performance.now();
        const result = this.scanExtra(pixels, w, h, enabled, effort, eanAddOnSymbol, rgba);
        additionalMs += performance.now() - start;
        unfinished ||= result.unfinished;
        return result;
      };
      if (!localized || !proposals.length) {
        for (const [enabled, effort] of [
          [linearFormats, this.effort],
          [matrixFormats, matrixFormats.includes("QRCode") ? this.qrEffort : 1],
        ] as const) {
          if (!enabled.length) continue;
          const result =
            enabled === linearFormats && preLinear
              ? preLinear
              : run(gray, width, height, enabled, effort);
          unfinished ||= result.unfinished;
          barcodes.push(...result.barcodes);
          regions.push(...(result.regions ?? []));
        }
      } else {
        if (matrixFormats.length) {
          const result = run(
            gray,
            width,
            height,
            matrixFormats,
            matrixFormats.includes("QRCode") ? this.qrEffort : 1,
          );
          barcodes.push(...result.barcodes);
          regions.push(...(result.regions ?? []));
        }
        if (this.crop) {
          this.extra.multi_prepare(this.handle, width, height);
          new Uint8Array(
            this.extra.memory.buffer,
            this.extra.multi_input(this.handle),
            width * height,
          ).set(gray);
          if (this.extra.multi_capture_source(this.handle) !== 0)
            throw Error("Could not retain source image.");
        }
        for (const proposal of proposals) {
          const start = performance.now();
          let crop;
          let pixels: Uint8Array | undefined;
          try {
            if (this.crop) {
              crop = rectificationPlan(proposal.polygon);
              new Float64Array(
                this.extra.memory.buffer,
                this.extra.multi_crop_transform(this.handle),
                8,
              ).set(crop.transform);
              if (this.extra.multi_crop(this.handle, crop.width, crop.height) !== 0)
                throw Error("Could not sample barcode crop.");
            } else {
              const sampled = rectify(gray, width, height, proposal.polygon);
              crop = sampled;
              pixels = sampled.data;
            }
          } catch {
            unfinished = true;
            regions.push({ format: "Unknown", text: "", polygon: proposal.polygon, support: 0 });
            continue;
          }
          preparationMs += performance.now() - start;
          const decodeStart = performance.now();
          const result = pixels
            ? run(pixels, crop.width, crop.height, linearFormats, 0)
            : this.scanPrepared(linearFormats, 0, eanAddOnSymbol);
          if (!pixels) {
            additionalMs += performance.now() - decodeStart;
            unfinished ||= result.unfinished;
          }
          const reads = result.barcodes;
          if (!reads.length)
            regions.push({ format: "Unknown", text: "", polygon: proposal.polygon, support: 0 });
          for (const read of [...reads, ...(result.regions ?? [])]) {
            const p = read.polygon;
            const mapped = (p: readonly [number, number]) => project(crop.transform, p[0], p[1]);
            read.polygon = [mapped(p[0]), mapped(p[1]), mapped(p[2]), mapped(p[3])];
            if (read.text) barcodes.push(read);
            else regions.push(read);
          }
        }
      }
    }
    if (extraFormats.length) {
      for (const read of barcodes.filter((b) => b.eanAddOn)) {
        for (const base of barcodes) {
          if (
            base.format === read.format &&
            base.text === read.text &&
            !base.eanAddOn &&
            polygonOverlap(base.polygon, read.polygon).smaller >= 0.65
          )
            base.eanAddOn = read.eanAddOn;
        }
      }
      if (eanAddOnSymbol === "Require") {
        for (let i = barcodes.length - 1; i >= 0; i--) {
          const b = barcodes[i];
          if (["EAN13", "UPCA", "EAN8", "UPCE"].includes(b.format) && !b.eanAddOn) {
            regions.push({ ...b, text: "" });
            barcodes.splice(i, 1);
          }
        }
      }
      const distinct = distinctReads(barcodes);
      barcodes.splice(0, barcodes.length, ...distinct);
      const remaining = regions.filter(
        (region) => !barcodes.some((b) => polygonOverlap(region.polygon, b.polygon).a >= 0.65),
      );
      regions.splice(0, regions.length, ...distinctReads(remaining));
    }
    const mergedLinear = mergeLinearDuplicates(barcodes, image);
    barcodes.splice(0, barcodes.length, ...mergedLinear);
    barcodes.sort((a, b) => b.support - a.support);
    barcodes.forEach((b, i) => {
      b.rank = i + 1;
    });
    return {
      primary,
      barcodes,
      eanAddOnSymbol,
      regions: [...barcodes, ...regions],
      formats,
      unfinished,
      scanMs: performance.now() - start,
      mediumMs,
      additionalMs,
      preparationMs,
      localizationMs,
      linearStrategy,
    };
  }
  private scanExtra(
    gray: Uint8Array,
    width: number,
    height: number,
    formats: Format[],
    effort: number,
    eanAddOnSymbol: EanAddOnSymbol,
    rgba = false,
  ) {
    const e = this.extra;
    if (!e || !this.handle) throw Error("Additional readers have not been loaded.");
    if ((rgba ? e.multi_prepare_rgba : e.multi_prepare)(this.handle, width, height) !== 0)
      throw Error("Additional reader rejected image size.");
    new Uint8Array(
      e.memory.buffer,
      (rgba ? e.multi_input_rgba : e.multi_input)(this.handle),
      width * height * (rgba ? 4 : 1),
    ).set(gray);
    return this.scanPrepared(formats, effort, eanAddOnSymbol, rgba);
  }
  private scanPrepared(
    formats: Format[],
    effort: number,
    eanAddOnSymbol: EanAddOnSymbol,
    rgba = false,
  ) {
    const e = this.extra;
    if (!e || !this.handle) throw Error("Additional readers have not been loaded.");
    const mask =
      maskFor(formats) |
      (eanAddOnSymbol === "Read" ? 32768 : eanAddOnSymbol === "Require" ? 65536 : 0);
    if ((rgba ? e.multi_scan_rgba : e.multi_scan)(this.handle, mask, effort) !== 0)
      throw Error("Additional scanner failed.");
    const bytes = new Uint8Array(
      e.memory.buffer,
      e.multi_output(this.handle),
      e.multi_output_len(this.handle),
    );
    const found = JSON.parse(new TextDecoder().decode(bytes)) as {
      barcodes: Barcode[];
      regions?: Barcode[];
      unfinished: boolean;
    };
    for (const b of [...found.barcodes, ...(found.regions ?? [])])
      if (!formats.includes(b.format as Format)) throw Error("Scanner returned a disabled format.");
    return found;
  }
  dispose() {
    if (this.disposed) return;
    this.medium?.dispose();
    if (this.handle) this.extra?.multi_free(this.handle);
    this.handle = 0;
    this.disposed = true;
  }
}

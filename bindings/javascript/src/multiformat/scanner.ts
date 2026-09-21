import { ReleaseDetailScanner, fitLimits } from "../detail.js";
import { IndependentScanner, runtimeHostOptions, type Image, type Quad } from "../runtime-host.mjs";
import type { Mode } from "../index.js";
import { policy } from "../policy.js";
import { rectify, rectificationPlan, project } from "../runtime-multiformat/geometry.js";
import { ExtraReaderSession, type ExtraScanResult } from "./extra-session.js";
import { toGray } from "./pixels.js";
import { resolveFormats, linearFormats as supportedLinearFormats, type Format } from "./formats.js";
import { reconcileResults } from "./result-reconciliation.js";

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
type CandidateRead = { candidate_indices: number[] };

function unreadRegions(
  proposals: readonly { polygon: Quad }[],
  reads: readonly CandidateRead[],
): Barcode[] {
  const decoded = new Set(reads.flatMap((read) => read.candidate_indices));
  return proposals.flatMap((proposal, index) =>
    decoded.has(index)
      ? []
      : [{ format: "Unknown", text: "", polygon: proposal.polygon, support: 0 }],
  );
}

function scanGray(image: Image): Uint8Array {
  return image.channels === 1 && image.stride === image.width
    ? image.data.subarray(0, image.width * image.height)
    : toGray(image);
}
/** Shared retail evidence over primary profiles, plus independent additional readers. */
export class MediumMultiformatScanner {
  private medium?: IndependentScanner | ReleaseDetailScanner;
  private mode: Mode = "medium";
  private extra?: ExtraReaderSession;
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
            : await IndependentScanner.create(mediumBytes, runtimeHostOptions.completion);
      if (extraBytes) {
        scanner.extra = await ExtraReaderSession.create(extraBytes);
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
    let preLinear: ExtraScanResult | undefined;
    const coverage: Quad[] = [];
    if (conservative) {
      const prepareStart = performance.now();
      preparedGray = scanGray(image);
      preparationMs += performance.now() - prepareStart;
      const extraStart = performance.now();
      preLinear = this.extraSession().scan(
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
      regions.push(...unreadRegions(localization.proposals, found.scan.barcodes));
      if ("recovery" in found) {
        const recovery = found.recovery as import("../runtime-detail/scanner.mjs").Recovery;
        for (const attempt of recovery.attempts)
          regions.push(...unreadRegions(attempt.proposals, attempt.reads));
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
      if (!this.extra) throw Error("Additional readers have not been loaded.");
      const begin = performance.now();
      const rgba =
        this.extra.supportsRgba &&
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
        const result = this.extraSession().scan(
          pixels,
          w,
          h,
          enabled,
          effort,
          eanAddOnSymbol,
          rgba,
        );
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
        if (this.extra.supportsCrop) this.extra.captureSource(gray, width, height);
        for (const proposal of proposals) {
          const start = performance.now();
          let crop;
          let pixels: Uint8Array | undefined;
          try {
            if (this.extra.supportsCrop) {
              crop = rectificationPlan(proposal.polygon);
              this.extra.sampleCrop(crop.transform, crop.width, crop.height);
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
            : this.extraSession().scanPrepared(linearFormats, 0, eanAddOnSymbol);
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
    reconcileResults(barcodes, regions, image, extraFormats.length > 0, eanAddOnSymbol);
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
  private extraSession(): ExtraReaderSession {
    if (!this.extra) throw Error("Additional readers have not been loaded.");
    return this.extra;
  }
  dispose() {
    if (this.disposed) return;
    this.medium?.dispose();
    this.extra?.dispose();
    this.disposed = true;
  }
}

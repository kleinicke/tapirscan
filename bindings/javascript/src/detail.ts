import { DetailScanner, type DetailResult } from "./runtime-detail/scanner.mjs";
import { IndependentScanner, type Image, type Quad } from "./runtime-host.mjs";
import type { Mode } from "./index.js";

export const fitLimits = { low: 0, medium: 1, high: 4, "very-high": 1 } as const;
/** Preserve the public gray/RGB/RGBA and padded-stride image contract. */
function packed(image: Image): Image {
  if (
    // Runtime callers can bypass the TypeScript image contract.
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
    !image ||
    !Number.isSafeInteger(image.width) ||
    !Number.isSafeInteger(image.height) ||
    image.width < 3 ||
    image.height < 3 ||
    image.width * image.height > 32 * 1024 * 1024 ||
    ![1, 3, 4].includes(image.channels) ||
    !Number.isSafeInteger(image.stride) ||
    image.stride < image.width * image.channels ||
    !(image.data instanceof Uint8Array) ||
    image.data.length < (image.height - 1) * image.stride + image.width * image.channels
  )
    throw Error("Invalid image dimensions or buffer");
  if (image.channels === 4 && image.stride === image.width * 4) {
    let opaque = true;
    for (let i = 3; i < image.width * image.height * 4; i += 4) {
      if (image.data[i] !== 255) {
        opaque = false;
        break;
      }
    }
    if (opaque) return image;
  }
  const data = new Uint8Array(image.width * image.height * 4);
  for (let y = 0; y < image.height; y++)
    for (let x = 0; x < image.width; x++) {
      const src = y * image.stride + x * image.channels,
        dst = (y * image.width + x) * 4;
      data[dst] = image.data[src];
      data[dst + 1] = image.data[src + (image.channels === 1 ? 0 : 1)];
      data[dst + 2] = image.data[src + (image.channels === 1 ? 0 : 2)];
      data[dst + 3] = 255;
    }
  return { data, width: image.width, height: image.height, channels: 4, stride: image.width * 4 };
}
/** Flatten decoded outputs only; retain crop-local evidence in recovery.attempts. */
export class ReleaseDetailScanner {
  private disposed = false;
  private constructor(private readonly scanner: DetailScanner) {}
  static async create(primary: ArrayBuffer, low: ArrayBuffer, mode: Exclude<Mode, "low">) {
    return new ReleaseDetailScanner(
      await DetailScanner.create(primary, low, mode === "medium" ? 1 : 2),
    );
  }
  scanLocalized(
    image: Image,
    policy: Parameters<IndependentScanner["scanLocalized"]>[1] & { retailMask?: number },
    fit: number,
    full: boolean,
    coverage: readonly Quad[] = [],
  ): DetailResult {
    if (this.disposed) throw Error("Scanner is disposed");
    const result = this.scanner.scanLocalized(packed(image), policy, fit, full, coverage);
    const primaryCount = result.scan.barcodes.length;
    const barcodes = result.recovery.barcodes.map((b, i) =>
      i < primaryCount ? b : { ...b, candidate_indices: [] },
    );
    const localization = result.localization as {
      proposals: { polygon: Quad; score: number; text: string }[];
      omitted: number;
      workLimited: boolean;
    };
    return {
      ...result,
      localization: {
        ...localization,
        proposals: localization.proposals.map((p) => ({
          polygon: p.polygon,
          score: p.score,
          text: p.text,
        })),
      },
      scan: {
        ...result.scan,
        barcodes,
        unfinished:
          result.scan.unfinished ||
          result.recovery.searchLimited ||
          result.recovery.attempts.some((a) => a.unfinished),
      },
    };
  }
  dispose() {
    if (!this.disposed) {
      this.scanner.dispose();
      this.disposed = true;
    }
  }
}

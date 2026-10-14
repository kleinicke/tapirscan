// Worker side of tapirscan/browser: one core Scanner, fed with decoded pixels.
import {
  Scanner,
  type EanAddOnPolicy,
  type ExperimentalTurbo,
  type FormatSelection,
  type Mode,
  type PixelImage,
  type ScanOptions,
} from "./index.js";
import { scanRows, workingPixels, type RowImage, type RowReader } from "./row-scan.js";

/** Scanner options that can cross the worker boundary. */
export interface WorkerScannerOptions {
  mode?: Mode;
  formats?: FormatSelection;
  eanAddOnPolicy?: EanAddOnPolicy;
  experimentalTurbo?: ExperimentalTurbo;
  /** Absolute URL of a directory serving the packaged WASM files. */
  wasmBaseUrl?: string;
}
/** Inputs that are cheap to send: everything else becomes an ImageBitmap first. */
export type WorkerSource = Blob | ImageBitmap | PixelImage;
export type WorkerRequest =
  | { id: number; type: "create"; options: WorkerScannerOptions }
  | { id: number; type: "scan" | "inspect"; source: WorkerSource; options: ScanOptions };
export type WorkerResponse =
  | { id: number; value: unknown }
  | { id: number; error: { name: string; message: string; code?: string } };

// Literal URLs let bundlers such as Vite and webpack emit these assets without
// configuration. Keep the file names in sync with index.ts (verify-package checks).
const wasm: Record<Mode, URL> = {
  low: new URL("../wasm/low.wasm", import.meta.url),
  medium: new URL("../wasm/medium.wasm", import.meta.url),
  high: new URL("../wasm/high.wasm", import.meta.url),
  "very-high": new URL("../wasm/very-high.wasm", import.meta.url),
};
const turboWasm: Record<ExperimentalTurbo, URL> = {
  2: new URL("../wasm/experimental-turbo2.wasm", import.meta.url),
  4: new URL("../wasm/experimental-turbo4.wasm", import.meta.url),
  8: new URL("../wasm/experimental-turbo8.wasm", import.meta.url),
  16: new URL("../wasm/experimental-turbo16.wasm", import.meta.url),
};

// Typed locally: the DOM and WebWorker libraries cannot share one compilation.
const scope = globalThis as unknown as {
  onmessage: ((event: MessageEvent<WorkerRequest>) => void) | null;
  postMessage(message: WorkerResponse): void;
};
let scanner: Scanner | undefined;

async function load(url: URL): Promise<ArrayBuffer> {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`WASM load failed: ${String(response.status)} ${url.href}`);
  return response.arrayBuffer();
}

/**
 * Canvas reused for images up to 16 Mi pixels (4K frames, 12-megapixel photos), so
 * it holds at most 64 MiB; it is resized only when the image size changes.
 */
let decodeContext: OffscreenCanvasRenderingContext2D | undefined;
const reusedPixels = 16 * 1024 * 1024;
/**
 * Pixel bytes per band when a large image is decoded band by band. Each band of a
 * GPU-backed bitmap costs one readback: 64 MiB (three bands at 48 MP) measured
 * 17% slower than one full-size canvas for canvas sources and as fast for Blobs.
 */
const bandBytes = 64 * 1024 * 1024;

function context2d(width: number, height: number): OffscreenCanvasRenderingContext2D {
  const reuse = width * height <= reusedPixels;
  if (reuse && decodeContext) {
    if (decodeContext.canvas.width !== width || decodeContext.canvas.height !== height) {
      decodeContext.canvas.width = width;
      decodeContext.canvas.height = height;
    }
    return decodeContext;
  }
  const created = new OffscreenCanvas(width, height).getContext("2d", {
    willReadFrequently: true,
  });
  if (!created) throw new Error("2D canvas is unavailable in this browser's workers");
  if (reuse) decodeContext = created;
  return created;
}

/** Composite `bitmap` onto white, as images are displayed, with its row `top` at y = 0. */
function draw(context: OffscreenCanvasRenderingContext2D, bitmap: ImageBitmap, top: number): void {
  // The scanner ignores alpha, so transparent areas would otherwise read as black.
  context.fillStyle = "#fff";
  context.fillRect(0, 0, context.canvas.width, context.canvas.height);
  context.drawImage(bitmap, 0, -top);
}

type Decoded =
  { image: PixelImage; release(): void } | { rows: RowImage; read: RowReader; release(): void };

/**
 * Decode a Blob or ImageBitmap onto white RGBA pixels; pixel inputs pass through.
 * Images above the working size are read band by band, so neither a full-size
 * canvas nor a full-size pixel copy is needed.
 */
async function decode(source: WorkerSource): Promise<Decoded> {
  if (!(source instanceof Blob) && !(source instanceof ImageBitmap))
    return { image: source, release: () => undefined };
  const bitmap = source instanceof Blob ? await createImageBitmap(source) : source;
  const release = () => {
    bitmap.close();
  };
  try {
    const { width, height } = bitmap;
    // Reject before allocating a canvas; the scanner enforces the same limits.
    if (
      width < 3 ||
      height < 3 ||
      Math.max(width, height) > 65_535 ||
      width * height > 256 * 1024 * 1024
    )
      throw new TypeError(
        `Images must be 3 to 65,535 pixels per side and at most 256 megapixels, got ${String(width)}×${String(height)}`,
      );
    if (width * height > workingPixels) {
      const band = Math.max(1, Math.floor(bandBytes / (width * 4)));
      const context = context2d(width, Math.min(band, height));
      let top = -1;
      let pixels = new Uint8Array(0);
      const read: RowReader = (y, count, target) => {
        for (let done = 0; done < count;) {
          const row = y + done;
          if (top < 0 || row < top || row >= top + band) {
            top = row - (row % band);
            draw(context, bitmap, top);
            const rows = Math.min(band, height - top);
            // A same-type view keeps the copies below block copies.
            const { data } = context.getImageData(0, 0, width, rows);
            pixels = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
          }
          const take = Math.min(count - done, top + band - row, height - row);
          const from = (row - top) * width * 4;
          target.set(pixels.subarray(from, from + take * width * 4), done * width * 4);
          done += take;
        }
      };
      return { rows: { width, height, channels: 4 }, read, release };
    }
    const context = context2d(width, height);
    draw(context, bitmap, 0);
    const image = context.getImageData(0, 0, width, height);
    release();
    return { image, release: () => undefined };
  } catch (error) {
    release();
    throw error;
  }
}

async function handle(request: WorkerRequest): Promise<unknown> {
  if (request.type === "create") {
    const { wasmBaseUrl, ...options } = request.options;
    const { experimentalTurbo: turbo, mode = "medium" } = options;
    // The core validates options before loading; a custom base URL uses its loader.
    // Invalid modes or presets fail validation, so the lookup below is never used for them.
    const asset = turbo === undefined ? wasm[mode] : turboWasm[turbo];
    scanner = await Scanner.create(
      wasmBaseUrl === undefined
        ? { ...options, loadWasm: () => load(asset) }
        : { ...options, wasmBaseUrl },
    );
    return { mode: scanner.mode, formats: scanner.formats, eanAddOnPolicy: scanner.eanAddOnPolicy };
  }
  if (!scanner) throw new Error("Scanner is not created");
  const decoded = await decode(request.source);
  try {
    const inspect = request.type === "inspect";
    if ("rows" in decoded)
      return scanRows(scanner, decoded.rows, decoded.read, request.options, inspect);
    return inspect
      ? scanner.inspect(decoded.image, request.options)
      : scanner.scan(decoded.image, request.options);
  } finally {
    decoded.release();
  }
}

scope.onmessage = ({ data: request }) => {
  handle(request).then(
    (value) => {
      scope.postMessage({ id: request.id, value });
    },
    (error: unknown) => {
      const { name, message, code } =
        error instanceof Error
          ? (error as Error & { code?: string })
          : { name: "Error", message: String(error), code: undefined };
      scope.postMessage({
        id: request.id,
        error: { name, message, ...(typeof code === "string" ? { code } : {}) },
      });
    },
  );
};

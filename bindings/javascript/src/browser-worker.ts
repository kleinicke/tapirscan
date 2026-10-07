// Worker side of tapirscan/browser: one core Scanner, fed with decoded pixels.
import {
  Scanner,
  type EanAddOnPolicy,
  type FormatSelection,
  type Mode,
  type PixelImage,
  type ScanOptions,
} from "./index.js";

/** Scanner options that can cross the worker boundary. */
export interface WorkerScannerOptions {
  mode?: Mode;
  formats?: FormatSelection;
  eanAddOnPolicy?: EanAddOnPolicy;
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
  low: new URL("../wasm/low-scan-results-20261007.wasm", import.meta.url),
  medium: new URL("../wasm/medium-scan-results-20261007.wasm", import.meta.url),
  high: new URL("../wasm/high-scan-results-20261007.wasm", import.meta.url),
  "very-high": new URL("../wasm/very-high-scan-results-20261007.wasm", import.meta.url),
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

/** Decode a Blob or ImageBitmap to RGBA pixels; pixel inputs pass through. */
async function pixels(source: WorkerSource): Promise<PixelImage> {
  if (!(source instanceof Blob) && !(source instanceof ImageBitmap)) return source;
  const bitmap = source instanceof Blob ? await createImageBitmap(source) : source;
  try {
    const { width, height } = bitmap;
    // Reject before allocating a canvas; the scanner enforces the same limits.
    if (width < 3 || height < 3 || width * height > 32 * 1024 * 1024)
      throw new TypeError(
        `Images must be 3×3 to 32 megapixels, got ${String(width)}×${String(height)}`,
      );
    const context = new OffscreenCanvas(width, height).getContext("2d", {
      willReadFrequently: true,
    });
    if (!context) throw new Error("2D canvas is unavailable in this browser's workers");
    context.drawImage(bitmap, 0, 0);
    return context.getImageData(0, 0, width, height);
  } finally {
    bitmap.close();
  }
}

async function handle(request: WorkerRequest): Promise<unknown> {
  if (request.type === "create") {
    const { wasmBaseUrl, ...options } = request.options;
    const mode = options.mode ?? "medium";
    // The core validates options before loading; a custom base URL uses its loader.
    scanner = await Scanner.create(
      wasmBaseUrl === undefined
        ? { ...options, loadWasm: () => load(wasm[mode]) }
        : { ...options, wasmBaseUrl },
    );
    return { mode: scanner.mode, formats: scanner.formats, eanAddOnPolicy: scanner.eanAddOnPolicy };
  }
  if (!scanner) throw new Error("Scanner is not created");
  const image = await pixels(request.source);
  return request.type === "scan"
    ? scanner.scan(image, request.options)
    : scanner.inspect(image, request.options);
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

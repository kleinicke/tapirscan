// Compile-time consumer contract. This function is never executed.
import {
  scan,
  Scanner,
  type Barcode,
  type Format,
  type Image,
  type InspectionResult,
  type ScanResult,
  type ScannerOptions,
} from "../dist/index.js";

export async function consumer(data: Uint8Array, imageData: ImageData) {
  const image: Image = { data, width: 640, height: 480, channels: 1 };
  const scanner = await Scanner.create({
    mode: "high",
    formats: "1D",
    eanAddOnPolicy: "read",
    wasmBaseUrl: "/engines/",
  });
  try {
    const scanResult: ScanResult = scanner.scan(image);
    const barcodes: readonly Barcode[] = scanResult.barcodes;
    console.log(barcodes.length);
    const first: Barcode | undefined = scanResult.best;
    // Decoded barcodes always have a known format.
    const format: Format | undefined = first?.format;
    console.log(format);
    console.log(first?.text);
    // @ts-expect-error Ordinary scans do not collect diagnostics.
    console.log(scanResult.diagnostics);
    // @ts-expect-error Inspection is a separate operation.
    scanner.scan(image, { debug: true });
    const result: InspectionResult = scanner.inspect(image, {});
    const best: Barcode | undefined = result.best;
    const values: readonly string[] = result.values;
    const formats = scanner.formats;
    for (const region of result.undecoded) {
      // @ts-expect-error Undecoded regions have no decoded text.
      region.text = "decoded";
    }
    // @ts-expect-error Supplement policy is fixed at creation.
    scanner.eanAddOnPolicy = "ignore";
    // @ts-expect-error No per-call supplement policy.
    scanner.inspect(image, { eanAddOnPolicy: "require" });
    // @ts-expect-error Invalid supplement policy.
    await scan(image, { eanAddOnPolicy: "Read" });
    for (const region of result.diagnostics.regions.undecoded) {
      // @ts-expect-error Undecoded regions have geometry, not decoded text.
      region.text = "decoded";
      // @ts-expect-error Region geometry is immutable.
      region.polygon[0][0] = 0;
    }
    const bytes: readonly number[] | undefined = best?.payloadBytes;
    if (bytes) {
      // @ts-expect-error Original payload bytes remain immutable.
      bytes[0] = 0;
    }
    // @ts-expect-error Creation formats remain immutable.
    formats[0] = "QRCode";
    await scan(imageData, { formats: ["EAN13"] });
    // @ts-expect-error Results are immutable, including nested coordinates.
    result.barcodes[0].polygon[0][0] = 0;
    // @ts-expect-error Derived values cannot drift through caller mutation.
    result.values[0] = "changed";
    // @ts-expect-error Select one read through result.best.
    scanner.inspect(image, { multiple: false });
    // @ts-expect-error There is one diagnostic option.
    scanner.inspect(image, { includeRegions: true });
    scanner.inspect(image, { formats: "EAN13" });
    // @ts-expect-error Removed in 1.3.0.
    scanner.inspect(image, { extendedBudget: true });
    const support: number | undefined = best?.support;
    const append: number | undefined = best?.structuredAppend?.index;
    const startStop: string | undefined = best?.codabarStartStop;
    if (best?.structuredAppend) {
      // @ts-expect-error Metadata remains immutable.
      best.structuredAppend.index = 2;
    }
    return { best, values, support, append, startStop };
  } finally {
    scanner.dispose();
  }
}

// Experimental values stay numeric; runtime validation rejects mode + preset.
void Scanner.create({ experimentalTurbo: 4 });
// @ts-expect-error Turbo accepts only the four named presets.
void Scanner.create({ experimentalTurbo: 3 });
export interface ApplicationOptions extends ScannerOptions {
  label: string;
}

// tapirscan/browser: synchronous construction, promise results, browser sources.
import {
  scan as scanSource,
  Scanner as BrowserScanner,
  type InspectionResult as BrowserResult,
  type ScanResult as BrowserScanResult,
} from "../dist/browser.js";

export async function browserConsumer(file: File, video: HTMLVideoElement, image: Image) {
  const scanner = new BrowserScanner({ mode: "low", formats: "retail" });
  try {
    await scanner.ready;
    const fromFile: BrowserScanResult = await scanner.scan(file);
    const fromVideo: BrowserResult = await scanner.inspect(video, { formats: "EAN13" });
    const fromPixels: BrowserScanResult = await scanSource(image, { formats: "EAN13" });
    await scanSource(image, { transfer: true, mode: "low" });
    // @ts-expect-error Results are frozen.
    fromFile.barcodes[0].text = "changed";
    // @ts-expect-error Functions cannot reach the worker; use the core entry.
    new BrowserScanner({ loadWasm: () => Promise.resolve(new ArrayBuffer(0)) });
    // @ts-expect-error Scans need an image source.
    await scanner.scan("photo.png");
    return [fromFile.best?.text, fromVideo.values, fromPixels.barcodes.length];
  } finally {
    scanner.dispose();
  }
}

// tapirscan/browser/<mode> and tapirscan/camera: one bundled mode, any scanner.
import {
  Scanner as MediumScanner,
  scan as scanMedium,
  type OneShotOptions as MediumOneShotOptions,
  type ScannerOptions as MediumOptions,
} from "../dist/browser-medium.js";
import { CameraScanner } from "../dist/camera.js";

export async function cameraConsumer(video: HTMLVideoElement, image: Image) {
  const scanner = new MediumScanner({ formats: ["EAN13", "QRCode"] });
  // @ts-expect-error A single-mode entry has no mode option.
  new MediumScanner({ mode: "medium" });
  // @ts-expect-error Turbo presets need tapirscan/browser.
  new MediumScanner({ experimentalTurbo: 8 });
  await scanMedium(image, { transfer: true });
  // Each entry's option types list only the options it accepts.
  const options: MediumOptions = { formats: "1D" };
  // @ts-expect-error A single-mode entry has no mode option.
  const withMode: MediumOptions = { mode: "high" };
  const oneShot: MediumOneShotOptions = { ...options, transfer: true };
  // @ts-expect-error Turbo presets need tapirscan/browser.
  const withTurbo: MediumOneShotOptions = { experimentalTurbo: 8 };
  await scanMedium(image, oneShot);
  new MediumScanner(options);
  const values: string[][] = [];
  const camera = new CameraScanner(video, scanner, {
    onScan: (result) => values.push([...result.values]),
    onError: (error: Error) => error.message,
    scanOptions: { formats: "EAN13" },
    constraints: { width: { ideal: 640 } },
  });
  // Scanners of every entry fit.
  new CameraScanner(video, new BrowserScanner());
  const detected: string[] = [];
  new CameraScanner(video, scanner, {
    onDetect: (barcode) => detected.push(barcode.text),
    confirmations: 2,
    scanInterval: 200,
    region: { left: 0.1, top: 0.3, width: 0.8, height: 0.4 },
  });
  const started: boolean = await camera.start({ facingMode: "user" });
  const cameras: MediaDeviceInfo[] = await CameraScanner.cameras();
  await camera.start({ deviceId: cameras[0]?.deviceId });
  const torch: boolean = camera.torchAvailable && (await camera.setTorch(true));
  const track: MediaStreamTrack | undefined = camera.track;
  camera.stop();
  scanner.dispose();
  return [started, torch, track, camera.running, values, detected, withMode, withTurbo];
}

// tapirscan/support and a worker started by the application.
import { browserSupported, wasmSupported } from "../dist/support.js";

export function workerConsumer(): boolean {
  const scanner = new MediumScanner({
    wasmBaseUrl: new URL("tapirscan/", document.baseURI),
    worker: () => new Worker(new URL("./scanner.worker.js", import.meta.url), { type: "module" }),
  });
  // @ts-expect-error The worker option takes a function, not a URL.
  new MediumScanner({ worker: "./scanner.worker.js" });
  scanner.dispose();
  return browserSupported() && wasmSupported();
}

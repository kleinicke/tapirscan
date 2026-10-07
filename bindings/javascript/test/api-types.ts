// Compile-time consumer contract. This function is never executed.
import {
  best as bestOf,
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
    eanAddOnPolicy: "Read",
    wasmBaseUrl: "/engines/",
  });
  try {
    const scanResult: ScanResult = scanner.scan(image);
    const barcodes: readonly Barcode[] = scanResult.barcodes;
    const first: Barcode | undefined = bestOf(barcodes);
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
    scanner.eanAddOnPolicy = "Ignore";
    // @ts-expect-error No per-call supplement policy.
    scanner.inspect(image, { eanAddOnPolicy: "Require" });
    // @ts-expect-error Invalid supplement policy.
    await scan(image, { eanAddOnPolicy: "read" });
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
    scanner.inspect(image, { formats: "EAN13", extendedBudget: true });
    // @ts-expect-error Continuation is a boolean.
    scanner.inspect(image, { extendedBudget: "yes" });
    const support: number | undefined = best?.support;
    const append: number | undefined = best?.structuredAppend?.index;
    if (best?.structuredAppend) {
      // @ts-expect-error Metadata remains immutable.
      best.structuredAppend.index = 2;
    }
    return { best, values, support, append };
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
  best as bestRead,
  type InspectionResult as BrowserResult,
  type ScanResult as BrowserScanResult,
} from "../dist/browser.js";

export async function browserConsumer(file: File, video: HTMLVideoElement, image: Image) {
  const scanner = new BrowserScanner({ mode: "low", formats: "retail" });
  try {
    await scanner.ready;
    const fromFile: BrowserScanResult = await scanner.scan(file);
    const fromVideo: BrowserResult = await scanner.inspect(video, { extendedBudget: true });
    const fromPixels: BrowserScanResult = await scanSource(image, { formats: "EAN13" });
    // @ts-expect-error Results are frozen.
    fromFile.barcodes[0].text = "changed";
    // @ts-expect-error Functions cannot reach the worker; use the core entry.
    new BrowserScanner({ loadWasm: () => Promise.resolve(new ArrayBuffer(0)) });
    // @ts-expect-error Scans need an image source.
    await scanner.scan("photo.png");
    return [bestRead(fromFile.barcodes)?.text, fromVideo.values, fromPixels.barcodes.length];
  } finally {
    scanner.dispose();
  }
}

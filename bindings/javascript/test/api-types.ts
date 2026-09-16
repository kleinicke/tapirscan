// Compile-time consumer contract. This function is never executed.
import { scan, Scanner, type Barcode, type Image, type ScanResult } from "../dist/index.js";

export async function consumer(data: Uint8Array, imageData: ImageData) {
  const image: Image = { data, width: 640, height: 480, channels: 1 };
  const scanner = await Scanner.create({
    mode: "high",
    formats: "1D",
    eanAddOnPolicy: "Read",
    wasmBaseUrl: "/engines/",
  });
  try {
    const result: ScanResult = scanner.scan(image, { debug: true });
    const best: Barcode | undefined = result.best;
    const values: readonly string[] = result.values;
    const formats = scanner.formats;
    // @ts-expect-error Supplement policy is fixed at creation.
    scanner.eanAddOnPolicy = "Ignore";
    // @ts-expect-error No per-call supplement policy.
    scanner.scan(image, { eanAddOnPolicy: "Require" });
    // @ts-expect-error Invalid supplement policy.
    await scan(image, { eanAddOnPolicy: "read" });
    for (const region of result.debug?.regions.undecoded ?? []) {
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
    await scan(imageData, { formats: ["EAN13"], debug: true });
    // @ts-expect-error Results are immutable, including nested coordinates.
    result.barcodes[0].polygon[0][0] = 0;
    // @ts-expect-error Derived values cannot drift through caller mutation.
    result.values[0] = "changed";
    // @ts-expect-error Select one read through result.best.
    scanner.scan(image, { multiple: false });
    // @ts-expect-error There is one diagnostic option.
    scanner.scan(image, { includeRegions: true });
    scanner.scan(image, { formats: "EAN13" });
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

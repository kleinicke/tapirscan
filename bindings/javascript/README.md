# Tapirscan for JavaScript and TypeScript

Scan image pixels in a browser or Node with the same Rust/WASM core.
[Quick start](#quick-start) · [WASM loading](#wasm-loading) · [Functions](#functions) · [All options](#all-options) · [Results](#results)

## Quick start

```sh
npm install tapirscan
```

Registry publication is pending; until then, install a tarball from the
[local build guide](../../docs/DEVELOPMENT.md). TypeScript declarations and WASM
binaries are included. Your runtime must support WebAssembly SIMD.

Pass a canvas's `ImageData` directly:

```js
import { scan } from "tapirscan";

// Using an existing canvas and its 2D context:
const image = context.getImageData(0, 0, canvas.width, canvas.height);
const result = await scan(image);
console.log(result.values); // e.g. ["4006381333931"]
```

Defaults are Medium effort, EAN13, multiple results, and debug disabled.
`result.barcodes` also gives each read's text, format, polygon and rectangle.
The helper creates and disposes a scanner automatically. Browser apps need to
serve its [WASM assets](#wasm-loading); Node loads the packaged files automatically.

For more control or repeated images, reuse a scanner:

```js
import { Scanner } from "tapirscan";

const scanner = await Scanner.create({ mode: "high", formats: "1D" });
try {
  const result = scanner.scan(image);
  for (const barcode of result.barcodes) {
    console.log(barcode.text, barcode.format, barcode.polygon);
  }
} finally {
  scanner.dispose();
}
```

Here `image` is the `ImageData` above. `formats: "1D"` enables all supported linear
formats; additional readers are experimental. Settings also work with the helper:
`await scan(image, { mode: "high", formats: "1D" })`.

## WASM loading

In Node, the default loader reads assets from the installed package. Decode your
image with an image library first, then pass grayscale, RGB or RGBA bytes:

```js
const result = await scan({ data: pixels, width, height, channels: 1, stride: width });
```

Here `pixels` is a Uint8Array of decoded grayscale pixels. Image codecs are not
included; filenames, URLs and encoded JPEG/PNG bytes are not scan inputs.

In a browser, the default loader fetches assets relative to the module. If your
bundler relocates modules, copy the WASMs into your public directory:

```sh
mkdir -p public/tapirscan
cp node_modules/tapirscan/wasm/*.wasm public/tapirscan/
```

Then supply a loader pointing at those files:

```js
const scanner = await Scanner.create({
  loadWasm: async (url) => {
    const filename = url.pathname.split("/").pop();
    const response = await fetch(`/tapirscan/${filename}`);
    if (!response.ok) throw new Error(`Could not load scanner: ${response.status}`);
    return response.arrayBuffer();
  },
});
try {
  console.log(scanner.scan(image).values);
} finally {
  scanner.dispose();
}
```

Use the deployed base path if your app is hosted below a subpath. Copy all current
WASMs: Medium/High/Very high also load the Low recovery decoder. The demo and its
comparison engines are not needed in your app.

## Camera and worker use

Initialization is asynchronous; scanning is synchronous. For a responsive browser
UI, initialize one scanner inside a Web Worker and transfer an owned frame buffer.
Capture the next frame after the previous result arrives. Avoid racing scanner
initialization or modifying pixels during scanning. The [demo worker](../../demo/src/lib/scan.worker.ts)
shows a complete integration. Camera capture belongs to your app and requires
HTTPS (localhost works for development).

## Moving from ZXing

For [`zxing-wasm`](https://github.com/Sec-ant/zxing-wasm), you can keep your
existing ImageData and replace the decoding call:

```js
// Before:
import { readBarcodes } from "zxing-wasm/reader";
const reads = await readBarcodes(image, { formats: ["EAN13"] });
const oldValues = reads.filter((read) => read.isValid).map((read) => read.text);

// After:
import { scan } from "tapirscan";
const result = await scan(image, { formats: ["EAN13"] });
const values = result.values;
```

Here `image` is decoded ImageData. Keep an explicit format selection during
migration; Tapirscan defaults to EAN13. For successive frames, initialize a
`Scanner` once and scan inside a worker, as shown above.

| Existing ZXing integration               | Tapirscan equivalent or difference                                                          |
| ---------------------------------------- | ------------------------------------------------------------------------------------------- |
| Array of reads                           | `result.barcodes`, or `result.values` for strings.                                          |
| `read.text`                              | `barcode.text`.                                                                             |
| `read.position`                          | `barcode.polygon` as four [x, y] corners, or `barcode.rect`.                                |
| `tryHarder`, `tryRotate`, `tryDownscale` | No direct option mapping. Select an effort mode and measure your images.                    |
| `maxNumberOfSymbols`                     | No arbitrary count limit; `multiple: false` selects one after the scan, without early exit. |
| Blob or encoded image input              | Decode to ImageData or a supported byte buffer before scanning.                             |
| WASM overrides/asset paths               | Use Tapirscan's `loadWasm` and packaged assets.                                             |

[`@zxing/browser`](https://github.com/zxing-js/browser) also manages browser image
and video acquisition. Tapirscan's scanner accepts pixels; it does not replace
camera-device helpers or continuous-scan callbacks. Keep your capture loop,
draw frames to a canvas, and pass ImageData to a reusable scanner. Stop media
tracks when capture ends, and dispose the scanner when finished. See the
[demo](../../demo/README.md) for capture behavior.

Check supported formats and reader-specific metadata before switching. Additional
formats are experimental, and Tapirscan is not a drop-in replacement for every
ZXing package. Run both readers on representative inputs before replacing one.

## Functions

| Function                            | Return type            | Behavior                                                                                                                  |
| ----------------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `scan(image, options = {})`         | `Promise<ScanResult>`  | One image with automatic scanner creation and disposal, including on failure. Accepts creation and scan options together. |
| `Scanner.create(options = {})`      | `Promise<Scanner>`     | Initialize a reusable scanner. Mode and formats are fixed for its lifetime.                                               |
| `scanner.scan(image, options = {})` | `ScanResult`           | Synchronously scan pixels. Accepts scan options only.                                                                     |
| `scanner.best(result)`              | `Barcode \| undefined` | Convenience alias for `result.best`.                                                                                      |
| `scanner.dispose()`                 | `void`                 | Release WASM sessions. Repeated disposal is safe; do not scan after disposal.                                             |

`image` is required for either scan function. All options are optional. Reuse a
scanner for successive frames to avoid repeated initialization; create another
to change effort or formats. Previously returned results survive disposal.

## All options

| Option           | Where    | Default                | Meaning                                                                                                    |
| ---------------- | -------- | ---------------------- | ---------------------------------------------------------------------------------------------------------- |
| `mode`           | Creation | `"medium"`             | `"low"`, `"medium"`, `"high"`, `"very-high"`.                                                              |
| `formats`        | Creation | `["EAN13"]`            | `"1D"`, `"2D"`, `"all"`, or a nonempty array of exact identifiers.                                         |
| `loadWasm`       | Creation | Module-relative loader | `(url: URL) => Promise<ArrayBuffer>`. Uses HTTP fetch in browsers and filesystem reads for Node file URLs. |
| `multiple`       | Scan     | `true`                 | False keeps at most the highest-support read after scanning; it does not provide an early exit.            |
| `debug`          | Scan     | `false`                | Include search evidence under `result.debug`. Decoded polygons are always returned.                        |
| `includeRegions` | Scan     | Unset                  | Compatibility alias for `debug`; prefer `debug` in new code. Conflicting values are rejected.              |

Format presets cover supported symbologies. Exports `linearFormats`, `matrixFormats`
and `retailFormats` let you compose custom selections; `formatBits` provides their
native bit mapping. See [identifiers and coverage](../../docs/FORMATS.md).
The four effort modes tune EAN13/UPCA; additional readers use fixed effort.

Resolution, camera capture, preprocessing rotation, ROI, confidence thresholds,
timeouts and exact work budgets are not public scan options. Demo capture and
resize settings belong to the application.

## Image input

`PixelImage` accepts `ImageData` (or its data/width/height fields) or an explicit
`Image` buffer:

| Field             | Type          | Meaning                                                                                                 |
| ----------------- | ------------- | ------------------------------------------------------------------------------------------------------- |
| `data`            | `Uint8Array`  | Decoded pixels. ImageData instead uses `Uint8ClampedArray` and implies tightly packed RGBA.             |
| `width`, `height` | `number`      | Integer input dimensions, at least 3 pixels each.                                                       |
| `channels`        | `1 \| 3 \| 4` | Grayscale, RGB or RGBA. Alpha is ignored. Required for explicit buffers.                                |
| `stride`          | `number`      | Bytes between row starts, at least width × channels. Required for explicit buffers; padding is allowed. |

Input is limited to 128 MiB of addressed pixels. Keep the buffer stable during the
call. Convert DOM image elements or encoded images to pixels before scanning.

## Results

| Field               | Type                                                           | Meaning                                                                           |
| ------------------- | -------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `result.values`     | `string[]`                                                     | Decoded strings.                                                                  |
| `result.barcodes`   | `Barcode[]`                                                    | Decoded values with format and geometry.                                          |
| `result.best`       | `Barcode \| undefined`                                         | Highest-support read, or undefined when empty.                                    |
| `result.image`      | `{ width: number, height: number }`                            | Dimensions of supplied pixels.                                                    |
| `result.mode`       | `Mode`                                                         | Selected effort.                                                                  |
| `result.elapsedMs`  | `number`                                                       | Host scan time in milliseconds; excludes file loading and scanner initialization. |
| `result.unfinished` | `boolean`                                                      | Incomplete work; returned reads may still be useful.                              |
| `result.debug`      | `Diagnostics \| undefined`                                     | Requested diagnostic evidence; absent by default.                                 |
| `barcode.text`      | `string`                                                       | Decoded text.                                                                     |
| `barcode.format`    | `Format \| "Unknown"`                                          | Symbology identifier.                                                             |
| `barcode.polygon`   | `Quad`                                                         | Four `[x, y]` corners in input-image coordinates.                                 |
| `barcode.rect`      | `{ left: number, top: number, width: number, height: number }` | Enclosing integer rectangle.                                                      |

Both result arrays are empty when nothing is decoded. Coordinates start at the
top left, x rightward and y downward. Geometry is returned, not a cropped bitmap.
Map coordinates back yourself if you resize/rotate before scanning. Support is a
ranking heuristic, not a probability.

The package exports `ScannerOptions`, `ScanOptions`, `ScanResult`, `Barcode`,
`PixelImage`, `Image`, `Quad`, `Mode`, `Format`, `FormatSelection`, `Diagnostics`
and `DiagnosticBarcode` types. TypeScript infers results from calls; runtime
checks still validate pixel buffers and dimensions.

## Diagnostics and errors

```js
const result = scanner.scan(image, { debug: true });
if (result.debug) {
  console.log(result.debug.localization, result.debug.searchWindows);
  console.log(result.debug.scan.barcodes);
}
```

Diagnostics retain the raw schema-2 result: `scan` includes support and candidate
evidence, and `localizationLimited` reports localization limits. Depending on the
reader, `localization`, `searchWindows`, `recovery` and `detailRegions` may be
present. Raw metadata includes GS1 and structured append where supported.
Candidate indices inside recovery crops are local to the crop and are not
identifiers for tracking between frames.

Invalid options can raise TypeError. Scanner validation and engine failures can
raise the exported `ScannerError` with a `.code` and `.message`. Loader/fetch
errors propagate to the caller; creation and the one-shot helper reject their
promises on failure. Always dispose reusable scanners with `finally`.

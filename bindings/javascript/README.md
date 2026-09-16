# Tapirscan for JavaScript and TypeScript

This guide describes Tapirscan 1.1.0.

Scan image pixels in a browser or Node with the same Rust/WASM core.
[Try the live demo](https://tapirscan.netlify.app) · [Quick start](#quick-start) · [WASM loading](#wasm-loading) · [Functions](#functions) · [All options](#all-options) · [Results](#results)

## Quick start

```sh
npm install tapirscan
```

TypeScript declarations and WASM binaries are included in the
[npm package](https://www.npmjs.com/package/tapirscan). Your runtime must support
WebAssembly SIMD.

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

`formats: "retail"` selects EAN13, UPCA,
EAN8 and UPCE. `"common1D"` adds Code128, Code39 and ITF; `"common"` adds
QRCode and DataMatrix to `"common1D"`. See
[format presets and runtime behavior](../../docs/FORMATS.md).

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

Then point the scanner at that directory:

```js
const scanner = await Scanner.create({ wasmBaseUrl: "/tapirscan/" });
try {
  console.log(scanner.scan(image).values);
} finally {
  scanner.dispose();
}
```

The one-shot helper accepts the same option:
`await scan(image, { wasmBaseUrl: "/tapirscan/" })`.
`wasmBaseUrl` accepts a string or URL, with or without a trailing slash. Relative
URLs resolve against the page/worker URL in browsers and the package module in
Node; use an absolute URL for an unambiguous location. For authenticated requests
or custom storage, use `loadWasm: async (url) => arrayBuffer`. The callback receives
URLs resolved against `wasmBaseUrl` when both options are supplied. Only engines needed by the selected formats are loaded.

Use the deployed base path if your app is hosted below a subpath. Copy all current
WASMs: EAN13/UPCA scanning in Medium/High/Very high also loads the Low recovery decoder. The demo and its
comparison engines are not needed in your app.

## Camera and worker use

A standalone [worker client](examples/worker-client.mjs), [worker](examples/scan-worker.mjs)
and [camera page](examples/camera.html) are included in the package. From this
binding directory (or the installed package directory), run `python3 -m http.server`
and open `/examples/camera.html` on localhost. The example handles initialization,
frame ownership transfer, one frame in flight, errors and shutdown. It transfers
pixel buffers; callers must not reuse the transferred buffer. Worker messages
produce independent mutable result copies through structured cloning. Custom
`loadWasm` functions must be configured inside the worker; functions cannot be sent
in a message. Adjust the worker import and WASM asset path for your bundler.

Initialization is asynchronous; scanning is synchronous. For a responsive browser
UI, initialize one scanner inside a Web Worker and transfer an owned frame buffer.
Capture the next frame after the previous result arrives. Avoid racing scanner
initialization or modifying pixels during scanning. The [demo worker](../../demo/src/lib/scan.worker.ts)
shows a complete integration. Camera capture belongs to your app and requires
HTTPS (localhost works for development).

## Functions

| Function                            | Return type           | Behavior                                                                                                                  |
| ----------------------------------- | --------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `scan(image, options = {})`         | `Promise<ScanResult>` | One image with automatic scanner creation and disposal, including on failure. Accepts creation and scan options together. |
| `Scanner.create(options = {})`      | `Promise<Scanner>`    | Initialize a reusable scanner. Mode is fixed; formats define defaults and allowed per-call subsets.                       |
| `scanner.scan(image, options = {})` | `ScanResult`          | Synchronously scan pixels. Accepts scan options only.                                                                     |
| `scanner.dispose()`                 | `void`                | Release WASM sessions. Repeated disposal is safe; do not scan after disposal.                                             |

`image` is required for either scan function. All options are optional. Reuse a
scanner for successive frames to avoid repeated initialization; create another
to change effort or enable formats outside its configured selection. A per-call
subset such as `scanner.scan(image, { formats: "EAN13" })` applies only to that
call and does not change the default formats. Previously returned results survive disposal. `scanner.formats` exposes the frozen creation selection.

## All options

| Option           | Where               | Default                | Meaning                                                                                                                                                           |
| ---------------- | ------------------- | ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mode`           | Creation            | `"medium"`             | `"low"`, `"medium"`, `"high"`, `"very-high"`.                                                                                                                     |
| `formats`        | Creation / scan     | `["EAN13"]`            | A single identifier, `"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"`, `"all"`, or a nonempty array. Per-call selections must be subsets of creation formats. |
| `wasmBaseUrl`    | Creation            | Module-relative assets | Directory URL for packaged WASMs. Use this for normal browser hosting.                                                                                            |
| `loadWasm`       | Creation            | Module-relative loader | `(url: URL) => Promise<ArrayBuffer>`. Uses HTTP fetch in browsers and filesystem reads for Node file URLs.                                                        |
| `eanAddOnPolicy` | Creation / one-shot | `"Ignore"`             | `"Ignore"`, `"Read"`, `"Require"`; optional EAN/UPC supplement policy.                                                                                            |
| `debug`          | Scan                | `false`                | Include search evidence under `result.debug`. Decoded polygons are always returned.                                                                               |

Format presets cover supported symbologies. Exports `commonFormats`, `commonLinearFormats`, `linearFormats`, `matrixFormats`
and `retailFormats` let you compose custom selections; `formatBits` provides their
native bit mapping. See [identifiers and coverage](../../docs/FORMATS.md).
The four effort modes tune EAN13/UPCA, Common1D and QR Code; other matrix readers use fixed effort.

Resolution, camera capture, preprocessing rotation, ROI, confidence thresholds,
timeouts and exact work budgets are not public scan options. Demo capture and
resize settings belong to the application.

## Image input

`PixelImage` accepts `ImageData` (or its data/width/height fields) or an explicit
`Image` buffer:

| Field             | Type          | Meaning                                                                                                          |
| ----------------- | ------------- | ---------------------------------------------------------------------------------------------------------------- |
| `data`            | `Uint8Array`  | Decoded pixels. ImageData instead uses `Uint8ClampedArray` and implies tightly packed RGBA.                      |
| `width`, `height` | `number`      | Integer input dimensions, at least 3 pixels each.                                                                |
| `channels`        | `1 \| 3 \| 4` | Grayscale, RGB or RGBA. Alpha is ignored. Required for explicit buffers.                                         |
| `stride`          | `number`      | Bytes between row starts, at least width × channels. Optional; defaults to width × channels. Padding is allowed. |

Input is limited to 32 megapixels and 128 MiB of addressed pixels. Keep the buffer stable during the
call. Convert DOM image elements or encoded images to pixels before scanning.

## Results

| Field                          | Type                                                           | Meaning                                                                                                    |
| ------------------------------ | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `result.values`                | `readonly string[]`                                            | Decoded strings.                                                                                           |
| `result.barcodes`              | `readonly Barcode[]`                                           | Decoded values with format and geometry.                                                                   |
| `result.best`                  | `Barcode \| undefined`                                         | Highest-support read, or undefined when empty.                                                             |
| `result.image`                 | `{ width: number, height: number }`                            | Dimensions of supplied pixels.                                                                             |
| `result.mode`                  | `Mode`                                                         | Selected effort.                                                                                           |
| `result.elapsedMs`             | `number`                                                       | Host scan time in milliseconds; excludes file loading and scanner initialization.                          |
| `result.unfinished`            | `boolean`                                                      | Incomplete work; returned reads may still be useful.                                                       |
| `result.debug`                 | `Diagnostics \| undefined`                                     | Requested diagnostic evidence; absent by default.                                                          |
| `barcode.payloadBytes`         | `readonly number[] \| undefined`                               | Original decoded matrix payload bytes when available; use `Uint8Array.from(...)` for an owned byte buffer. |
| `barcode.text`                 | `string`                                                       | Decoded text.                                                                                              |
| `barcode.format`               | `Format \| "Unknown"`                                          | Symbology identifier.                                                                                      |
| `barcode.polygon`              | `Quad`                                                         | Four `[x, y]` corners in input-image coordinates.                                                          |
| `barcode.rect`                 | `{ left: number, top: number, width: number, height: number }` | Enclosing integer rectangle.                                                                               |
| `barcode.support`              | `number`                                                       | Reader-specific ranking evidence; not confidence or a probability.                                         |
| `barcode.gs1`                  | `boolean \| undefined`                                         | GS1 indicator when supplied by the reader.                                                                 |
| `barcode.readerInitialization` | `boolean \| undefined`                                         | Reader initialization data indicator; never executed.                                                      |
| `barcode.structuredAppend`     | `StructuredAppend \| undefined`                                | Immutable multipart metadata: one-based `index`, `count`, optional `id` and `parity`.                      |
| `barcode.eanAddOn`             | `string \| undefined`                                          | Optional EAN supplement; populated when `eanAddOnPolicy` is `"Read"` or `"Require"`.                       |

Results, including nested geometry and requested diagnostics, are immutable at
runtime and in TypeScript. Use `structuredClone(result)` if you need a mutable
copy. Both result arrays are empty when nothing is decoded. Use `result.best` for
one read, or `undefined` when empty. All decoded instances remain available,
including separate copies of the same value. Coordinates start at the
top left, x rightward and y downward. Geometry is returned, not a cropped bitmap.
Map coordinates back yourself if you resize/rotate before scanning. Support is a
ranking heuristic, not a probability.

The package exports `EanAddOnPolicy`, `ScannerOptions`, `ScanOptions`, `ScanResult`, `Barcode`,
`PixelImage`, `Image`, `Quad`, `Mode`, `Format`, `FormatSelection`, `Diagnostics`,
`StructuredAppend` and `DiagnosticBarcode` types. TypeScript infers results from calls; runtime
checks still validate pixel buffers and dimensions.

## EAN/UPC supplements

Set `eanAddOnPolicy: "Read"` when creating a scanner or calling one-shot `scan()`.
The policy is fixed for that scanner; its default is `"Ignore"`.

| Policy      | Behavior                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| `"Ignore"`  | Decode the main barcode without reading its supplement.                                          |
| `"Read"`    | Try reading the two- or five-digit supplement; keep the main barcode if none is readable.        |
| `"Require"` | Return an EAN/UPC barcode only when its supplement is readable. Other formats remain unaffected. |

`barcode.polygon` and `barcode.rect` describe the main barcode, excluding the
supplement. Supplement geometry is not exposed separately.

The supplement appears separately in `barcode.eanAddOn`; `barcode.text` remains
the main payload. Reading supplements enables additional experimental decoding
work independently of the effort mode. With debug enabled, retail reads rejected
by `"Require"` remain available as undecoded-region evidence.

## Evidence and work limits

Most applications need `barcode.text`, `.format`, `.polygon` and `.rect`.
`barcode.support` exposes the evidence used by `.best`. It is an uncalibrated,
reader-specific ranking heuristic, not a certainty percentage; values are not
comparable confidence across formats or effort modes. Consequently, `.best` means the largest support value,
not the most reliable barcode in a mixed-format image. Select by the format or
payload your application needs when that distinction matters. Checksums and consistency
checks reduce wrong reads but cannot guarantee that every returned decode is correct.

Select EAN13/UPCA, Common1D and QR Code search effort with `mode: "low"` through `"very-high"` at creation.
Other matrix readers use fixed effort. `result.unfinished` is available without debug and
combines reported decoding and localization limits. Returned reads are still usable.
Candidate, retry and parsing caps are reported, including bounded searches that
also returned reads. False does not promise exhaustive scanning. Exact budgets and interruptible timeouts are not public options.

`debug: true` adds attempted search windows, localization proposals, candidate
outcomes and engine traces. It is unnecessary for drawing decoded barcode locations.

### Switching between retail and QR scanning

```js
const scanner = await Scanner.create({ formats: "common" });
try {
  console.log(scanner.formats);
  const retail = scanner.scan(image, { formats: "retail" });
  const qr = scanner.scan(image, { formats: "QRCode" });
} finally {
  scanner.dispose();
}
```

`payloadBytes` is supplied by QR Code, Data Matrix, Aztec, PDF417 and MaxiCode.
It contains decoded data bytes before character-set interpretation, not raw symbol
codewords. Aztec Rune represents its numeric value as decimal ASCII. Other readers
leave it absent. Encoding `.text` as UTF-8 does not reconstruct original bytes.
Unsupported character encodings can still prevent decoding; reader behavior is
unchanged. The frozen number array is directly JSON-compatible.

## Diagnostics and errors

```js
const result = scanner.scan(image, { debug: true });
if (result.debug) {
  console.log(result.debug.regions.proposals, result.debug.regions.searchWindows);
  console.log(result.debug.regions.undecoded);
  console.log(result.debug.scan.barcodes);
}
```

`debug.regions` has a stable shape across creation formats: `proposals` and
`searchWindows` contain evidence or null when unavailable, and `undecoded` contains
unread source-image geometry as immutable `UndecodedRegion` objects (`format` hint
and `polygon`, with no decoded text). Empty arrays mean available evidence with no entries.
EAN evidence remains available when a scanner also enables additional readers.

Diagnostics also retain the raw schema-2 result: `scan` includes support and candidate
evidence, and `localizationLimited` reports localization limits. Depending on the
reader, `localization`, `searchWindows`, `recovery` and `detailRegions` may be
present. GS1, reader initialization and structured append are available directly on
barcodes without debug; raw metadata also retains these fields where supported.
Candidate indices inside recovery crops are local to the crop and are not
identifiers for tracking between frames.

Invalid options can raise TypeError. Scanner validation and engine failures can
raise the exported `ScannerError` with a `.code` and `.message`. Loader/fetch
errors propagate to the caller; creation and the one-shot helper reject their
promises on failure. Always dispose reusable scanners with `finally`.

## Finishing candidate work

Use `scanner.scan(image, { finishCandidates: true })` or
`await scan(image, { finishCandidates: true })` to let all selected EAN13/UPC-A
candidates use their effort budget, without the shared frame retry and association
budgets stopping later candidates. The default is `false`; at least one of
`EAN13` or `UPCA` must be selected. This is a per-scan option.

Crowded or difficult images can take longer. Per-candidate effort, intentional
weak-candidate deferral, localization, sampling and result limits still apply.
Other formats keep their existing budgets. The synchronous scan has no library
wall-clock deadline; use a Worker when responsiveness matters.
`result.unfinished` can remain true, so this is not an exhaustiveness guarantee.
Custom WASM engines must advertise support; unsupported engines fail clearly.

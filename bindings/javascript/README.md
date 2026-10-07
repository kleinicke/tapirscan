# Tapirscan for JavaScript and TypeScript

Scan barcodes in the browser or Node with the same Rust/WASM core. Results contain
every decoded barcode with its `text`, `format` and source-image `polygon` / `rect`.
No detection returns `[]`.

- **`tapirscan/browser`** for web apps: scan files, `<img>`, `<video>`, canvases
  or bitmaps in a bundled worker, with no WASM setup.
- **`tapirscan`** (the core) for Node and decoded pixels, scanning synchronously
  on the calling thread.

[Try the live demo](https://tapirscan.f-kleinicke.de) · [Browser apps](#browser-apps-react-and-svelte) · [Core API](#core-api) · [WASM loading](#wasm-loading) · [All options](#all-options) · [Results](#results)

```sh
npm install tapirscan
```

TypeScript declarations and WASM binaries are included in the
[npm package](https://www.npmjs.com/package/tapirscan). Your runtime must support
WebAssembly SIMD.

## Browser apps, React and Svelte

```js
import { scan } from "tapirscan/browser";

const result = await scan(file); // a File from <input type="file">
console.log(result.values); // e.g. ["4006381333931"]
```

Reuse a scanner for several images or camera frames. Construction is synchronous:
the worker and WASM load in the background, so the first scan simply waits for them.

```js
import { Scanner } from "tapirscan/browser";

const scanner = new Scanner({ formats: ["EAN13", "QRCode"] });
const result = await scanner.scan(image); // a file, <img>, canvas, video frame, ...
scanner.dispose(); // stops the worker
```

**Camera:** scan a playing `<video>` in a loop. Waiting for the next frame scans
each frame at most once, and awaiting each scan means slow devices skip frames
instead of falling behind:

```js
while (running) {
  await new Promise((resolve) => video.requestVideoFrameCallback(resolve));
  if (!running) break;
  const result = await scanner.scan(video);
  // ...show result.values and result.barcodes
}
```

`dispose()` rejects the scan in progress with "Scanner was disposed". In React
(add `"use client";` at the top in Next.js):

```jsx
import { useEffect, useRef, useState } from "react";
import { Scanner } from "tapirscan/browser";

export default function BarcodeScanner() {
  const video = useRef(null);
  const [barcodes, setBarcodes] = useState([]);

  useEffect(() => {
    const element = video.current;
    const scanner = new Scanner();
    const camera = navigator.mediaDevices.getUserMedia({ video: { facingMode: "environment" } });
    let running = true; // React StrictMode mounts twice in development
    camera
      .then(async (stream) => {
        if (!running) return;
        element.srcObject = stream;
        while (running) {
          await new Promise((resolve) => element.requestVideoFrameCallback(resolve));
          if (!running) break;
          const found = await scanner.scan(element);
          if (running) setBarcodes(found.barcodes);
        }
      })
      .catch((error) => running && console.error(error));
    return () => {
      running = false;
      scanner.dispose();
      camera.then(
        (stream) => stream.getTracks().forEach((track) => track.stop()),
        () => {}, // Camera errors are handled above.
      );
    };
  }, []);

  return (
    <>
      <video ref={video} autoPlay muted playsInline />
      {barcodes.map((barcode, i) => (
        <p key={i}>
          {barcode.format}: {barcode.text}
        </p>
      ))}
    </>
  );
}
```

In Svelte 5 and SvelteKit:

```svelte
<script>
  import { Scanner } from "tapirscan/browser";
  import { onDestroy } from "svelte";

  const scanner = new Scanner();
  onDestroy(() => scanner.dispose());

  let barcodes = $state.raw([]);
  async function onchange(event) {
    const file = event.currentTarget.files?.[0];
    if (file) barcodes = (await scanner.scan(file)).barcodes;
  }
</script>

<input type="file" accept="image/*" {onchange} />
{#each barcodes as barcode}<p>{barcode.format}: {barcode.text}</p>{/each}
```

**Bundlers:** Next.js (Turbopack or webpack), Vite 8 and production builds bundle
the worker and WASM files with no configuration. The Vite 6 and 7 development
servers (including SvelteKit on them) need one line, or the scanner reports that
its worker failed to start:

```js
// vite.config.js
export default defineConfig({
  // ...your plugins
  optimizeDeps: { exclude: ["tapirscan"] },
});
```

- **Sources:** a `File` or `Blob` (any image the browser decodes), `<img>`,
  `<video>` (its current frame), `<canvas>`, `OffscreenCanvas`, `ImageBitmap`,
  `VideoFrame`, `ImageData`, or decoded pixels as in the core API. Inputs are not
  modified or transferred.
- **Methods:** `scan` and `inspect` match the core API but return promises.
  Concurrent calls are queued. For camera loops, await each scan before the next,
  as in the [camera example](examples/camera.html).
- **Options:** `mode`, `formats` and `eanAddOnPolicy` work as in the core;
  `wasmBaseUrl` serves the WASM files from another directory. `extendedBudget` and
  per-call `formats` are scan options. `loadWasm` and Turbo presets need the core.
- **Lifecycle:** `scanner.ready` resolves once loaded, which is optional to await;
  loading errors also reject every scan. During server rendering the constructor
  does nothing, and scans reject. `dispose()` stops the worker and rejects queued
  scans; results stay valid.
- **Results and errors:** the same deeply frozen results as the core. Invalid
  constructor options can throw; scan validation rejects with `TypeError`. Engine
  failures use `ScannerError`; browser image decoding and loading can also fail.
- **Requirements:** module workers, `OffscreenCanvas` and WebAssembly SIMD:
  Chrome 91, Firefox 114, Safari 16.4 or later. The camera examples additionally
  require `requestVideoFrameCallback`. Images may have at most 32 megapixels.

`best(barcodes)` and the result types are exported from `tapirscan/browser` too.

## Core API

```js
import { scan } from "tapirscan";
const result = await scan(image);
for (const barcode of result.barcodes) console.log(barcode.text, barcode.polygon);
```

Use `scanner.scan(image)` when reusing a scanner. For unread regions, work status,
timing and engine evidence, explicitly call `inspect(image)` or `scanner.inspect(image)`.
There is no `debug` option. The core scans decoded pixels; pass a canvas's
`ImageData` directly:

```js
import { scan } from "tapirscan";

// Using an existing canvas and its 2D context:
const image = context.getImageData(0, 0, canvas.width, canvas.height);
const result = await scan(image);
console.log(result.values); // e.g. ["4006381333931"]
```

Defaults are Medium effort, retail formats, multiple results, and diagnostics available through inspection.
Each barcode contains text, format, polygon and rectangle.
The helper creates and disposes a scanner automatically. In browsers, the core
needs its [WASM assets](#wasm-loading) served; Node loads the packaged files automatically.

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
formats; see [format coverage](../../docs/FORMATS.md) for supported variants. Settings also work with the helper:
`await scan(image, { mode: "high", formats: "1D" })`.

`formats: "retail"` selects EAN13, UPCA,
EAN8 and UPCE. `"common1D"` adds Code128, Code39 and ITF; `"common"` adds
QRCode and DataMatrix to `"common1D"`. See
[format presets and runtime behavior](../../docs/FORMATS.md).

## Experimental Turbo presets

For faster **1D barcode scanning**, opt into a Turbo preset:

```js
import { Scanner, inspect } from "tapirscan";

const scanner = await Scanner.create({ experimentalTurbo: 4, formats: "retail" });
try {
  const result = scanner.inspect(image);
  console.log(result.values, result.experimentalTurbo); // Selected preset: 4
} finally {
  scanner.dispose();
}

// The one-shot helper accepts the same selection:
const result = await inspect(image, { experimentalTurbo: 8 });
```

Accepted values are **2, 4, 8 and 16**. These identify presets with historical
speed targets, not guaranteed speed multipliers or fractions of a time budget.
Higher presets generally do less work and can miss more barcodes, including clean
symbols placed close together. Start with 2 and check detection on your own inputs.
The acceleration targets linear formats, including all four Retail formats and
Code128, Code39 and ITF. **These presets do not provide corresponding 2D speedups.**
QR and other matrix formats remain usable when selected, but changing the Turbo
number does not select a faster matrix-decoding tier. Mixed-format scanning still
pays for the enabled 2D readers. See [Turbo behavior and limitations](../../docs/EXPERIMENTAL_TURBO.md).

Choose either `mode` or `experimentalTurbo`; supplying both is an error. Omitting
both keeps the normal Medium default. The Turbo selection is fixed at creation;
`scanner.experimentalTurbo` and `result.experimentalTurbo` report it. The underlying
`mode` is `"low"`. `scan` returns the usual lightweight result; `inspect` also reports
the preset, diagnostics and `unfinished`. Stable-mode inspection omits
`experimentalTurbo`. Fast linear inspection reports unfinished work; this does not invalidate decoded values.

Turbo requires the default `eanAddOnPolicy: "Ignore"` and rejects
`extendedBudget: true`, because those options would bypass the fast linear path.
Per-call format subsets and inspection work normally.

**API stability:** this option, its accepted presets and associated experimental
asset imports may change or be removed in a minor release; patch releases retain
API compatibility. Changes are documented in release notes. Pin the exact package
version when relying on this experimental interface. Stable effort modes and
shared result fields retain their normal compatibility policy.

For Vite/SvelteKit, use the same loader pattern below, with the matching asset:

```js
import turboWasmUrl from "tapirscan/wasm/experimental-turbo4.wasm?url";

const response = await fetch(turboWasmUrl);
if (!response.ok) throw new Error(`WASM load failed: ${response.status}`);
const bytes = await response.arrayBuffer();
const scanner = await Scanner.create({
  experimentalTurbo: 4,
  loadWasm: async () => bytes,
});
// Reuse scanner for frames; call scanner.dispose() on teardown.
```

Exports exist for `experimental-turbo2.wasm`, `experimental-turbo4.wasm`,
`experimental-turbo8.wasm` and `experimental-turbo16.wasm` under `tapirscan/wasm/`.
Only the selected asset is loaded. A mismatched stable mode or Turbo asset is an error.

## WASM loading

In Node, the default loader reads assets from the installed package. Decode your
image with an image library first, then pass grayscale, RGB or RGBA bytes:

```js
import { scan } from "tapirscan";

const result = await scan({ data: pixels, width, height, channels: 1, stride: width });
```

Here `pixels` is a Uint8Array of decoded grayscale pixels. Image codecs are not
included; filenames, URLs and encoded JPEG/PNG bytes are not scan inputs.

### Vite and SvelteKit

[`tapirscan/browser`](#browser-apps-react-and-svelte) needs no WASM setup. To use the
core directly, import a stable WASM asset URL so Vite includes it in development
and production builds, including apps deployed under a base path:

```js
import { Scanner } from "tapirscan";
import mediumWasmUrl from "tapirscan/wasm/medium.wasm?url";

// Load once; reuse these bytes if you create more than one scanner.
const response = await fetch(mediumWasmUrl);
if (!response.ok) throw new Error(`WASM load failed: ${response.status}`);
const bytes = await response.arrayBuffer();
const scanner = await Scanner.create({ loadWasm: async () => bytes });
try {
  console.log(scanner.scan(image).values);
} finally {
  scanner.dispose();
}
```

The stable imports are `tapirscan/wasm/low.wasm`, `medium.wasm`, `high.wasm`
and `very-high.wasm` (all under `tapirscan/wasm/`). Match the imported asset to
`mode`; the example uses the default Medium. The package maps these imports to
its current immutable binaries, so upgrades need no filename changes or manual
copies. `?url` is Vite syntax, not part of the export name. In SvelteKit, create
core scanners in browser code such as `onMount`, not during server rendering.

### Other browser setups

The default loader fetches assets relative to the module. If your bundler
relocates modules, serve the WASMs from your public directory. Run the copy as
part of your build so package upgrades cannot leave stale assets:

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
URLs resolved against `wasmBaseUrl` when both options are supplied. Creation loads
one complete Rust scanner for the selected effort mode.

The default loader shares pending and completed loads by resolved URL within each
JavaScript module instance, retaining at most four assets. Failed loads are removed
so they can be retried. Each scanner still owns its own WASM instance and memory.
Custom `loadWasm` callbacks are called for every creation and manage their own cache.
Workers have separate caches. Reuse a scanner to avoid repeated initialization.

Use the deployed base path if your app is hosted below a subpath. Copy all current
WASMs so every effort mode remains available. The demo and its comparison engines
are not needed in your app.

## Camera and worker use

The core scans synchronously, so a long scan on the main thread blocks the page.
[`tapirscan/browser`](#browser-apps-react-and-svelte) runs the core in a worker and
handles initialization, frame capture, queueing and shutdown. The
[camera example](examples/camera.html) scans a live camera with it: from this
directory (or the installed package), run `python3 -m http.server` and open
`/examples/camera.html` on localhost. Camera capture requires HTTPS; localhost
works for development.

To manage your own worker instead, create one core scanner inside it and transfer
owned frame buffers; `loadWasm` functions must be configured inside the worker.

## Functions

| Function                               | Return type                 | Behavior                                                                                                                  |
| -------------------------------------- | --------------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `scan(image, options = {})`            | `Promise<ScanResult>`       | One image with automatic creation and disposal. Accepts creation and scan options together.                               |
| `scanner.scan(image, options = {})`    | `ScanResult`                | Synchronously scan pixels with a reusable core scanner.                                                                   |
| `inspect(image, options = {})`         | `Promise<InspectionResult>` | One image with automatic scanner creation and disposal, including on failure. Accepts creation and scan options together. |
| `Scanner.create(options = {})`         | `Promise<Scanner>`          | Initialize a reusable scanner. Mode is fixed; formats define defaults and allowed per-call subsets.                       |
| `scanner.inspect(image, options = {})` | `InspectionResult`          | Synchronously scan pixels. Accepts scan options only.                                                                     |
| `scanner.dispose()`                    | `void`                      | Release WASM sessions. Repeated disposal is safe; do not scan after disposal.                                             |

`image` is required for either scan function. All options are optional. Reuse a
scanner for successive frames to avoid repeated initialization; create another
to change effort or enable formats outside its configured selection. A per-call
subset such as `scanner.inspect(image, { formats: "EAN13" })` applies only to that
call and does not change the default formats. Previously returned results survive disposal. `scanner.formats` exposes the frozen creation selection.

## All options

| Option              | Where               | Default                | Meaning                                                                                                                                                           |
| ------------------- | ------------------- | ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `experimentalTurbo` | Creation / one-shot | Unset                  | **Experimental:** `2`, `4`, `8` or `16`; targets faster 1D scanning, not 2D speedups. Mutually exclusive with `mode`.                                             |
| `mode`              | Creation            | `"medium"`             | `"low"`, `"medium"`, `"high"`, `"very-high"`.                                                                                                                     |
| `formats`           | Creation / scan     | `"retail"`             | A single identifier, `"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"`, `"all"`, or a nonempty array. Per-call selections must be subsets of creation formats. |
| `wasmBaseUrl`       | Creation            | Module-relative assets | Directory URL for packaged WASMs. Use this for normal browser hosting.                                                                                            |
| `loadWasm`          | Creation            | Module-relative loader | `(url: URL) => Promise<ArrayBuffer>`. Uses HTTP fetch in browsers and filesystem reads for Node file URLs.                                                        |
| `eanAddOnPolicy`    | Creation / one-shot | `"Ignore"`             | `"Ignore"`, `"Read"`, `"Require"`; optional EAN/UPC supplement policy.                                                                                            |
| `extendedBudget`    | Scan / one-shot     | `false`                | Allow extra reader work for any format. Exact budgets may evolve.                                                                                                 |

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
call. Convert DOM image elements or encoded images to pixels before scanning, or
use [`tapirscan/browser`](#browser-apps-react-and-svelte), which accepts them directly.

## Results

`scan` returns a read-only `ScanResult` with `values`, `barcodes` and `best`.
`inspect` returns an `InspectionResult` with those same fields plus image size,
mode, timing, work status, undecoded proposals and diagnostics. The barcode
fields below are available from both operations.

| Field                          | Type                                                           | Meaning                                                                                                    |
| ------------------------------ | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `result.values`                | `readonly string[]`                                            | Decoded strings.                                                                                           |
| `result.barcodes`              | `readonly Barcode[]`                                           | Decoded values with format and geometry.                                                                   |
| `result.best`                  | `Barcode \| undefined`                                         | Highest-support read, or undefined when empty.                                                             |
| `result.image`                 | `{ width: number, height: number }`                            | Dimensions of supplied pixels.                                                                             |
| `result.mode`                  | `Mode`                                                         | Selected effort.                                                                                           |
| `result.elapsedMs`             | `number`                                                       | Host scan time in milliseconds; excludes file loading and scanner initialization.                          |
| `result.unfinished`            | `boolean`                                                      | Incomplete work; returned reads may still be useful.                                                       |
| `result.undecoded`             | `readonly UndecodedRegion[]`                                   | Localized proposals without accepted decodes; always available.                                            |
| `result.diagnostics`           | `Diagnostics`                                                  | Engine evidence from inspection.                                                                           |
| `barcode.payloadBytes`         | `readonly number[] \| undefined`                               | Original decoded matrix payload bytes when available; use `Uint8Array.from(...)` for an owned byte buffer. |
| `barcode.text`                 | `string`                                                       | Decoded text.                                                                                              |
| `barcode.format`               | `Format`                                                       | Symbology identifier.                                                                                      |
| `barcode.polygon`              | `Quad`                                                         | Four `[x, y]` corners in input-image coordinates.                                                          |
| `barcode.rect`                 | `{ left: number, top: number, width: number, height: number }` | Enclosing integer rectangle.                                                                               |
| `barcode.support`              | `number`                                                       | Reader-specific ranking evidence; not confidence or a probability.                                         |
| `barcode.gs1`                  | `boolean \| undefined`                                         | GS1 indicator when supplied by the reader.                                                                 |
| `barcode.readerInitialization` | `boolean \| undefined`                                         | Reader initialization data indicator; never executed.                                                      |
| `barcode.structuredAppend`     | `StructuredAppend \| undefined`                                | Immutable multipart metadata: one-based `index`, `count`, optional `id` and `parity`.                      |
| `barcode.eanAddOn`             | `string \| undefined`                                          | Optional EAN supplement; populated when `eanAddOnPolicy` is `"Read"` or `"Require"`.                       |

Results, including nested geometry and requested diagnostics, are immutable at
runtime and in TypeScript. Use `structuredClone(result)` if you need a mutable
copy. `barcodes` and `values` are empty when nothing is decoded; `undecoded` may still
contain proposals. Use `result.best` for one read, keeping the first on ties, or `undefined` when empty. All decoded instances remain available,
including separate copies of the same value. Coordinates start at the
top left, x rightward and y downward. Geometry is returned, not a cropped bitmap.
Map coordinates back yourself if you resize/rotate before scanning. Support is a
ranking heuristic, not a probability.

The package exports `EanAddOnPolicy`, `ScannerOptions`, `ScanOptions`, `InspectionResult`, `Barcode`,
`PixelImage`, `Image`, `Quad`, `Mode`, `Format`, `FormatSelection`, `Diagnostics`,
`StructuredAppend` and `DiagnosticBarcode` types. TypeScript infers results from calls; runtime
checks still validate pixel buffers and dimensions.

## EAN/UPC supplements

Set `eanAddOnPolicy: "Read"` when creating a scanner or calling one-shot `scan` or `inspect`.
The policy is fixed for that scanner; its default is `"Ignore"`.

| Policy      | Behavior                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| `"Ignore"`  | Decode the main barcode without reading its supplement.                                          |
| `"Read"`    | Try reading the two- or five-digit supplement; keep the main barcode if none is readable.        |
| `"Require"` | Return an EAN/UPC barcode only when its supplement is readable. Other formats remain unaffected. |

`barcode.polygon` and `barcode.rect` describe the main barcode, excluding the
supplement. Supplement geometry is not exposed separately.

The supplement appears separately in `barcode.eanAddOn`; `barcode.text` remains
the main payload. Reading supplements enables additional decoding
work independently of the effort mode. Retail reads rejected
by `"Require"` remain available in `result.undecoded`.

## Evidence and work limits

Most applications need `barcode.text`, `.format`, `.polygon` and `.rect`.
`barcode.support` exposes the evidence used by `.best`. It is an uncalibrated,
reader-specific ranking heuristic, not a certainty percentage; values are not
comparable confidence across formats or effort modes. Consequently, `.best` means the largest support value,
not the most reliable barcode in a mixed-format image. Select by the format or
payload your application needs when that distinction matters. Checksums and consistency
checks reduce wrong reads but cannot guarantee that every returned decode is correct.

Select EAN13/UPCA, Common1D and QR Code search effort with `mode: "low"` through `"very-high"` at creation.
Other matrix readers use fixed effort. `result.unfinished` is available in inspection reports and
combines reported decoding and localization limits. Returned reads are still usable.
Candidate, retry and parsing caps are reported, including bounded searches that
also returned reads. False does not promise exhaustive scanning. Exact budgets and interruptible timeouts are not public options.

Inspection includes attempted search windows, localization proposals, candidate
outcomes and engine traces. It is unnecessary for drawing decoded barcode locations.

### Switching between retail and QR scanning

```js
const scanner = await Scanner.create({ formats: "common" });
try {
  console.log(scanner.formats);
  const retail = scanner.inspect(image, { formats: "retail" });
  const qr = scanner.inspect(image, { formats: "QRCode" });
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
const result = scanner.inspect(image, {});
if (result.diagnostics) {
  console.log(result.diagnostics.regions.proposals, result.diagnostics.regions.searchWindows);
  console.log(result.diagnostics.regions.undecoded);
  console.log(result.diagnostics.scan.barcodes);
}
```

`diagnostics.regions` has a stable shape across creation formats: `proposals` and
`searchWindows` contain evidence or null when unavailable, and `undecoded` contains
unread source-image geometry as immutable `UndecodedRegion` objects (`format` hint
and `polygon`, with no decoded text). Empty arrays mean available evidence with no entries.
EAN evidence remains available when a scanner also enables additional readers.

Diagnostics also retain the raw schema-2 result: `scan` includes support and candidate
evidence, and `localizationLimited` reports localization limits. Depending on the
reader, `localization`, `searchWindows`, `recovery` and `detailRegions` may be
present. GS1, reader initialization and structured append are available directly on
barcodes from either operation; raw metadata also retains these fields where supported.
Candidate indices inside recovery crops are local to the crop and are not
identifiers for tracking between frames.

Invalid options can raise TypeError. Scanner validation and engine failures can
raise the exported `ScannerError` with a `.code` and `.message`. Loader/fetch
errors propagate to the caller; creation and the one-shot helper reject their
promises on failure. Always dispose reusable scanners with `finally`.

## Extended work budget

Use `scanner.inspect(image, { extendedBudget: true })` to allow additional reader work. The default
is false. This option is valid for every format; the exact budgets and stages are
implementation details that may evolve. Effort mode remains a separate setting.

Today this relaxes shared EAN-13/UPC-A retry and association limits. Other readers
currently retain their existing budgets. Per-candidate limits and intentional
deferrals remain; `unfinished` can still be true. This is not unlimited search,
an exhaustiveness guarantee or a wall-clock deadline. Custom primary-reader
engines must support the extended-work capability or report an error.

## Undecoded regions

`result.undecoded` is always available, in inspection reports. Each entry has
a source-image polygon and a format hint. It is a localized proposal without an
accepted decode, not proof of a real or permanently unreadable barcode. Entries
can overlap or describe false candidates. An empty collection does not prove
that every barcode was found. Raw candidate attempts remain in inspection diagnostics.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

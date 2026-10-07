# Tapirscan for JavaScript and TypeScript

Scan barcodes in the browser or Node with a Rust/WASM core. Each scan returns
every decoded barcode with its `text`, `format` and source-image `polygon` / `rect`.

- **`tapirscan/browser`** for web apps: scan files, `<img>`, `<video>`, canvases
  or bitmaps in a bundled worker, with no WASM setup.
- **`tapirscan`** (the core) for Node and decoded pixels, scanning synchronously
  on the calling thread.

[Try the live demo](https://tapirscan.f-kleinicke.de) · [Browser apps](#browser-apps-react-and-svelte) · [Core API](#core-api) · [WASM loading](#wasm-loading) · [All options](#all-options) · [Results](#results)

```sh
npm install tapirscan
```

TypeScript declarations and WASM binaries are included. Your runtime must support
WebAssembly SIMD; Node 20 or later is required.

## Browser apps, React and Svelte

```js
import { scan } from "tapirscan/browser";

const result = await scan(file); // a File from <input type="file">
console.log(result.values); // e.g. ["4006381333931"]
```

Reuse a scanner for several images or camera frames. Construction is synchronous:
the worker and WASM load in the background, and the first scan waits for them.

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
- **Methods:** `scan` and `inspect` work as in the core API but return promises.
  Concurrent calls are queued.
- **Options:** `mode`, `formats` and `eanAddOnPolicy` work as in the core, and
  `scan(image, { formats })` overrides the formats for one call. `wasmBaseUrl`
  serves the WASM files from another directory. `loadWasm` and Turbo presets need
  the core.
- **Lifecycle:** `scanner.ready` resolves once loaded; awaiting it is optional,
  because loading errors also reject every scan. During server rendering the
  constructor does nothing and scans reject. `dispose()` stops the worker and
  rejects queued scans; returned results stay valid.
- **Errors:** unknown option names throw in the constructor; invalid option values
  and scan arguments reject with `TypeError`; engine failures reject with
  `ScannerError`. Browser image decoding and loading can also fail.
- **Requirements:** module workers, `OffscreenCanvas` and WebAssembly SIMD:
  Chrome 91, Firefox 114, Safari 16.4 or later. The camera loop also needs
  `requestVideoFrameCallback`. Images may have at most 32 megapixels.

The [camera example](examples/camera.html) is a complete page: from this
directory (or the installed package), run `python3 -m http.server` and open
`/examples/camera.html` on localhost. Camera capture requires HTTPS; localhost
works for development.

## Core API

The core scans decoded pixels, for example a canvas's `ImageData`:

```js
import { scan } from "tapirscan";

const image = context.getImageData(0, 0, canvas.width, canvas.height);
const result = await scan(image);
console.log(result.values); // e.g. ["4006381333931"]
```

The one-shot helper creates and disposes a scanner. Defaults are Medium effort
and retail formats. In browsers the core needs its [WASM assets](#wasm-loading)
served; Node loads the packaged files automatically.

Reuse a scanner for repeated images. Its scans are synchronous:

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

Settings also work with the helper: `await scan(image, { mode: "high", formats: "1D" })`.
For unread regions, timing and diagnostics, call `inspect` instead of `scan`.

The core scans synchronously, so a long scan on the main thread blocks the page.
Use `tapirscan/browser`, or create one core scanner inside your own worker and
transfer frame buffers to it; configure `loadWasm` inside that worker.

| Function                              | Returns                     | Behavior                                                                     |
| ------------------------------------- | --------------------------- | ---------------------------------------------------------------------------- |
| `scan(image, options = {})`           | `Promise<ScanResult>`       | One image; creates and disposes a scanner.                                   |
| `inspect(image, options = {})`        | `Promise<InspectionResult>` | Like `scan`, with timing, unread regions and diagnostics.                    |
| `Scanner.create(options = {})`        | `Promise<Scanner>`          | A reusable scanner with fixed mode, default formats and supplement policy.   |
| `scanner.scan(image, { formats })`    | `ScanResult`                | Synchronous scan; `formats` optionally overrides the defaults for this call. |
| `scanner.inspect(image, { formats })` | `InspectionResult`          | Synchronous inspection.                                                      |
| `scanner.dispose()`                   | `void`                      | Releases WASM memory. Repeated disposal is safe; do not scan after disposal. |

Create another scanner to change effort or the supplement policy. Per-call
formats such as `scanner.scan(image, { formats: "QRCode" })` apply only to that
call; `scanner.formats` exposes the defaults. Results stay valid after disposal.

## Experimental Turbo presets

For faster **1D barcode scanning**, opt into a Turbo preset instead of a mode:

```js
import { Scanner } from "tapirscan";

const scanner = await Scanner.create({ experimentalTurbo: 2, formats: "retail" });
try {
  console.log(scanner.scan(image).values);
} finally {
  scanner.dispose();
}
```

Accepted values are **2, 4, 8 and 16**; they name presets, not speed multipliers.
Higher presets do less work and can miss more barcodes, including clean symbols
placed close together. Start with 2 and check detection on your own inputs.
The presets speed up EAN-13, UPC-A, EAN-8, UPC-E, Code 128, Code 39 and ITF.
Other formats stay readable when selected, but do not get faster, and mixed-format
scans still pay for the enabled 2D readers. See
[Turbo behavior and limitations](../../docs/EXPERIMENTAL_TURBO.md).

`experimentalTurbo` and `mode` are mutually exclusive. `scanner.experimentalTurbo`
and `inspect` results report the preset; `scanner.mode` and `result.mode` report
`"low"`. Turbo requires `eanAddOnPolicy: "ignore"`.

**Stability:** this option, its presets and their asset imports may change or be
removed in a minor release. Pin the exact package version if you rely on them.

For Vite/SvelteKit, load the matching asset as in [WASM loading](#vite-and-sveltekit):
`tapirscan/wasm/experimental-turbo2.wasm`, `-turbo4`, `-turbo8` or `-turbo16`.

## WASM loading

In Node, the default loader reads assets from the installed package. Decode your
image with an image library first, then pass grayscale, RGB or RGBA bytes:

```js
import { scan } from "tapirscan";

const result = await scan({ data: pixels, width, height, channels: 1, stride: width });
```

Here `pixels` is a `Uint8Array` of decoded grayscale pixels. Filenames, URLs and
encoded JPEG/PNG bytes are not scan inputs.

### Vite and SvelteKit

[`tapirscan/browser`](#browser-apps-react-and-svelte) needs no WASM setup. To use the
core directly, import the WASM asset URL so Vite includes it in development and
production builds, including apps deployed under a base path:

```js
import { Scanner } from "tapirscan";
import mediumWasmUrl from "tapirscan/wasm/medium.wasm?url";

// Load once; reuse these bytes if you create more than one scanner.
const response = await fetch(mediumWasmUrl);
if (!response.ok) throw new Error(`WASM load failed: ${response.status}`);
const bytes = await response.arrayBuffer();
const scanner = await Scanner.create({ loadWasm: async () => bytes });
```

The imports are `tapirscan/wasm/low.wasm`, `medium.wasm`, `high.wasm` and
`very-high.wasm`; match the asset to `mode` (the example uses the default
Medium). A mismatched asset is an error. `?url` is Vite syntax. In SvelteKit,
create core scanners in browser code such as `onMount`, not during server rendering.

### Other browser setups

The default loader fetches assets relative to the module. If your bundler
relocates modules, copy the WASM files to your public directory as part of your
build, so package upgrades cannot leave stale assets:

```sh
mkdir -p public/tapirscan
cp node_modules/tapirscan/wasm/*.wasm public/tapirscan/
```

Then point the scanner at that directory:

```js
const scanner = await Scanner.create({ wasmBaseUrl: "/tapirscan/" });
```

`wasmBaseUrl` accepts a string or URL, with or without a trailing slash. Relative
URLs resolve against the page or worker in browsers and the package module in
Node. For authenticated requests or custom storage, use
`loadWasm: async (url) => arrayBuffer`; it receives URLs resolved against
`wasmBaseUrl`. The default loader caches loaded assets within a module instance;
custom loaders manage their own caching. Each scanner owns its own WASM instance.

## All options

| Option              | Where           | Default                | Meaning                                                                                                                                    |
| ------------------- | --------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `mode`              | Creation        | `"medium"`             | `"low"`, `"medium"`, `"high"`, `"very-high"`.                                                                                              |
| `formats`           | Creation / scan | `"retail"`             | An identifier, `"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"`, `"all"`, or a nonempty array. Per-call formats override the defaults. |
| `eanAddOnPolicy`    | Creation        | `"ignore"`             | `"ignore"`, `"read"`, `"require"`; see [supplements](#eanupc-supplements).                                                                 |
| `wasmBaseUrl`       | Creation        | Module-relative assets | Directory URL for the packaged WASM files.                                                                                                 |
| `loadWasm`          | Creation        | Module-relative loader | `(url: URL) => Promise<ArrayBuffer>`.                                                                                                      |
| `experimentalTurbo` | Creation        | Unset                  | **Experimental:** `2`, `4`, `8` or `16`. Mutually exclusive with `mode`.                                                                   |

`"retail"` selects EAN13, UPCA, EAN8 and UPCE. `"common1D"` adds Code128,
Code39 and ITF; `"common"` adds QRCode and DataMatrix. `"1D"` and `"2D"`
enable all linear or all 2D formats.
The exports `retailFormats`, `commonLinearFormats`, `commonFormats`,
`linearFormats` and `matrixFormats` help compose custom selections. See
[format coverage](../../docs/FORMATS.md) for identifiers and variants.

## Image input

The core accepts `ImageData` (or an object with its `data`, `width` and `height`)
or an explicit pixel buffer:

| Field             | Type          | Meaning                                                                              |
| ----------------- | ------------- | ------------------------------------------------------------------------------------ |
| `data`            | `Uint8Array`  | Decoded pixels. `ImageData` uses `Uint8ClampedArray` with tightly packed RGBA.       |
| `width`, `height` | `number`      | Integer dimensions, at least 3 pixels each.                                          |
| `channels`        | `1 \| 3 \| 4` | Grayscale, RGB or RGBA. Alpha is ignored. Required for explicit buffers.             |
| `stride`          | `number`      | Bytes between row starts; defaults to width × channels. Larger values allow padding. |

Input is limited to 32 megapixels and 128 MiB of addressed pixels. Keep the buffer
unchanged during the call. Coordinates start at the top left, x rightward and y
downward. If you resize or rotate before scanning, map coordinates back yourself.

## Results

`scan` returns a `ScanResult`; `inspect` returns an `InspectionResult` with the
same fields plus the ones marked _inspect_.

| Field                          | Type                                                           | Meaning                                                                     |
| ------------------------------ | -------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `result.values`                | `readonly string[]`                                            | Decoded strings, one per barcode.                                           |
| `result.barcodes`              | `readonly Barcode[]`                                           | Decoded barcodes with format and geometry.                                  |
| `result.best`                  | `Barcode \| undefined`                                         | Highest-support barcode, first on ties.                                     |
| `result.image`                 | `{ width, height }`                                            | _inspect_: dimensions of the supplied pixels.                               |
| `result.mode`                  | `Mode`                                                         | _inspect_: effort mode used.                                                |
| `result.elapsedMs`             | `number`                                                       | _inspect_: scan time, excluding image loading and scanner creation.         |
| `result.undecoded`             | `readonly UndecodedRegion[]`                                   | _inspect_: [regions without a decode](#undecoded-regions).                  |
| `result.diagnostics`           | `Diagnostics`                                                  | _inspect_: [engine evidence](#diagnostics).                                 |
| `barcode.text`                 | `string`                                                       | Decoded text.                                                               |
| `barcode.format`               | `Format`                                                       | Symbology identifier.                                                       |
| `barcode.polygon`              | `Quad`                                                         | Four `[x, y]` corners in input-image coordinates.                           |
| `barcode.rect`                 | `{ left: number, top: number, width: number, height: number }` | Enclosing integer rectangle.                                                |
| `barcode.support`              | `number`                                                       | Reader-specific ranking evidence used by `best`.                            |
| `barcode.payloadBytes`         | `readonly number[] \| undefined`                               | Decoded matrix payload bytes, when available.                               |
| `barcode.gs1`                  | `boolean \| undefined`                                         | GS1 indicator, when supplied by the reader.                                 |
| `barcode.readerInitialization` | `boolean \| undefined`                                         | Reader initialization indicator; never executed.                            |
| `barcode.structuredAppend`     | `StructuredAppend \| undefined`                                | Multipart metadata: one-based `index`, `count`, optional `id` and `parity`. |
| `barcode.eanAddOn`             | `string \| undefined`                                          | EAN/UPC supplement, with `eanAddOnPolicy` `"read"` or `"require"`.          |

Separate labels with the same value stay separate entries. Results are deeply
frozen; use `structuredClone(result)` for a mutable copy. `JSON.stringify(result)`
gives plain JSON.

`support` is an uncalibrated, reader-specific ranking heuristic, not a
probability, and is not comparable across formats. In a mixed-format image,
`best` is therefore not necessarily the most reliable read; select by format or
payload when your application knows what it expects. Checksums reduce wrong
reads but cannot rule them out.

`payloadBytes` is supplied by QR Code, Data Matrix, Aztec, PDF417 and MaxiCode.
It holds decoded data bytes before character-set interpretation, not raw
codewords; Aztec Rune gives its value as decimal ASCII. Encoding `text` as UTF-8
does not reconstruct these bytes. Use `Uint8Array.from(barcode.payloadBytes)`
for a byte buffer.

All types (`ScanResult`, `InspectionResult`, `Barcode`, `ScannerOptions`,
`ScanOptions`, `Format` and others) are exported from both entries.

## EAN/UPC supplements

Set `eanAddOnPolicy` when creating a scanner or calling one-shot `scan` or `inspect`.

| Policy      | Behavior                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| `"ignore"`  | Decode the main barcode without reading its supplement.                                          |
| `"read"`    | Try reading the two- or five-digit supplement; keep the main barcode if none is readable.        |
| `"require"` | Return an EAN/UPC barcode only when its supplement is readable. Other formats remain unaffected. |

The supplement appears in `barcode.eanAddOn`; `barcode.text` is the main payload,
and `polygon` and `rect` describe the main barcode. Reading supplements adds
decoding work. Retail reads rejected by `"require"` appear in `result.undecoded`.

## Undecoded regions

`inspect` results list `undecoded` regions: localized proposals without an
accepted decode, each with a source-image `polygon` and a `format` hint. They
can overlap or be false candidates, and an empty list does not prove that every
barcode was found.

## Diagnostics

```js
const report = scanner.inspect(image);
console.log(report.diagnostics.regions.proposals, report.diagnostics.regions.searchWindows);
console.log(report.diagnostics.scan.barcodes);
```

`diagnostics.regions` has a stable shape: `proposals` and `searchWindows` are
`null` when a reader does not expose them, and empty arrays when it found nothing.
The other fields are the engine's own evidence; they vary by reader and may
change between releases. Candidate indices inside recovery crops are local to
the crop, not identifiers for tracking between frames.

## Errors

Invalid options throw `TypeError`. Engine and validation failures throw the
exported `ScannerError` with `.message` and a `.code` of `"invalid_input"`,
`"disposed"`, `"engine"` or `"capacity"` (too many live scanners). Loader and
fetch errors reject `Scanner.create` and the one-shot helpers. Dispose reusable
scanners in `finally`.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

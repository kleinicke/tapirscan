# Migrating from Quagga

For [QuaggaJS](https://github.com/serratus/quaggaJS) and its fork
[`@ericblade/quagga2`](https://github.com/ericblade/quagga2), which share one API;
examples use the quagga2 names.

## Install

```sh
npm uninstall @ericblade/quagga2   # or: npm uninstall quagga
npm install tapirscan
```

## Single images

### Browser

Quagga loads the image itself from `src` (a URL or data URL):

```js
// Before
Quagga.decodeSingle(
  { src: url, numOfWorkers: 0, decoder: { readers: ["ean_reader", "code_128_reader"] } },
  (result) => {
    if (result?.codeResult) console.log(result.codeResult.code);
    else console.log("not found");
  },
);
```

Tapirscan takes a `File`, `Blob`, `<img>`, canvas or `ImageBitmap`; fetch URLs
yourself:

```js
// After
import { scan } from "tapirscan/browser/medium";

const blob = await (await fetch(url)).blob(); // or a File from <input type="file">
const result = await scan(blob, { formats: ["EAN13", "Code128"] });
console.log(result.values); // [] when nothing is decoded; no error is thrown
```

The one-shot `scan` starts a worker per call; reuse a `Scanner` for several
images.

### Node

quagga2 decodes the image file for you. Its `decodeSingle` scales every image to
800 pixels on its longest side by default (`inputStream.size: 800`, up or down),
and returns `box`, `boxes` and `line` in that scaled space. Set
`inputStream: { size: 0 }` to keep the original size:

```js
// Before
import Quagga from "@ericblade/quagga2";

const result = await Quagga.decodeSingle({
  src: "label.png",
  numOfWorkers: 0, // no web workers in Node
  inputStream: { size: 0 },
  decoder: { readers: ["ean_reader"] },
});
console.log(result.codeResult?.code);
```

Tapirscan's core scans decoded pixels, so decode the file first, for example
with [sharp](https://sharp.pixelplumbing.com). Coordinates are in the pixels you
pass in.

```js
// After
import sharp from "sharp";
import { Scanner } from "tapirscan";

const scanner = await Scanner.create({ formats: ["EAN13"] });
try {
  const { data, info } = await sharp("label.png").raw().toBuffer({ resolveWithObject: true });
  const result = scanner.scan({
    data,
    width: info.width,
    height: info.height,
    channels: info.channels,
  });
  console.log(result.values);
} finally {
  scanner.dispose();
}
```

## Live camera

Quagga creates the video and overlay inside `target`:

```js
// Before
Quagga.init(
  {
    inputStream: {
      type: "LiveStream",
      target: document.querySelector("#viewport"),
      constraints: { facingMode: "environment", width: 1280, height: 720 },
    },
    locate: true,
    numOfWorkers: 2,
    frequency: 10,
    decoder: { readers: ["ean_reader"] },
  },
  (error) => {
    if (error) return console.error(error);
    Quagga.start();
  },
);
Quagga.onProcessed((result) => {
  /* draw boxes on Quagga.canvas.dom.overlay */
});
Quagga.onDetected((result) => console.log(result.codeResult.code));
// later
Quagga.stop();
```

`CameraScanner` plays the camera in a `<video>` element you place yourself:

```js
// After
import { Scanner } from "tapirscan/browser/medium";
import { CameraScanner } from "tapirscan/camera";

const video = document.querySelector("video");
const scanner = new Scanner({ formats: ["EAN13"] });
const camera = new CameraScanner(video, scanner, {
  onScan(result) {
    // every scanned frame; result.barcodes may be empty
    for (const barcode of result.barcodes) console.log(barcode.text);
  },
  onError(error) {
    // camera or scanner failure; scanning has stopped
    console.error(error);
  },
});
await camera.start();
// later
camera.stop();
```

| Quagga                                    | Tapirscan                                                                 |
| ----------------------------------------- | ------------------------------------------------------------------------- |
| `inputStream.target`                      | Your own `<video>` element.                                               |
| `inputStream.constraints`                 | `constraints` option, merged over a 1280×720 ideal size.                  |
| `Quagga.onDetected`                       | `onScan`, called for every scanned frame; check `result.barcodes.length`. |
| `Quagga.onProcessed`                      | The same `onScan` callback; draw from `result.barcodes`.                  |
| `Quagga.init` error callback              | `onError`, after which scanning has stopped.                              |
| `Quagga.start()` / `Quagga.stop()`        | `await camera.start()` / `camera.stop()`.                                 |
| `numOfWorkers`                            | Not needed: one scanner runs in one worker.                               |
| `frequency`                               | None: each new frame is scanned once the previous scan finished.          |
| `decoder.readers`                         | `formats` on the scanner. See [reader mapping](#reader-mapping).          |
| `locate`, `locator.*`, `inputStream.area` | See [localization and cropping](#localization-and-cropping).              |

`camera.stop()` does not dispose the scanner. See
[camera scanning](../bindings/javascript/README.md#camera-scanning) for camera
selection, torch and hidden pages.

### Drawing an overlay

`polygon` is in frame pixels (`videoWidth` × `videoHeight`). To draw on a canvas
over the `<video>`, map it to the displayed size, accounting for `object-fit`:

```js
function frameToDisplay(video) {
  const frameW = video.videoWidth;
  const frameH = video.videoHeight;
  const { width: viewW, height: viewH } = video.getBoundingClientRect();
  const fit = getComputedStyle(video).objectFit;
  // "cover" fills the element and crops; "contain" letterboxes.
  const scale =
    fit === "cover"
      ? Math.max(viewW / frameW, viewH / frameH)
      : Math.min(viewW / frameW, viewH / frameH);
  const offsetX = (viewW - frameW * scale) / 2; // negative for "cover" when cropped
  const offsetY = (viewH - frameH * scale) / 2;
  return ([x, y]) => [x * scale + offsetX, y * scale + offsetY];
}

function drawBarcodes(canvas, video, barcodes) {
  const ratio = devicePixelRatio;
  canvas.width = video.clientWidth * ratio;
  canvas.height = video.clientHeight * ratio;
  const ctx = canvas.getContext("2d");
  ctx.scale(ratio, ratio);
  const map = frameToDisplay(video);
  ctx.lineWidth = 3;
  ctx.strokeStyle = "lime";
  for (const { polygon } of barcodes) {
    ctx.beginPath();
    polygon.map(map).forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.closePath();
    ctx.stroke();
  }
}

// In the onScan handler: drawBarcodes(overlay, video, result.barcodes);
```

Give the canvas the video's CSS size and position, and the same mirroring
transform if any. Redrawing on every `onScan`, including empty ones, clears
stale boxes.

## Reader mapping

`formats` takes an identifier, a keyword group such as `"retail"`, or an array
([format guide](FORMATS.md)).

| Quagga reader / setting                                | Tapirscan                                                                                           |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| `ean_reader`                                           | `EAN13`                                                                                             |
| `ean_8_reader`                                         | `EAN8`                                                                                              |
| `upc_reader`                                           | `UPCA`. With `EAN13` also selected, a zero-prefixed EAN-13 is returned as 12-digit `UPCA`.          |
| `upc_e_reader`                                         | `UPCE`                                                                                              |
| `code_128_reader`                                      | `Code128` (includes GS1-128)                                                                        |
| `code_39_reader`                                       | `Code39` (standard characters; no full-ASCII expansion)                                             |
| `code_39_vin_reader`                                   | No VIN reader. `Code39` reads the characters; validate the 17-character VIN (check digit) yourself. |
| `code_93_reader`                                       | `Code93`                                                                                            |
| `codabar_reader`                                       | `Codabar`                                                                                           |
| `i2of5_reader`                                         | `ITF` (Interleaved 2 of 5)                                                                          |
| `2of5_reader` (standard/industrial 2 of 5)             | Not supported.                                                                                      |
| `code_32_reader`                                       | `Code32`, opt-in: no keyword group includes it.                                                     |
| `pharmacode_reader`                                    | Not supported. `PZN` (German pharmacy number in Code 39) is a different code and opt-in.            |
| `config.supplements: ["ean_5_reader", "ean_2_reader"]` | `eanAddOnPolicy`: `"ignore"`, `"read"` or `"require"`.                                              |

Quagga apps list a supplement reader and a plain `ean_reader`, because a reader
with supplements no longer reads plain EAN-13. Tapirscan uses one policy:

```js
// Before
decoder: {
  readers: [
    { format: "ean_reader", config: { supplements: ["ean_5_reader", "ean_2_reader"] } },
    "ean_reader",
  ],
}

// After
const scanner = new Scanner({ formats: ["EAN13"], eanAddOnPolicy: "read" });
// barcode.text is the main code; barcode.eanAddOn holds the 2- or 5-digit supplement
```

`"require"` returns EAN/UPC barcodes only with a readable supplement; the
default `"ignore"` skips supplements. Result formats are the identifiers above:
`codeResult.format === "ean_13"` becomes `barcode.format === "EAN13"`.

## Results

| Quagga                                      | Tapirscan                                                                        |
| ------------------------------------------- | -------------------------------------------------------------------------------- |
| `result.codeResult.code`                    | `result.values[0]` or `barcode.text`                                             |
| `result.codeResult.format`                  | `barcode.format` (`"EAN13"`, ...)                                                |
| `result.box`, `result.boxes`, `result.line` | `barcode.polygon` (four `[x, y]` corners); `barcode.rect` encloses it            |
| `result.codeResult.decodedCodes[].error`    | No per-bar error values. `barcode.support` ranks reads; it is not a probability. |
| `result.codeResult` absent                  | `result.barcodes` is empty                                                       |
| `decoder.multiple: true`                    | Always on: every decoded barcode is returned.                                    |

```js
// Before
Quagga.onDetected(({ codeResult }) => console.log(codeResult.code, codeResult.format));

// After
for (const { text, format, polygon } of result.barcodes) {
  console.log(text, format, polygon);
}
```

`polygon` outlines the decoded barcode in the coordinates of the image you
passed in; Quagga's `box` is a localization candidate.

### Suppressing misreads

Tapirscan validates check digits, so filters on `decodedCodes` errors have no
equivalent. Check digits do not rule out misreads, especially for formats
without one (Code 39, Codabar), so camera flows may still confirm a value
across frames:

```js
function confirmed(requiredReads = 2, windowMs = 1000) {
  const seen = new Map(); // key -> { count, last }
  return (barcodes, now = performance.now()) => {
    const hits = [];
    for (const [key, entry] of seen) if (now - entry.last > windowMs) seen.delete(key);
    for (const { format, text } of barcodes) {
      const key = `${format}:${text}`;
      const entry = seen.get(key) ?? { count: 0, last: now };
      entry.count += 1;
      entry.last = now;
      seen.set(key, entry);
      if (entry.count === requiredReads) hits.push({ format, text });
    }
    return hits;
  };
}

const confirm = confirmed(2);
// In onScan: for (const hit of confirm(result.barcodes)) accept(hit);
```

Each value is reported once, on its `requiredReads`th sighting; a gap longer
than `windowMs` restarts the count.

## Localization and cropping

Tapirscan always localizes, in any orientation. Effort is one `mode`:

| Quagga                    | Tapirscan                                                                     |
| ------------------------- | ----------------------------------------------------------------------------- |
| `locate: true`, defaults  | `mode: "medium"` (default)                                                    |
| `locate: false`, speed    | `mode: "low"`, or a narrow `formats` list                                     |
| `halfSample`, `patchSize` | Not configurable; harder images: `mode: "high"` or `"very-high"`              |
| `inputStream.size`        | Pass frames at the resolution you want scanned; camera: request 720p or 1080p |
| `inputStream.area`        | No region option. Crop the input yourself, as below.                          |

To replace `area`, crop before scanning and add the offset back:

```js
async function scanRegion(scanner, source, { left, top, width, height }) {
  const canvas = new OffscreenCanvas(width, height);
  canvas.getContext("2d").drawImage(source, left, top, width, height, 0, 0, width, height);
  const result = await scanner.scan(canvas);
  return result.barcodes.map((barcode) => ({
    ...barcode,
    polygon: barcode.polygon.map(([x, y]) => [x + left, y + top]),
  }));
}
```

For a video, the region is in `videoWidth` × `videoHeight` pixels; Quagga's
middle-half `area` becomes `{ left: 0, top: h * 0.25, width: w, height: h * 0.5 }`.

## Bundle size and CSP

Import a single-mode entry such as `tapirscan/browser/medium` (1 MB gzip). Sites
with a Content Security Policy need `'wasm-unsafe-eval'`; see
[browser deployment](BROWSER_DEPLOYMENT.md).

## Python for server-side pipelines

A Node service that runs quagga2 only to decode uploaded images can move to
Python:

```sh
pip install tapirscan pillow
```

```python
from PIL import Image
import tapirscan

with Image.open("label.png") as image:
    result = tapirscan.scan(image, formats=["EAN13"])

for barcode in result.barcodes:
    print(barcode.text, barcode.format, barcode.rect)
```

`scan` also accepts NumPy arrays and PyTorch tensors; see the
[Python guide](../bindings/python/README.md).

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

`CameraScanner` (preview) plays the camera in a `<video>` element you place yourself:

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

| Quagga                                    | Tapirscan                                                                                           |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `inputStream.target`                      | Your own `<video>` element.                                                                         |
| `inputStream.constraints`                 | `start({ facingMode, deviceId })`; other constraints in `constraints`.                              |
| `Quagga.onDetected`                       | `onDetect(barcode)`, once per barcode in view; `confirmations` requires several consecutive reads.  |
| `Quagga.onProcessed`                      | The same `onScan` callback; draw from `result.barcodes`.                                            |
| `Quagga.init` error callback              | `start()` rejects; later failures call `onError`.                                                   |
| `Quagga.start()` / `Quagga.stop()`        | `await camera.start()` / `camera.stop()`.                                                           |
| `numOfWorkers`                            | Not needed: one scanner runs in one worker.                                                         |
| `frequency`                               | `scanInterval` in milliseconds (`frequency: 10` → `scanInterval: 100`).                             |
| `decoder.readers`                         | `formats` on the scanner. See [reader mapping](#reader-mapping).                                    |
| `locate`, `locator.*`, `inputStream.area` | See [localization and cropping](#localization-and-cropping).                                        |
| `Quagga.offDetected`, `offProcessed`      | One `onScan` and one `onDetect` per controller; check a flag in them or call `camera.stop()`.       |
| `inputStream.type: "VideoStream"`         | A frame loop on a `<video src>`: [recorded video](../bindings/javascript/README.md#recorded-video). |
| `inputStream.type: "ImageStream"`         | `for (const file of files) await scanner.scan(file)`.                                               |
| `Quagga.registerReader`                   | Not supported; Tapirscan has no reader plugins.                                                     |

`camera.stop()` does not dispose the scanner. See
[camera scanning](../bindings/javascript/README.md#camera-scanning) for camera
selection, torch and hidden pages.

`Quagga.CameraAccess` maps to the controller:

| `Quagga.CameraAccess`              | Tapirscan                                                                                                             |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `enumerateVideoDevices()`          | `CameraScanner.cameras()`, after `start()` ([camera picker](../bindings/javascript/README.md#camera-picker-and-zoom)) |
| `request(video, constraints)`      | `camera.start({ deviceId })` or `start({ facingMode })`                                                               |
| `release()`                        | `camera.stop()`                                                                                                       |
| `getActiveTrack()`                 | `camera.track`                                                                                                        |
| `getActiveStreamLabel()`           | `camera.track?.label`                                                                                                 |
| `enableTorch()` / `disableTorch()` | `await camera.setTorch(true)` / `setTorch(false)`; `camera.torchAvailable` tells whether it works                     |

### Drawing results

`Quagga.canvas.dom.overlay` and `Quagga.ImageDebug.drawPath` become a canvas you
lay over the video and draw `polygon` on; see
[drawing results over the video](../bindings/javascript/README.md#drawing-results-over-the-video).

Only decoded barcodes are drawn. Quagga's `boxes` show every localization
candidate; the nearest Tapirscan equivalent is `scanner.inspect(video)`, whose
`undecoded` regions mark candidates that did not decode.

## Reader mapping

`formats` takes an identifier, a keyword group such as `"retail"`, or an array
([format guide](FORMATS.md)). Quagga's default reader is `code_128_reader`;
Tapirscan's default, `retail` (EAN13, UPCA, EAN8, UPCE), does not include
Code128, so list every format you need.

| Quagga reader / setting                                | Tapirscan                                                                                           |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| `ean_reader`                                           | `EAN13`                                                                                             |
| `ean_8_reader`                                         | `EAN8`                                                                                              |
| `upc_reader`                                           | `UPCA`. With `EAN13` also selected, a zero-prefixed EAN-13 is returned as 12-digit `UPCA`.          |
| `upc_e_reader`                                         | `UPCE`                                                                                              |
| `code_128_reader`                                      | `Code128` (includes GS1-128)                                                                        |
| `code_39_reader`                                       | `Code39` (standard characters); `Code39Extended` (opt-in) expands full ASCII                        |
| `code_39_vin_reader`                                   | No VIN reader. `Code39` reads the characters; validate the 17-character VIN (check digit) yourself. |
| `code_93_reader`                                       | `Code93`                                                                                            |
| `codabar_reader`                                       | `Codabar`; `text` omits the start/stop letters, `codabarStartStop` holds them.                      |
| `i2of5_reader`                                         | `ITF` (Interleaved 2 of 5)                                                                          |
| `2of5_reader` (standard/industrial 2 of 5)             | `Industrial25` (opt-in).                                                                            |
| `code_32_reader`                                       | `Code32`, opt-in: no keyword group includes it.                                                     |
| `pharmacode_reader`                                    | `Pharmacode` (opt-in). `PZN` (German pharmacy number in Code 39) is a different code.               |
| `config.supplements: ["ean_5_reader", "ean_2_reader"]` | `eanAddOnPolicy`: `"ignore"`, `"read"` or `"require"`.                                              |

Quagga apps list a supplement reader and a plain `ean_reader`, because a reader
with supplements does not read plain EAN-13. Tapirscan uses one policy:

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

Tapirscan validates check digits, so filters on `decodedCodes` errors have no
equivalent. Formats without a check digit (Code 39, Codabar) can still misread;
for those, set `confirmations: 2` or more and act on `onDetect`.

## Localization and cropping

Tapirscan always localizes, in any orientation. Effort is a mode: import
`tapirscan/browser/<mode>`, or pass `mode` to `tapirscan/browser` or the core.

| Quagga                    | Tapirscan                                                              |
| ------------------------- | ---------------------------------------------------------------------- |
| `locate: true`, defaults  | Medium (default)                                                       |
| `locate: false`, speed    | Low, or a narrow `formats` list                                        |
| `halfSample`, `patchSize` | Not configurable; harder images: High or Very High                     |
| `inputStream.size`        | Pass frames at the resolution to scan; the camera defaults to 1280×720 |
| `inputStream.area`        | `region` of `CameraScanner`, as below                                  |

Quagga's percent insets become a `region` in fractions of the frame:

```js
// Before: inputStream: { area: { top: "25%", right: "10%", left: "10%", bottom: "25%" } }
const camera = new CameraScanner(video, scanner, {
  region: { left: 0.1, top: 0.25, width: 0.8, height: 0.5 },
  onScan,
  onError,
});
```

For `decodeSingle` with an `area`, draw the same part of the image into a canvas,
scan the canvas, and add the offset in the same way.

## Bundle size and CSP

Import a single-mode entry such as `tapirscan/browser/medium`; sizes are under
[bundle one mode](BROWSER_DEPLOYMENT.md#bundle-one-mode). Sites with a Content
Security Policy need `'wasm-unsafe-eval'`; see
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
[Python guide](../bindings/python/README.md). For camera or video frames with
OpenCV, see the [Python OpenCV loop](../bindings/python/README.md#opencv).

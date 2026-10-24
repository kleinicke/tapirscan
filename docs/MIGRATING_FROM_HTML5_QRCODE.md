# Migrating from html5-qrcode

For [html5-qrcode](https://github.com/mebjas/html5-qrcode) 2.x: the
`Html5QrcodeScanner` widget and the `Html5Qrcode` class.

- **No widget.** html5-qrcode builds its camera view, camera list, torch
  button and file picker inside a `<div>`. Tapirscan scans; `CameraScanner`
  (preview) plays the camera in a `<video>` you place, and you add the controls
  you want ([full page below](#replacing-html5qrcodescanner)).
- **Default formats are `retail`** (EAN13, UPCA, EAN8, UPCE). html5-qrcode reads
  all its formats unless `formatsToSupport` is set. Pass `formats`: `"common"`
  adds Code128, Code39, ITF, QR Code and Data Matrix; `"all"` reads all but the opt-in formats.
- **Empty results are reported.** html5-qrcode calls the error callback for each
  frame without a code; `onScan` receives each scanned frame's result, with an
  empty `barcodes` array when nothing was read.

## Install

```sh
npm uninstall html5-qrcode
npm install tapirscan
```

Sites with a Content Security Policy need `'wasm-unsafe-eval'`; see
[browser deployment](BROWSER_DEPLOYMENT.md), which also lists the
[size of each entry](BROWSER_DEPLOYMENT.md#bundle-one-mode).

## Scanning a file

```js
// Before
import { Html5Qrcode } from "html5-qrcode";

const reader = new Html5Qrcode("reader"); // needs a <div id="reader">
try {
  const text = await reader.scanFile(file, false);
  console.log(text);
} catch {
  console.log("not found");
}
```

```js
// After
import { Scanner } from "tapirscan/browser/medium";

const scanner = new Scanner({ formats: ["QRCode"] });
const { barcodes } = await scanner.scan(file); // also <img>, canvas, Blob, ImageData
console.log(barcodes.map((b) => b.text)); // [] when nothing is read; no exception
```

`scanFileV2` maps the same way: `decodedText` is `barcode.text`. Keep one
scanner for repeated scans; `dispose()` stops its worker.

## Camera with `Html5Qrcode`

```js
// Before
const reader = new Html5Qrcode("reader", {
  formatsToSupport: [Html5QrcodeSupportedFormats.QR_CODE],
});
await reader.start(
  { facingMode: "environment" },
  { fps: 10, qrbox: { width: 250, height: 250 } },
  (decodedText, decodedResult) => console.log(decodedText),
  (errorMessage) => {}, // called for every frame without a code
);
// later
await reader.stop();
```

```js
// After
import { Scanner } from "tapirscan/browser/medium";
import { CameraScanner } from "tapirscan/camera";

const video = document.querySelector("video");
const scanner = new Scanner({ formats: ["QRCode"] });
const camera = new CameraScanner(video, scanner, {
  onScan({ barcodes }) {
    for (const barcode of barcodes) console.log(barcode.text);
  },
  onError(error) {
    // camera or scanner failure after start(); scanning has stopped
    console.error(error);
  },
});
await camera.start(); // rejects without permission or camera
// later
camera.stop();
```

| html5-qrcode                                                                 | Tapirscan                                                                                                              |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `new Html5Qrcode(elementId)`                                                 | `new CameraScanner(video, scanner, options)` with your own `<video>`                                                   |
| `start(cameraId, …)`                                                         | `start({ deviceId })`                                                                                                  |
| `start({ facingMode: "environment" }, …)`                                    | `start()` (back camera preferred) or `start({ facingMode: "user" })`                                                   |
| `fps`                                                                        | `scanInterval` in milliseconds (`fps: 10` → `scanInterval: 100`); default: the next frame once a scan finishes         |
| `qrbox`                                                                      | `region`; see [scan region](#qrbox-scan-region)                                                                        |
| `aspectRatio`, `videoConstraints`                                            | `constraints` option, for example `{ aspectRatio: 1 }`; the default ideal size is 1280×720                             |
| `disableFlip`                                                                | None; mirrored QR Code, Data Matrix and Aztec symbols are read automatically                                           |
| success callback                                                             | `onScan`, when `barcodes.length > 0`                                                                                   |
| error callback per frame                                                     | `onScan` with empty `barcodes`; delete "No MultiFormat Readers" handling                                               |
| `start()` rejection                                                          | `start()` rejection; later failures call `onError`                                                                     |
| `stop()`, `clear()`                                                          | `camera.stop()`; remove your elements yourself. `scanner.dispose()` when done                                          |
| `pause()` / `resume()`                                                       | `video.pause()` / `video.play()` pauses scanning with the camera open; `camera.stop()` / `camera.start()` releases it  |
| `getState()`, `isScanning`                                                   | `camera.running`                                                                                                       |
| `Html5Qrcode.getCameras()` (`{ id, label }`)                                 | `CameraScanner.cameras()` (`MediaDeviceInfo`: `deviceId`, `label`)                                                     |
| `applyVideoConstraints`, `getRunningTrackSettings`                           | `camera.track.applyConstraints(…)`, `camera.track.getSettings()`                                                       |
| torch via `applyVideoConstraints`                                            | `camera.torchAvailable`, `await camera.setTorch(true)`                                                                 |
| zoom via `applyVideoConstraints`                                             | `camera.track.applyConstraints(…)` ([camera picker and zoom](../bindings/javascript/README.md#camera-picker-and-zoom)) |
| `rememberLastUsedCamera`, `showTorchButtonIfSupported`, `supportedScanTypes` | Your own controls; see [the full page](#replacing-html5qrcodescanner)                                                  |
| `useBarCodeDetectorIfSupported`                                              | None; the same WebAssembly reader runs in every browser                                                                |

To stop after the first code, as many html5-qrcode apps do in the success
callback:

```js
onScan({ barcodes }) {
  if (!barcodes.length) return;
  camera.stop();
  handle(barcodes[0].text);
},
```

For formats without a check digit (Code 39, Codabar), handle reads in
`onDetect` with `confirmations: 2`: it is called once a barcode was read in two
consecutive scans.

## Formats

| `Html5QrcodeSupportedFormats` | Tapirscan                                                    |
| ----------------------------- | ------------------------------------------------------------ |
| `QR_CODE`                     | `QRCode`                                                     |
| `AZTEC`                       | `Aztec`                                                      |
| `CODABAR`                     | `Codabar`                                                    |
| `CODE_39`                     | `Code39`                                                     |
| `CODE_93`                     | `Code93`                                                     |
| `CODE_128`                    | `Code128` (includes GS1-128)                                 |
| `DATA_MATRIX`                 | `DataMatrix`                                                 |
| `MAXICODE`                    | `MaxiCode`                                                   |
| `ITF`                         | `ITF`                                                        |
| `EAN_13`, `EAN_8`             | `EAN13`, `EAN8`                                              |
| `PDF_417`                     | `PDF417`                                                     |
| `RSS_14`, `RSS_EXPANDED`      | `DataBar`, `DataBarExpanded`                                 |
| `UPC_A`, `UPC_E`              | `UPCA`, `UPCE`                                               |
| `UPC_EAN_EXTENSION`           | No format; `eanAddOnPolicy: "read"`, then `barcode.eanAddOn` |

`formats` also takes groups: `"common"` (retail, Code128, Code39, ITF, QR Code,
Data Matrix), `"2D"`, `"all"` ([format guide](FORMATS.md)). Each enabled 2D
format searches the whole frame, so select only what you expect.

## Results

| html5-qrcode                                 | Tapirscan                                                                                  |
| -------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `decodedText`                                | `barcode.text`                                                                             |
| `decodedResult.result.format.formatName`     | `barcode.format` (`"QRCode"`, `"EAN13"`, …)                                                |
| `decodedResult.result.bounds` (when present) | `barcode.rect`; `barcode.polygon` has the four corners in frame pixels                     |
| one code per frame                           | every code in the frame                                                                    |
| none                                         | `barcode.content`: Wi-Fi, contact, GS1 and other fields ([structured content](CONTENT.md)) |
| none                                         | `barcode.payloadBytes`: decoded payload bytes, when available                              |

html5-qrcode uses the browser's `BarcodeDetector` where available and
`@zxing/library` otherwise, so its text varies by browser. Where it used ZXing,
UPC-A, UPC-E, Codabar and DataBar text differs from Tapirscan's
([details](MIGRATING_FROM_ZXING.md#formats); Codabar start/stop letters are in
`codabarStartStop`).

## `qrbox`: scan region

html5-qrcode scans only the `qrbox` area and shades the rest. `region` does the
same, in fractions of the frame; shade the area with CSS if you want the same
look:

```js
// A centered 250×250 box of a 1280×720 frame
const camera = new CameraScanner(video, scanner, {
  region: { left: 515 / 1280, top: 235 / 720, width: 250 / 1280, height: 250 / 720 },
  onScan,
  onError,
});
```

`qrbox` is in displayed pixels, while `region` is a fraction of the camera frame
(`videoWidth` × `videoHeight`); convert with the preview's displayed size.
Tapirscan finds codes anywhere in the frame, in any orientation, so the region is
optional.

## Replacing `Html5QrcodeScanner`

A complete page with what the widget offers: preview, camera list, torch,
image file, the last result, and a remembered camera.

```html
<video style="display: block; width: 100%; max-width: 640px"></video>
<select hidden></select>
<button id="torch" hidden>Torch</button>
<input type="file" accept="image/*" />
<p id="result"></p>
<script type="module">
  import { Scanner } from "tapirscan/browser/medium";
  import { CameraScanner } from "tapirscan/camera";

  const scanner = new Scanner({ formats: ["QRCode", "EAN13", "Code128"] });
  const result = document.querySelector("#result");
  const show = ({ barcodes }) => {
    if (barcodes.length)
      result.textContent = barcodes.map((b) => `${b.format}: ${b.text}`).join("\n");
  };
  const camera = new CameraScanner(document.querySelector("video"), scanner, {
    onScan: show,
    onError: (error) => (result.textContent = error.message),
  });

  const saved = localStorage.getItem("camera");
  try {
    await camera.start(saved ? { deviceId: saved } : {});
  } catch {
    await camera.start().catch((error) => (result.textContent = error.message));
  }

  const torch = document.querySelector("#torch");
  let on = false;
  torch.hidden = !camera.torchAvailable;
  torch.onclick = async () => {
    if (await camera.setTorch(!on)) on = !on;
  };

  const select = document.querySelector("select");
  const cameras = await CameraScanner.cameras();
  select.replaceChildren(
    ...cameras.map((c, i) => new Option(c.label || `Camera ${i + 1}`, c.deviceId)),
  );
  select.value = camera.track?.getSettings().deviceId ?? "";
  select.hidden = cameras.length < 2;
  select.onchange = async () => {
    localStorage.setItem("camera", select.value);
    await camera.start({ deviceId: select.value });
    torch.hidden = !camera.torchAvailable; // the torch is off after a switch
    on = false;
  };

  document.querySelector("input").onchange = async (event) => {
    const file = event.currentTarget.files?.[0];
    if (file) show(await scanner.scan(file));
  };
</script>
```

With a bundler, `tapirscan/browser/medium` resolves from `node_modules`; without
one, import `dist/browser-medium.js` and `dist/camera.js` from where you serve the
package ([browser deployment](BROWSER_DEPLOYMENT.md#serve-the-worker-from-the-pages-origin)).

Requirements: HTTPS or `localhost`; browser versions are listed under
[browser apps](../bindings/javascript/README.md#browser-apps-react-and-svelte). The scanner
runs in a worker, so the page stays responsive.

## Drawing results

`polygon` is in frame pixels. A canvas laid over the video with the frame's size
needs no conversion; see
[drawing results over the video](../bindings/javascript/README.md#drawing-results-over-the-video).

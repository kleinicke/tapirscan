# Migrating from flutter_zxing

Covers [`flutter_zxing`](https://pub.dev/packages/flutter_zxing) 3.x, the Flutter
plugin over zxing-cpp. Tapirscan's [Dart package](../bindings/flutter/README.md)
replaces its decoder; it has no camera widget, so camera code stays in the app.
The package is a preview built from source; see its setup section first.

## Install

```yaml
dependencies:
  tapirscan:
    git:
      url: https://github.com/kleinicke/tapirscan
      path: bindings/flutter
```

flutter_zxing uses one global `zx` and passes `DecodeParams` with every call.
Tapirscan uses a `Scanner` created once with its options and disposed when done:

```dart
// Before
import 'package:flutter_zxing/flutter_zxing.dart';
final params = DecodeParams(format: Format.ean13 | Format.qrCode, tryHarder: true);

// After
import 'package:tapirscan/tapirscan.dart' as ts;
final scanner = ts.Scanner(
  const ts.ScannerOptions(mode: ts.Mode.medium, formats: {ts.Format.ean13, ts.Format.qrCode}),
);
// ...
scanner.dispose();
```

The prefix avoids clashes: both packages export `Format`.

## Formats

flutter_zxing combines `int` constants with `|`; Tapirscan takes a `Set<Format>`.

| flutter_zxing                                         | Tapirscan                                                                                                    |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `ean13`, `ean8`, `upca`, `upce`                       | `ean13`, `ean8`, `upcA`, `upcE`, or `Format.retail`                                                          |
| `code128`, `code39`, `code93`, `codabar`, `itf`       | the same names                                                                                               |
| `dataBar`, `dataBarExpanded`                          | the same names                                                                                               |
| `qrCode`, `dataMatrix`, `pdf417`, `aztec`, `maxiCode` | the same names                                                                                               |
| `linearCodes`, `matrixCodes`, `any` (the default)     | `Format.linear`, `Format.matrix`, `Format.all`                                                               |
| `microQRCode`, `dataBarLimited`                       | `microQrCode`, `dataBarLimited`                                                                              |
| `rmqrCode`, `microPdf417`, `telepen`, `dxFilmEdge`    | not supported                                                                                                |
| none                                                  | `code32`, `pzn`, `msiPlessey`, `industrial25`, `pharmacode`, `code39Extended`, `iata25`, `matrix25` (opt-in) |

Tapirscan's default is `Format.retail`, not every format: each enabled 2D format
searches the whole image, so select only what the app reads.

flutter_zxing decodes with zxing-cpp, so values follow the zxing-cpp column of the
[ZXing text differences](MIGRATING_FROM_ZXING.md#formats): UPC-A, UPC-E, Codabar
and DataBar text differ. Check stored values for those formats; for Codabar,
`codabarStartStop` restores the letters ZXing keeps.

## Options

| flutter_zxing `DecodeParams`                  | Tapirscan                                                                                                                                                       |
| --------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tryHarder`, `tryRotate`, `tryInverted`       | `mode`: `low`, `medium` (default), `high`, `veryHigh`; rotation is always handled; inverted (light-on-dark) symbols are read for QR Code, Data Matrix and Aztec |
| `maxSize` (768), `tryDownscale`               | none: full resolution up to 32 megapixels; pass a smaller image for speed                                                                                       |
| `cropLeft`/`cropTop`/`cropWidth`/`cropHeight` | a view with a stride, see [camera frames](#camera-frames)                                                                                                       |
| `maxNumberOfSymbols`, `isMultiScan`           | none: every scan returns all barcodes                                                                                                                           |
| `imageFormat`                                 | `PixelImage` `channels` (1, 3, 4) or `PixelImage.bgra`                                                                                                          |

## Image files

flutter_zxing reads paths and URLs; Tapirscan scans decoded pixels. Decode with the
platform codec, which is faster than a Dart decoder:

```dart
// Before
final codes = await zx.readBarcodesImagePathString(path, params);

// After (import 'dart:ui' as ui;)
final codec = await ui.instantiateImageCodec(await File(path).readAsBytes());
final image = (await codec.getNextFrame()).image;
final rgba = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
final result = await scanner.scan(
  ts.PixelImage(rgba!.buffer.asUint8List(), width: image.width, height: image.height, channels: 4),
);
```

For raw pixels, `zx.readBarcodes(bytes, DecodeParams(imageFormat: ImageFormat.rgb, width: w, height: h))`
becomes `scanner.scanSync(ts.PixelImage(bytes, width: w, height: h, channels: 3))`,
or `await scanner.scan(...)` off the UI isolate.

## Camera frames

Replace `ReaderWidget` with `CameraPreview` and an image stream from
[`camera`](https://pub.dev/packages/camera), or keep flutter_zxing's widget and
feed frames to both (see below). flutter_zxing copies the luma plane without its
row padding; Tapirscan reads it with its stride.

```dart
// Before
await zx.startCameraProcessing();
controller.startImageStream((image) async {
  final code = await zx.processCameraImage(image, params);
  if (code.isValid) onCode(code.text!);
});

// After
var busy = false;
controller.startImageStream((image) async {
  if (busy) return; // drop frames while one is scanned
  busy = true;
  try {
    final plane = image.planes.first;
    final pixels = image.format.group == ImageFormatGroup.bgra8888
        ? ts.PixelImage.bgra(plane.bytes, width: image.width, height: image.height, stride: plane.bytesPerRow)
        : ts.PixelImage(plane.bytes, width: image.width, height: image.height, stride: plane.bytesPerRow);
    final best = (await scanner.scan(pixels)).best;
    if (best != null) onCode(best.text);
  } finally {
    busy = false;
  }
});
```

flutter_zxing 3.1 notes that `camera_desktop` on Linux and Windows reports RGBA
frames as `bgra8888`; when `image.format.raw == 'RGBA'`, use
`PixelImage(..., channels: 4)` instead.

To scan only the centre of a frame, as `ReaderWidget`'s `cropPercent` does, pass a
view and keep the frame's stride; add the offset back to the results:

```dart
final view = Uint8List.sublistView(plane.bytes, top * plane.bytesPerRow + left);
final crop = ts.PixelImage(view, width: cropWidth, height: cropHeight, stride: plane.bytesPerRow);
```

## Results

| flutter_zxing `Code`                       | Tapirscan `Barcode`                                      |
| ------------------------------------------ | -------------------------------------------------------- |
| `text` (nullable), `isValid`               | `text`; a result contains only decoded barcodes          |
| `format` (`int`, `.name` via an extension) | `format` (`Format`, `.id` such as `'EAN13'`)             |
| `position` (`topLeftX` … `bottomRightY`)   | `polygon` (four `PixelPoint`s) and `rect` (`PixelRect`)  |
| `rawBytes`                                 | `payloadBytes` (matrix formats)                          |
| `duration`                                 | none; time the call                                      |
| none                                       | `support`, `eanAddOn`, `gs1`, `structuredAppend`, `best` |
| Codabar start/stop letters in `text`       | `codabarStartStop` (`text` omits them)                   |

`Codes.codes` becomes `ScanResult.barcodes`; `ScanResult.values` lists the texts.
Positions are in the scanned frame's pixels and sensor orientation in both
packages.

## Errors

flutter_zxing reports failures in `Code.isValid` and `Code.error`. Tapirscan
returns an empty result when nothing is found and throws `ScannerException`
(`code`: `invalidInput` or `engine`) for invalid images, and `StateError` after
`dispose`.

## Using both

The packages can live in one app: their native libraries have different names
(`libflutter_zxing`, `libtapirscan`).
Share one `CameraController` and pass each frame to the decoder you need. A common
split is Tapirscan where it runs and flutter_zxing elsewhere, such as 32-bit
Android or the web:

```dart
final tapirscan = ts.Tapirscan.isSupported ? ts.Scanner() : null;
// in the frame callback:
if (tapirscan != null) {
  // scan as above
} else {
  final code = await zx.processCameraImage(image, params);
}
```

Both native libraries ship in the app; Tapirscan's library is 5.9 MB for arm64 and 7.5 MB for x86_64 (measured in a debug
APK).

## Not available in Tapirscan

- A camera widget, torch or zoom UI: build them with `camera`.
- Barcode generation (`zx.encodeBarcode`, `WriterWidget`): keep flutter_zxing for
  it or use a generator package.
- Reading from URLs: download the bytes and decode them as above.
- The web: use Tapirscan's [JavaScript package](../bindings/javascript/README.md).

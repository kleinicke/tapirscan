# Tapirscan for Dart and Flutter

> **Preview:** the API may change in minor releases.

A Dart package over the Tapirscan C library through `dart:ffi`, for Flutter apps
and plain Dart programs. `scan` returns a `ScanResult` with `values`, `barcodes`
and `best`. Shared semantics (support, geometry, image limits) are in
[API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md).

## Setup

```sh
flutter pub add tapirscan   # or: dart pub add tapirscan
```

Requires Dart 3.10+ (Flutter 3.38+); tested with Flutter 3.44 and 3.47. The
dependency ranges also accept the older `code_assets` 1.x and `hooks` 2.0 that
other packages in an app may pin.

The package's build hook downloads the native library for each target from the
[GitHub release](https://github.com/kleinicke/tapirscan/releases) of the package
version, checks it against the SHA-256 checksums in the package's
`hook/SHA256SUMS`, and caches it in `.dart_tool`. Rust is not needed. The first
build of each target needs network access to `github.com`; `http_proxy` and
`https_proxy` are respected. Prebuilt libraries cover:

| Platform | Targets                                                                            |
| -------- | ---------------------------------------------------------------------------------- |
| Android  | arm64-v8a and x86_64, Android 7.0 (API 24) or later                                |
| iOS      | iOS 13 or later on arm64 devices and x86_64 simulators, iOS 14 on arm64 simulators |
| macOS    | macOS 11 or later, arm64 and x86_64                                                |
| Linux    | x86_64 and arm64, glibc 2.28 or later                                              |
| Windows  | x64 and arm64                                                                      |

Other 64-bit targets need a `source_dir` (a Tapirscan checkout) or a
`library_dir`. Settings go in the app's `pubspec.yaml`:

```yaml
hooks:
  user_defines:
    tapirscan:
      # A mirror of the release assets (https:// or file://), for builds
      # without access to github.com. Downloads are verified either way.
      # release_url: https://mirror.example.com/tapirscan/
      # or prebuilt libraries: <os>-<arch>/libtapirscan.so, such as android-arm64/
      # library_dir: native/
      # or a Tapirscan checkout to build from source:
      # source_dir: ../tapirscan
```

### Building from source

Inside a Tapirscan checkout, with `source_dir`, or as a Git dependency, the
hook compiles the library with Cargo on the first build of each target:

```yaml
dependencies:
  tapirscan:
    git:
      url: https://github.com/kleinicke/tapirscan
      path: bindings/flutter
```

The machine then needs the
[development prerequisites](https://github.com/kleinicke/tapirscan/blob/main/docs/DEVELOPMENT.md#prerequisites)
(rustup, Python 3.10+), the Cargo dependencies fetched once in the checkout
(`cargo fetch --locked` for `multiformat/` and `bindings/rust/`), and for Android
the NDK that Flutter uses. Concurrent builds of one checkout wait for each
other. `scripts/build_flutter_native.py --target <rust-triple> --output <dir>`
builds one library for `library_dir`, for example for an F-Droid source build.

## Scan

```dart
import 'package:tapirscan/tapirscan.dart';

final scanner = Scanner(
  const ScannerOptions(mode: Mode.low, formats: {Format.ean13, Format.qrCode}),
);
final result = await scanner.scan(
  PixelImage(grayPixels, width: 640, height: 480), // Uint8List, 1 byte per pixel
);
for (final barcode in result.barcodes) {
  print('${barcode.format} ${barcode.text} ${barcode.rect}');
}
scanner.dispose();
```

`scan` runs on a background isolate so the UI stays responsive; `scanSync` runs
on the calling isolate. Scans on one scanner run one at a time; create a scanner
once and reuse it. Results stay valid after `dispose`.

`ScannerOptions` holds `mode` (default `Mode.medium`), `formats` (default
`Format.retail`), `eanAddOnPolicy` (default `EanAddOnPolicy.ignore`) and
`parallel`. `Format` and its presets (`retail`, `common1D`, `common`, `linear`,
`matrix`, `all`) are generated from `config/formats.json`; see the
[format matrix](https://github.com/kleinicke/tapirscan/blob/main/docs/FORMATS.md#format-matrix). New formats may be added
in minor releases. `scan(image, formats: {...})` replaces the formats for one
call; an empty set throws `ArgumentError`.

`PixelImage` takes gray (1 channel), RGB (3) or RGBA (4, alpha ignored) bytes
and an optional row `stride`; the last row may be shorter than the stride. Each
scan copies the pixels into native memory.

Each `Barcode` has `text`, `format`, `polygon`, `rect`, `support`,
`payloadBytes`, `eanAddOn`, `gs1`, `readerInitialization`, `structuredAppend`
and `codabarStartStop` (Codabar start and stop characters such as `'AB'`, which
`text` omits); optional fields are null when unavailable. `polygon`
(`PixelPoint`s) and `rect` (`PixelRect`) are in source-image pixels; the names
avoid Flutter's `Offset` and `Rect`, for example
`Rect.fromLTWH(r.left.toDouble(), r.top.toDouble(), r.width.toDouble(), r.height.toDouble())`.

### Images and camera frames

Decode an image file with the platform codec and scan its RGBA pixels:

```dart
final codec = await ui.instantiateImageCodec(encodedBytes); // import 'dart:ui' as ui;
final image = (await codec.getNextFrame()).image;
final rgba = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
final result = await scanner.scan(
  PixelImage(rgba!.buffer.asUint8List(), width: image.width, height: image.height, channels: 4),
);
```

A `package:camera` frame in YUV (Android YUV420/NV21, iOS bi-planar) is scanned
through its luma plane without conversion; a BGRA frame is converted with
`PixelImage.bgra`. Results are in the frame's sensor orientation.

```dart
final plane = image.planes.first;
final pixels = image.format.group == ImageFormatGroup.bgra8888
    ? PixelImage.bgra(plane.bytes, width: image.width, height: image.height, stride: plane.bytesPerRow)
    : PixelImage(plane.bytes, width: image.width, height: image.height, stride: plane.bytesPerRow);
final result = await scanner.scan(pixels);
```

Scan one frame at a time and drop frames that arrive meanwhile. To scan only
part of a frame, pass a view: `Uint8List.sublistView(plane.bytes, top * stride + left)`
with the crop's width and height and the frame's stride.

Coming from `flutter_zxing`: see the
[migration guide](https://github.com/kleinicke/tapirscan/blob/main/docs/MIGRATING_FROM_FLUTTER_ZXING.md).

## Platforms and errors

64-bit Android (arm64-v8a, x86_64), iOS, macOS, Linux and Windows. Tested on
macOS arm64, Linux x86_64 and an Android arm64 emulator. Native ABI 7 is
64-bit only: on 32-bit Android `Tapirscan.isSupported` is false and `Scanner()`
throws `UnsupportedError`, so an app can fall back to another scanner. The app
build itself still succeeds.

Failed calls throw `ScannerException` with a `ScannerErrorCode` (`invalidInput`
or `engine`), the C `status` and the library's `message`. Calls on a disposed
scanner throw `StateError`, and a pending `scan` either completes or throws it.
No detection is an empty result, not an error.

Not available in Dart: `inspect` (undecoded regions, timings), structured
content parsing, and the cross-language parity tests the other bindings have.

## Development

```sh
cd bindings/flutter
dart pub get
dart test                  # builds the host library through the hook
dart analyze --fatal-infos
dart run tool/ffigen.dart  # after changing bindings/c/include/tapirscan.h
```

`lib/src/bindings.g.dart` is generated by `tool/ffigen.dart` and
`lib/src/format.g.dart` by `scripts/generate_formats.py`.

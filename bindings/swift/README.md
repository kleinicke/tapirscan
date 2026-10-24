# Tapirscan for Swift (iOS and macOS)

> **Preview:** built from source; the API may change in minor releases.

A Swift package over the Tapirscan C library, for iOS 15+ and macOS 12+. It scans
pixel buffers, `CGImage`s and camera `CVPixelBuffer`s. `scan` returns a
`ScanResult` with `values`, `barcodes` and `best`; `inspect` returns an
`InspectionResult` that adds undecoded regions, image size, mode, elapsed time and
diagnostics. Shared semantics (support, undecoded regions, geometry, image limits)
are in [API design](../../docs/API_DESIGN.md).

## Setup

Build the XCFramework (static libraries for macOS, iOS devices and the iOS
simulator), then add this directory as a local package in Xcode (**File → Add
Package Dependencies… → Add Local…**) or in your `Package.swift`:

```sh
python3 scripts/build_apple.py   # writes bindings/swift/TapirscanC.xcframework
```

```swift
.package(path: "path/to/tapirscan/bindings/swift")
```

## Scan

```swift
import Tapirscan

let scanner = try BarcodeScanner(options: ScannerOptions(formats: [.ean13, .qrCode]))
let result = try scanner.scan(cgImage)       // e.g. UIImage.cgImage
for barcode in result.barcodes {
    print(barcode.format, barcode.text, barcode.rect)
}
```

`ScannerOptions` holds `mode` (default `.medium`), `formats` (default `.retail`),
`eanAddOnPolicy` (default `.ignore`) and `parallel`, fixed when the scanner is
created. `parallel: false` keeps each scan on the calling thread, for apps that
already scan on several threads; `nil` uses the
[process default](../../docs/PERFORMANCE.md#threads-and-servers).
`Formats` is an option set of the `Format` cases with the presets `retail`,
`common1D`, `common`, `linear`, `matrix` and `all`; all of them are generated from
`config/formats.json`. A per-call `ScanOptions(formats:)` overrides the scanner's
formats for that call only.

For example, `ScannerOptions(formats: .common)` selects the common group.
See the [format and keyword-group matrix](../../docs/FORMATS.md#format-matrix)
for group membership and opt-in formats.

```swift
let qrOnly = try scanner.scan(cgImage, options: ScanOptions(formats: .qrCode))
```

Each `Barcode` has `text`, `format`, `polygon`, `rect`, `support`, `payloadBytes`,
`eanAddOn`, `gs1`, `readerInitialization`, `structuredAppend` and
`codabarStartStop` (Codabar start and stop characters such as `"AB"`, which
`text` omits); optional fields are `nil` when unavailable.

The class is named `BarcodeScanner` so that it does not clash with
`Foundation.Scanner`. Pixel buffers are read in place:

```swift
let result = try scanner.scan(pixels, width: 1280, height: 720, channels: 4) // [UInt8]
```

`channels` is 1 (gray), 3 (RGB) or 4 (RGBA, alpha ignored); pass `stride` for padded
rows. Every `scan` overload has an `inspect` twin.

In Swift concurrency, the array and `CGImage` overloads have `async` twins that
run on a detached task, so `await` keeps the main actor responsive. Swift picks
them in async code:

```swift
let result = try await scanner.scan(cgImage) // in a SwiftUI .task or other async code
```

Code that called `scan` inside an `async` function before these overloads existed
now needs `await`. Camera frames already arrive on the capture queue; scan them
synchronously there.

```swift
let inspection = try scanner.inspect(cgImage)
print(inspection.undecoded.count, inspection.elapsedMs, inspection.diagnostics)
```

## Errors

Calls throw `TapirscanError` with the library's `message`, the C `status` and a
`code` shared with the Python and JavaScript bindings: `.invalidInput`
(`"invalid_input"`), `.disposed` or `.engine`. No detection is an empty result, not
an error.

## Structured content

`parseContent(barcode)`, or `barcode.content`, returns the named fields of GS1,
Wi-Fi, contact, payment and similar payloads, or `nil` for plain text; it throws
if parsing fails. `parseContent(_:gs1:)` parses any text.

```swift
if let content = try barcode.content, content.kind == .wifi {
    connect(ssid: content["ssid"], password: content["password"])
}
```

Kinds and fields: [structured content](../../docs/CONTENT.md).

## Camera frames

`scan(_ pixelBuffer:)` reads bi-planar YUV frames (the AVFoundation default) in
place through their luma plane, and converts 32-bit BGRA frames to RGBA. Reuse one scanner for the session:

```swift
final class FrameScanner: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    private let scanner = try! BarcodeScanner(
        options: ScannerOptions(mode: .low, formats: [.ean13, .qrCode]))

    func captureOutput(_ output: AVCaptureOutput, didOutput sample: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        guard let frame = CMSampleBufferGetImageBuffer(sample),
              let result = try? scanner.scan(frame), !result.barcodes.isEmpty else { return }
        DispatchQueue.main.async { /* show result.barcodes */ }
    }
}
```

Set `alwaysDiscardsLateVideoFrames = true` on the video output so a slow scan skips
frames instead of queueing them, and use a 1280×720 or 1920×1080 session preset.

## Fast scanning

- Select only the formats you need; each enabled 2D format searches the whole image.
- Create one `BarcodeScanner` and reuse it; use `.low` for live camera frames.
- Camera luma planes are the cheapest camera input: they are read in place, with no
  color conversion; `CGImage`s are rendered once into a buffer the scanner reuses.
- Calls on one scanner run one at a time; use one scanner per queue to scan in
  parallel, with `parallel: false` when the queues already keep every core busy.

See [fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md).

## Testing

```sh
python3 scripts/build_apple.py
cd bindings/swift && swift test
```

The tests run on macOS. Build for iOS with
`swift build --triple arm64-apple-ios15.0 --sdk "$(xcrun --sdk iphoneos --show-sdk-path)"`.

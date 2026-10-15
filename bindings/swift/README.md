# Tapirscan for Swift (iOS and macOS)

A Swift package over the Tapirscan C library, for iOS 15+ and macOS 12+. It scans
pixel buffers, `CGImage`s and camera `CVPixelBuffer`s, and returns every decoded
barcode with its `text`, `format` and source-image `polygon`.

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

let scanner = try BarcodeScanner(formats: [.ean13, .qrCode])
let barcodes = try scanner.scan(cgImage)       // e.g. UIImage.cgImage
for barcode in barcodes {
    print(barcode.format, barcode.text, barcode.polygon)
}
```

The class is named `BarcodeScanner` so that it does not clash with
`Foundation.Scanner`. Pixel buffers are read in place:

```swift
let barcodes = try scanner.scan(pixels, width: 1280, height: 720, channels: 4) // [UInt8]
```

`channels` is 1 (gray), 3 (RGB) or 4 (RGBA, alpha ignored); pass `stride` for padded
rows. Errors are thrown as `TapirscanError` with the library's message.

## Structured content

`barcode.content` returns the named fields of GS1, Wi-Fi, contact, payment and
similar payloads; `parseContent(_:gs1:)` parses any text.

```swift
if let content = barcode.content, content.kind == .wifi {
    connect(ssid: content["ssid"], password: content["password"])
}
```

Kinds and fields: [structured content](../../docs/CONTENT.md).

## Camera frames

`scan(_ pixelBuffer:)` reads bi-planar YUV frames (the AVFoundation default) in
place through their luma plane, and converts 32-bit BGRA frames to RGBA. Reuse one scanner for the session:

```swift
final class FrameScanner: NSObject, AVCaptureVideoDataOutputSampleBufferDelegate {
    private let scanner = try! BarcodeScanner(mode: .low, formats: [.ean13, .qrCode])

    func captureOutput(_ output: AVCaptureOutput, didOutput sample: CMSampleBuffer,
                       from connection: AVCaptureConnection) {
        guard let frame = CMSampleBufferGetImageBuffer(sample),
              let barcodes = try? scanner.scan(frame), !barcodes.isEmpty else { return }
        DispatchQueue.main.async { /* show barcodes */ }
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
  parallel.

See [fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md)
for measured costs.

## Testing

```sh
python3 scripts/build_apple.py
cd bindings/swift && swift test
```

The tests run on macOS. Build for iOS with
`swift build --triple arm64-apple-ios15.0 --sdk "$(xcrun --sdk iphoneos --show-sdk-path)"`.

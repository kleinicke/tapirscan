// Swift interface to the Tapirscan C library.
import Foundation
import TapirscanC

#if canImport(CoreGraphics)
import CoreGraphics
#endif
#if canImport(CoreVideo)
import CoreVideo
#endif

/// Borrowed 8-bit pixels with their layout.
private struct Pixels {
    let buffer: UnsafeRawBufferPointer
    let width: Int
    let height: Int
    let channels: Int
    let stride: Int
}

/// Lends pixels to the native call for the duration of the call.
private typealias PixelSource = (_ call: (Pixels) throws -> tapirscan_result) throws -> tapirscan_result

/// A reusable barcode scanner (named apart from `Foundation.Scanner`). Create one and
/// reuse it for every frame; calls on one scanner run one at a time, and separate
/// scanners run concurrently.
public final class BarcodeScanner: @unchecked Sendable {
    public let options: ScannerOptions
    private let handle: tapirscan_scanner
    private let lock = NSLock()
    /// Reused RGBA conversion buffer for `CGImage` and BGRA inputs, guarded by `lock`.
    private var rgba: [UInt8] = []

    public init(options: ScannerOptions = ScannerOptions()) throws {
        var native = tapirscan_scanner_options(
            mode: options.mode.rawValue,
            formats: options.formats.rawValue,
            ean_add_on_policy: options.eanAddOnPolicy.rawValue,
            parallel: (options.parallel.map { $0 ? TAPIRSCAN_PARALLEL_ON : TAPIRSCAN_PARALLEL_OFF }
                ?? TAPIRSCAN_PARALLEL_DEFAULT).rawValue
        )
        var out = tapirscan_scanner()
        var error = tapirscan_error()
        let status = tapirscan_scanner_create(&native, &out, &error)
        guard status == 0 else { throw TapirscanError(status: status, error: &error) }
        self.options = options
        handle = out
    }

    deinit { tapirscan_scanner_destroy(handle) }

    // MARK: Pixels

    /// Scan 8-bit gray (1), RGB (3) or RGBA (4, alpha ignored) pixels in place.
    /// `stride` is bytes per row; 0 means packed rows. No detection is an empty result.
    public func scan(
        pixels: UnsafeRawBufferPointer,
        width: Int,
        height: Int,
        channels: Int,
        stride: Int = 0,
        options: ScanOptions = ScanOptions()
    ) throws -> ScanResult {
        try run(inspect: false, options, read: readScan) {
            try $0(Pixels(buffer: pixels, width: width, height: height, channels: channels, stride: stride))
        }
    }

    /// Scan pixels held in an array; the array is read in place.
    public func scan(
        _ pixels: [UInt8],
        width: Int,
        height: Int,
        channels: Int,
        stride: Int = 0,
        options: ScanOptions = ScanOptions()
    ) throws -> ScanResult {
        try pixels.withUnsafeBytes {
            try scan(pixels: $0, width: width, height: height, channels: channels,
                     stride: stride, options: options)
        }
    }

    /// Like `scan(pixels:width:height:channels:stride:options:)`, with unread regions and diagnostics.
    public func inspect(
        pixels: UnsafeRawBufferPointer,
        width: Int,
        height: Int,
        channels: Int,
        stride: Int = 0,
        options: ScanOptions = ScanOptions()
    ) throws -> InspectionResult {
        try run(inspect: true, options, read: readInspection) {
            try $0(Pixels(buffer: pixels, width: width, height: height, channels: channels, stride: stride))
        }
    }

    /// Like `scan(_:width:height:channels:stride:options:)`, with unread regions and diagnostics.
    public func inspect(
        _ pixels: [UInt8],
        width: Int,
        height: Int,
        channels: Int,
        stride: Int = 0,
        options: ScanOptions = ScanOptions()
    ) throws -> InspectionResult {
        try pixels.withUnsafeBytes {
            try inspect(pixels: $0, width: width, height: height, channels: channels,
                        stride: stride, options: options)
        }
    }

    // MARK: Native call

    /// Scan or inspect the pixels lent by `source` under the scanner lock, then copy
    /// the owned native result with `read`.
    private func run<Output>(
        inspect: Bool,
        _ options: ScanOptions,
        read: (tapirscan_result) throws -> Output,
        _ source: PixelSource
    ) throws -> Output {
        var native = tapirscan_scan_options(formats: try options.mask())
        lock.lock()
        let result: tapirscan_result
        do {
            defer { lock.unlock() }
            result = try source { pixels in
                var image = tapirscan_image(
                    data: pixels.buffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                    length: UInt64(pixels.buffer.count),
                    width: UInt64(clamping: pixels.width),
                    height: UInt64(clamping: pixels.height),
                    channels: UInt32(clamping: pixels.channels),
                    stride: UInt64(clamping: pixels.stride)
                )
                var result = tapirscan_result()
                var error = tapirscan_error()
                let status = inspect
                    ? tapirscan_inspect(handle, &image, &native, &result, &error)
                    : tapirscan_scan(handle, &image, &native, &result, &error)
                guard status == 0 else { throw TapirscanError(status: status, error: &error) }
                return result
            }
        }
        defer { tapirscan_result_destroy(result) }
        return try read(result)
    }

    /// Fill the reused RGBA buffer with `fill` and lend it to `call`; the caller holds the lock.
    private func withRGBA(
        width: Int,
        height: Int,
        fill: (UnsafeMutableRawBufferPointer) throws -> Void,
        _ call: (Pixels) throws -> tapirscan_result
    ) throws -> tapirscan_result {
        let count = width * height * 4
        if rgba.count != count { rgba = [UInt8](repeating: 0, count: count) }
        try rgba.withUnsafeMutableBytes(fill)
        return try rgba.withUnsafeBytes {
            try call(Pixels(buffer: $0, width: width, height: height, channels: 4, stride: 0))
        }
    }

    #if canImport(CoreGraphics)
    // MARK: CGImage

    /// Scan a `CGImage` (for example `UIImage.cgImage`), composited onto white.
    public func scan(_ image: CGImage, options: ScanOptions = ScanOptions()) throws -> ScanResult {
        try run(inspect: false, options, read: readScan) { try render(image, $0) }
    }

    /// Like `scan(_:options:)` for a `CGImage`, with unread regions and diagnostics.
    public func inspect(_ image: CGImage, options: ScanOptions = ScanOptions()) throws -> InspectionResult {
        try run(inspect: true, options, read: readInspection) { try render(image, $0) }
    }

    /// Render the image onto white in the RGBA buffer; the caller holds the lock.
    private func render(_ image: CGImage, _ call: (Pixels) throws -> tapirscan_result) throws -> tapirscan_result {
        let (width, height) = (image.width, image.height)
        return try withRGBA(width: width, height: height, fill: { buffer in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { throw TapirscanError.invalidInput("Could not render the CGImage") }
            context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
        }, call)
    }
    #endif

    #if canImport(CoreVideo)
    // MARK: Camera frames

    /// Scan a camera frame. Bi-planar YUV frames (the camera default) are read in
    /// place through their luma plane; 32-bit BGRA frames are converted to RGBA.
    public func scan(_ pixelBuffer: CVPixelBuffer, options: ScanOptions = ScanOptions()) throws -> ScanResult {
        try run(inspect: false, options, read: readScan) { try lend(pixelBuffer, $0) }
    }

    /// Like `scan(_:options:)` for a camera frame, with unread regions and diagnostics.
    public func inspect(_ pixelBuffer: CVPixelBuffer, options: ScanOptions = ScanOptions()) throws -> InspectionResult {
        try run(inspect: true, options, read: readInspection) { try lend(pixelBuffer, $0) }
    }

    /// Lock the frame and lend its luma plane or an RGBA copy; the caller holds the lock.
    private func lend(_ pixelBuffer: CVPixelBuffer, _ call: (Pixels) throws -> tapirscan_result) throws -> tapirscan_result {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }
        switch CVPixelBufferGetPixelFormatType(pixelBuffer) {
        case kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
             kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange:
            let width = CVPixelBufferGetWidthOfPlane(pixelBuffer, 0)
            let height = CVPixelBufferGetHeightOfPlane(pixelBuffer, 0)
            let stride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 0)
            guard let base = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 0) else {
                throw TapirscanError.invalidInput("The pixel buffer has no luma plane")
            }
            let plane = UnsafeRawBufferPointer(start: base, count: stride * height)
            return try call(Pixels(buffer: plane, width: width, height: height, channels: 1, stride: stride))
        case kCVPixelFormatType_32BGRA:
            let width = CVPixelBufferGetWidth(pixelBuffer)
            let height = CVPixelBufferGetHeight(pixelBuffer)
            let stride = CVPixelBufferGetBytesPerRow(pixelBuffer)
            guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else {
                throw TapirscanError.invalidInput("The pixel buffer has no pixels")
            }
            return try withRGBA(width: width, height: height, fill: { out in
                let source = base.assumingMemoryBound(to: UInt8.self)
                let target = out.baseAddress!.assumingMemoryBound(to: UInt8.self)
                for y in 0..<height {
                    let row = source + y * stride
                    let pixel = target + y * width * 4
                    for x in 0..<width {
                        pixel[x * 4] = row[x * 4 + 2]
                        pixel[x * 4 + 1] = row[x * 4 + 1]
                        pixel[x * 4 + 2] = row[x * 4]
                        pixel[x * 4 + 3] = 255
                    }
                }
            }, call)
        default:
            throw TapirscanError.invalidInput("Unsupported pixel format; use bi-planar YUV or 32-bit BGRA")
        }
    }
    #endif
}

// MARK: Reading results

private func readScan(_ result: tapirscan_result) throws -> ScanResult {
    ScanResult(barcodes: try readBarcodes(result))
}

private func points(_ polygon: (tapirscan_point, tapirscan_point, tapirscan_point, tapirscan_point)) -> [Point] {
    [polygon.0, polygon.1, polygon.2, polygon.3].map { Point(x: $0.x, y: $0.y) }
}

/// One variable-length barcode field including embedded NULs, or `nil` when absent.
private func copyField(_ result: tapirscan_result, _ index: UInt64, _ field: tapirscan_field, _ length: UInt64) throws -> [UInt8]? {
    if length == UInt64.max { return nil }
    var buffer = [UInt8](repeating: 0, count: Int(length) + 1)
    try check(tapirscan_result_copy(result, index, field.rawValue, &buffer, UInt64(buffer.count)))
    buffer.removeLast()
    return buffer
}

private func copyText(_ result: tapirscan_result, _ index: UInt64, _ field: tapirscan_field, _ length: UInt64) throws -> String? {
    try copyField(result, index, field, length).map { String(decoding: $0, as: UTF8.self) }
}

private func tristate(_ value: Int32) -> Bool? { value < 0 ? nil : value != 0 }

private func nativeFormat(_ bit: UInt32) throws -> Format {
    guard let format = Format(rawValue: bit) else { throw TapirscanError.unknown("format", bit) }
    return format
}

private func readBarcodes(_ result: tapirscan_result) throws -> [Barcode] {
    var count: UInt64 = 0
    try check(tapirscan_result_count(result, &count))
    var barcodes: [Barcode] = []
    barcodes.reserveCapacity(Int(count))
    for index in 0..<count {
        var barcode = tapirscan_barcode()
        try check(tapirscan_result_barcode(result, index, &barcode))
        var append: StructuredAppend?
        if barcode.structured_append_count != 0 {
            append = StructuredAppend(
                index: Int(clamping: barcode.structured_append_index),
                count: Int(clamping: barcode.structured_append_count),
                id: try copyText(result, index, TAPIRSCAN_FIELD_STRUCTURED_APPEND_ID, barcode.structured_append_id_length),
                parity: barcode.structured_append_parity < 0 ? nil : Int(barcode.structured_append_parity)
            )
        }
        barcodes.append(Barcode(
            text: try copyText(result, index, TAPIRSCAN_FIELD_TEXT, barcode.text_length) ?? "",
            format: try nativeFormat(barcode.format),
            polygon: points(barcode.polygon),
            support: barcode.support,
            payloadBytes: try copyField(result, index, TAPIRSCAN_FIELD_PAYLOAD_BYTES, barcode.payload_bytes_length),
            eanAddOn: try copyText(result, index, TAPIRSCAN_FIELD_EAN_ADD_ON, barcode.ean_add_on_length),
            gs1: tristate(barcode.gs1),
            readerInitialization: tristate(barcode.reader_initialization),
            structuredAppend: append
        ))
    }
    return barcodes
}

private func readInspection(_ result: tapirscan_result) throws -> InspectionResult {
    let barcodes = try readBarcodes(result)
    var summary = tapirscan_summary()
    try check(tapirscan_result_info(result, &summary))
    var undecoded: [UndecodedRegion] = []
    for index in 0..<summary.undecoded_count {
        var region = tapirscan_region()
        try check(tapirscan_result_undecoded(result, index, &region))
        undecoded.append(UndecodedRegion(
            format: region.format == 0 ? nil : try nativeFormat(region.format),
            polygon: points(region.polygon)
        ))
    }
    var length: UInt64 = 0
    try check(tapirscan_result_json_length(result, &length))
    var json = [UInt8](repeating: 0, count: Int(length) + 1)
    try check(tapirscan_result_copy_json(result, &json, UInt64(json.count)))
    guard let mode = Mode(rawValue: summary.mode) else { throw TapirscanError.unknown("mode", summary.mode) }
    return InspectionResult(
        barcodes: barcodes,
        undecoded: undecoded,
        width: Int(clamping: summary.width),
        height: Int(clamping: summary.height),
        mode: mode,
        elapsedMs: summary.elapsed_ms,
        diagnostics: String(decoding: json.dropLast(), as: UTF8.self)
    )
}

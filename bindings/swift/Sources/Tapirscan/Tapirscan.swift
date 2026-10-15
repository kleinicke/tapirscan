// Swift interface to the Tapirscan C library.
import Foundation
import TapirscanC

#if canImport(CoreGraphics)
import CoreGraphics
#endif
#if canImport(CoreVideo)
import CoreVideo
#endif

/// Effort mode; `medium` is the default.
public enum Mode: UInt32, Sendable {
    case low = 0, medium, high, veryHigh
}

/// Supplement policy for EAN/UPC two- and five-digit add-ons.
public enum EanAddOnPolicy: UInt32, Sendable {
    case ignore = 0, read, require
}

/// A set of barcode formats. Enable only the formats you need: each 2D format
/// searches the whole image.
public struct Formats: OptionSet, Sendable {
    public let rawValue: UInt32
    public init(rawValue: UInt32) { self.rawValue = rawValue }

    public static let ean13 = Formats(rawValue: 1)
    public static let upcA = Formats(rawValue: 2)
    public static let ean8 = Formats(rawValue: 4)
    public static let upcE = Formats(rawValue: 8)
    public static let code128 = Formats(rawValue: 16)
    public static let code39 = Formats(rawValue: 32)
    public static let itf = Formats(rawValue: 64)
    public static let codabar = Formats(rawValue: 128)
    public static let code93 = Formats(rawValue: 256)
    public static let qrCode = Formats(rawValue: 512)
    public static let dataMatrix = Formats(rawValue: 1024)
    public static let pdf417 = Formats(rawValue: 2048)
    public static let aztec = Formats(rawValue: 4096)
    public static let dataBar = Formats(rawValue: 8192)
    public static let dataBarExpanded = Formats(rawValue: 16384)
    public static let maxiCode = Formats(rawValue: 131_072)
    /// Italian pharmacy code, a Code 39 symbol reported as its 9-digit number. Opt-in: not in `all`.
    public static let code32 = Formats(rawValue: 262_144)
    /// German pharmacy number (PZN), a Code 39 symbol reported without the leading `-`. Opt-in: not in `all`.
    public static let pzn = Formats(rawValue: 524_288)
    /// MSI Plessey with a mod-10 check digit. Opt-in: not in `all`.
    public static let msiPlessey = Formats(rawValue: 1_048_576)

    /// EAN-13, UPC-A, EAN-8 and UPC-E (the default).
    public static let retail = Formats(rawValue: 15)
    /// Retail plus Code 128, Code 39 and ITF.
    public static let common1D = Formats(rawValue: 127)
    /// Common linear formats plus QR Code and Data Matrix.
    public static let common = Formats(rawValue: 1663)
    public static let linear = Formats(rawValue: 25087)
    public static let matrix = Formats(rawValue: 138_752)
    /// Every format except the opt-in `code32`, `pzn` and `msiPlessey`.
    public static let all = Formats(rawValue: 163_839)
}

/// A point in source-image pixels, origin top-left.
public struct Point: Equatable, Sendable {
    public let x: Double
    public let y: Double
}

/// One decoded barcode.
public struct Barcode: Sendable {
    public let text: String
    /// Format name such as `"EAN13"` or `"QRCode"`.
    public let format: String
    /// Four corners in source-image pixels.
    public let polygon: [Point]
    /// Uncalibrated, reader-specific ranking evidence.
    public let support: UInt64
    /// Whether the reader identified GS1 data; `nil` when the reader does not report it.
    public let gs1: Bool?

    /// Structured content of `text`, such as GS1 elements or a Wi-Fi network; `nil` for plain text.
    public var content: Content? { try? parseContent(text, gs1: gs1 ?? false) }
}

/// Kind of structured payload recognized by `parseContent`.
public enum ContentKind: UInt32, Sendable {
    case gs1 = 1, url, email, phone, sms, geo, wifi, contact, calendar, swissQRBill, epcPayment,
        hibc, vin, aamva

    /// Stable name shared with the other bindings, such as `"WiFi"`.
    public var name: String { String(cString: tapirscan_content_kind_name(rawValue)) }
}

/// One named value; names repeat where the payload repeats them.
public struct ContentField: Equatable, Sendable {
    /// GS1 application identifier such as `"01"`, otherwise a camelCase name such as `"ssid"`.
    public let name: String
    public let value: String
}

/// Structured content of decoded text, with fields in payload order.
public struct Content: Sendable {
    public let kind: ContentKind
    public let fields: [ContentField]

    /// First value of a field.
    public subscript(name: String) -> String? { fields.first { $0.name == name }?.value }

    /// Every value of a field, in payload order.
    public func all(_ name: String) -> [String] { fields.filter { $0.name == name }.map(\.value) }
}

/// Recognize GS1 element strings, URLs, Wi-Fi, contacts, payment slips, HIBC, VINs and
/// AAMVA licenses in decoded text. `gs1` parses the text as a GS1 element string.
/// Returns `nil` for text without recognized structure.
public func parseContent(_ text: String, gs1: Bool = false) throws -> Content? {
    let bytes = Array(text.utf8)
    var handle = tapirscan_content()
    var error = tapirscan_error()
    let status = tapirscan_parse_content(bytes, UInt64(bytes.count), gs1 ? 1 : 0, &handle, &error)
    guard status == 0 else { throw failure(status, &error) }
    guard handle != 0 else { return nil }
    defer { tapirscan_content_destroy(handle) }
    var summary = tapirscan_content_summary()
    tapirscan_content_info(handle, &summary)
    func copy(_ index: UInt64, _ part: UInt32, _ length: UInt64) -> String {
        var buffer = [UInt8](repeating: 0, count: Int(length) + 1)
        tapirscan_content_copy(handle, index, part, &buffer, UInt64(buffer.count))
        return String(decoding: buffer.dropLast(), as: UTF8.self)
    }
    var fields: [ContentField] = []
    for index in 0..<summary.field_count {
        var lengths = tapirscan_content_lengths()
        guard tapirscan_content_field(handle, index, &lengths) == 0 else { continue }
        fields.append(ContentField(
            name: copy(index, TAPIRSCAN_CONTENT_FIELD_NAME, lengths.name_length),
            value: copy(index, TAPIRSCAN_CONTENT_FIELD_VALUE, lengths.value_length)
        ))
    }
    guard let kind = ContentKind(rawValue: summary.kind) else { return nil }
    return Content(kind: kind, fields: fields)
}

/// A failed call: the C status and its detailed message.
public struct TapirscanError: Error, CustomStringConvertible, Sendable {
    public let status: Int32
    public let message: String
    public var description: String { message }
}

private func failure(_ status: Int32, _ error: inout tapirscan_error) -> TapirscanError {
    let detail = withUnsafeBytes(of: &error.message) { bytes in
        String(decoding: bytes.prefix { $0 != 0 }, as: UTF8.self)
    }
    let message = detail.isEmpty ? String(cString: tapirscan_status_message(status)) : detail
    return TapirscanError(status: status, message: message)
}

/// A reusable barcode scanner (named apart from `Foundation.Scanner`). Create one and reuse it for every frame; calls on one
/// scanner run one at a time, and separate scanners run concurrently.
public final class BarcodeScanner: @unchecked Sendable {
    private let handle: tapirscan_scanner
    private let lock = NSLock()
    /// Reused RGBA conversion buffer for `CGImage` and BGRA inputs.
    private var rgba: [UInt8] = []

    public init(
        mode: Mode = .medium,
        formats: Formats = .retail,
        eanAddOnPolicy: EanAddOnPolicy = .ignore
    ) throws {
        var options = tapirscan_scanner_options(
            mode: mode.rawValue,
            formats: formats.rawValue,
            ean_add_on_policy: eanAddOnPolicy.rawValue
        )
        var out = tapirscan_scanner()
        var error = tapirscan_error()
        let status = tapirscan_scanner_create(&options, &out, &error)
        guard status == 0 else { throw failure(status, &error) }
        handle = out
    }

    deinit { tapirscan_scanner_destroy(handle) }

    /// Scan 8-bit gray (1), RGB (3) or RGBA (4, alpha ignored) pixels in place.
    /// `stride` is bytes per row; 0 means packed rows.
    public func scan(
        pixels: UnsafeRawBufferPointer,
        width: Int,
        height: Int,
        channels: Int,
        stride: Int = 0,
        formats: Formats? = nil
    ) throws -> [Barcode] {
        lock.lock()
        defer { lock.unlock() }
        return try scanLocked(pixels, width, height, channels, stride, formats)
    }

    /// Scan pixels held in an array; the array is read in place.
    public func scan(
        _ pixels: [UInt8],
        width: Int,
        height: Int,
        channels: Int,
        stride: Int = 0,
        formats: Formats? = nil
    ) throws -> [Barcode] {
        try pixels.withUnsafeBytes {
            try scan(pixels: $0, width: width, height: height, channels: channels,
                     stride: stride, formats: formats)
        }
    }

    private func scanLocked(
        _ pixels: UnsafeRawBufferPointer,
        _ width: Int,
        _ height: Int,
        _ channels: Int,
        _ stride: Int,
        _ formats: Formats?
    ) throws -> [Barcode] {
        var image = tapirscan_image(
            data: pixels.baseAddress?.assumingMemoryBound(to: UInt8.self),
            length: UInt64(pixels.count),
            width: UInt64(clamping: width),
            height: UInt64(clamping: height),
            channels: UInt32(clamping: channels),
            stride: UInt64(clamping: stride)
        )
        var options = tapirscan_scan_options(formats: formats?.rawValue ?? 0)
        var result = tapirscan_result()
        var error = tapirscan_error()
        let status = tapirscan_scan(handle, &image, &options, &result, &error)
        guard status == 0 else { throw failure(status, &error) }
        defer { tapirscan_result_destroy(result) }
        var count: UInt64 = 0
        tapirscan_result_count(result, &count)
        var barcodes: [Barcode] = []
        barcodes.reserveCapacity(Int(count))
        for index in 0..<count {
            var barcode = tapirscan_barcode()
            guard tapirscan_result_barcode(result, index, &barcode) == 0 else { continue }
            var text = [UInt8](repeating: 0, count: Int(barcode.text_length) + 1)
            tapirscan_result_copy(result, index, 0, &text, UInt64(text.count))
            let corners = withUnsafeBytes(of: barcode.polygon) {
                Array($0.bindMemory(to: tapirscan_point.self))
            }
            barcodes.append(Barcode(
                text: String(decoding: text.dropLast(), as: UTF8.self),
                format: String(cString: tapirscan_format_name(barcode.format)),
                polygon: corners.map { Point(x: $0.x, y: $0.y) },
                support: barcode.support,
                gs1: barcode.gs1 < 0 ? nil : barcode.gs1 != 0
            ))
        }
        return barcodes
    }

    #if canImport(CoreGraphics)
    /// Scan a `CGImage` (for example `UIImage.cgImage`), composited onto white.
    public func scan(_ image: CGImage, formats: Formats? = nil) throws -> [Barcode] {
        lock.lock()
        defer { lock.unlock() }
        let (width, height) = (image.width, image.height)
        if rgba.count != width * height * 4 {
            rgba = [UInt8](repeating: 0, count: width * height * 4)
        }
        let drawn: Bool = rgba.withUnsafeMutableBytes { buffer in
            guard let context = CGContext(
                data: buffer.baseAddress, width: width, height: height,
                bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1))
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        guard drawn else {
            throw TapirscanError(status: 1, message: "Could not render the CGImage")
        }
        return try rgba.withUnsafeBytes {
            try scanLocked($0, width, height, 4, 0, formats)
        }
    }
    #endif

    #if canImport(CoreVideo)
    /// Scan a camera frame. Bi-planar YUV frames (the camera default) are read in
    /// place through their luma plane; 32-bit BGRA frames are converted to RGBA.
    public func scan(_ pixelBuffer: CVPixelBuffer, formats: Formats? = nil) throws -> [Barcode] {
        CVPixelBufferLockBaseAddress(pixelBuffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixelBuffer, .readOnly) }
        let format = CVPixelBufferGetPixelFormatType(pixelBuffer)
        switch format {
        case kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
             kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange:
            let width = CVPixelBufferGetWidthOfPlane(pixelBuffer, 0)
            let height = CVPixelBufferGetHeightOfPlane(pixelBuffer, 0)
            let stride = CVPixelBufferGetBytesPerRowOfPlane(pixelBuffer, 0)
            guard let base = CVPixelBufferGetBaseAddressOfPlane(pixelBuffer, 0) else {
                throw TapirscanError(status: 1, message: "The pixel buffer has no luma plane")
            }
            let plane = UnsafeRawBufferPointer(start: base, count: stride * height)
            return try scan(pixels: plane, width: width, height: height, channels: 1,
                            stride: stride, formats: formats)
        case kCVPixelFormatType_32BGRA:
            lock.lock()
            defer { lock.unlock() }
            let width = CVPixelBufferGetWidth(pixelBuffer)
            let height = CVPixelBufferGetHeight(pixelBuffer)
            let stride = CVPixelBufferGetBytesPerRow(pixelBuffer)
            guard let base = CVPixelBufferGetBaseAddress(pixelBuffer) else {
                throw TapirscanError(status: 1, message: "The pixel buffer has no pixels")
            }
            if rgba.count != width * height * 4 {
                rgba = [UInt8](repeating: 0, count: width * height * 4)
            }
            rgba.withUnsafeMutableBytes { out in
                let source = base.assumingMemoryBound(to: UInt8.self)
                for y in 0..<height {
                    let row = source + y * stride
                    let target = out.baseAddress!.assumingMemoryBound(to: UInt8.self) + y * width * 4
                    for x in 0..<width {
                        target[x * 4] = row[x * 4 + 2]
                        target[x * 4 + 1] = row[x * 4 + 1]
                        target[x * 4 + 2] = row[x * 4]
                        target[x * 4 + 3] = 255
                    }
                }
            }
            return try rgba.withUnsafeBytes {
                try scanLocked($0, width, height, 4, 0, formats)
            }
        default:
            throw TapirscanError(
                status: 1,
                message: "Unsupported pixel format; use bi-planar YUV or 32-bit BGRA"
            )
        }
    }
    #endif
}

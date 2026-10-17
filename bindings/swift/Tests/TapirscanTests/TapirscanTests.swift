import CoreGraphics
import CoreVideo
import TapirscanC
import XCTest
@testable import Tapirscan

/// A clean horizontal EAN-13 with three-pixel modules and a quiet zone, as gray pixels.
private func ean13(_ text: String) -> (pixels: [UInt8], width: Int, height: Int) {
    let l = ["0001101", "0011001", "0010011", "0111101", "0100011",
             "0110001", "0101111", "0111011", "0110111", "0001011"]
    let parity = ["LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG",
                  "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL", "LGGLGL"]
    let digits = text.compactMap { Int(String($0)) }
    let invert = { (bits: String) in String(bits.map { $0 == "0" ? "1" : "0" }) }
    var bits = "101"
    for (i, d) in digits[1...6].enumerated() {
        let pattern = parity[digits[0]][parity[digits[0]].index(parity[digits[0]].startIndex, offsetBy: i)]
        bits += pattern == "L" ? l[d] : String(invert(l[d]).reversed())
    }
    bits += "01010"
    for d in digits[7...12] { bits += invert(l[d]) }
    bits += "101"
    let module = 3, quiet = 12 * module, height = 120
    let width = bits.count * module + 2 * quiet
    var pixels = [UInt8](repeating: 255, count: width * height)
    for (i, bit) in bits.enumerated() where bit == "1" {
        for y in 10..<(height - 10) {
            for x in 0..<module { pixels[y * width + quiet + i * module + x] = 0 }
        }
    }
    return (pixels, width, height)
}

final class TapirscanTests: XCTestCase {
    let text = "4006381333931"

    func testScansGrayPixelsAndReusesTheScanner() throws {
        let (pixels, width, height) = ean13(text)
        let scanner = try BarcodeScanner(options: ScannerOptions(formats: [.ean13, .qrCode]))
        for _ in 0..<3 {
            let result = try scanner.scan(pixels, width: width, height: height, channels: 1)
            XCTAssertEqual(result.values, [text])
            let barcode = try XCTUnwrap(result.best)
            XCTAssertEqual(barcode.format, .ean13)
            XCTAssertEqual(barcode.polygon.count, 4)
            XCTAssertNil(barcode.eanAddOn)
            XCTAssertNil(barcode.structuredAppend)
            XCTAssertNil(try barcode.content)
            let rect = barcode.rect
            XCTAssertTrue(rect.left >= 0 && rect.top >= 0 && rect.width > 0 && rect.height > 0)
            XCTAssertTrue(rect.left + rect.width <= width && rect.top + rect.height <= height)
        }
        let blank = [UInt8](repeating: 255, count: width * height)
        let empty = try scanner.scan(blank, width: width, height: height, channels: 1)
        XCTAssertTrue(empty.barcodes.isEmpty)
        XCTAssertNil(empty.best)
    }

    func testPerCallFormatsOverrideTheScannerFormats() throws {
        let (pixels, width, height) = ean13(text)
        let scanner = try BarcodeScanner(options: ScannerOptions(formats: .qrCode))
        XCTAssertEqual(scanner.options.formats, .qrCode)
        XCTAssertTrue(try scanner.scan(pixels, width: width, height: height, channels: 1).barcodes.isEmpty)
        let override = ScanOptions(formats: .ean13)
        XCTAssertEqual(
            try scanner.scan(pixels, width: width, height: height, channels: 1, options: override).values,
            [text])
        // The override is per call: the scanner's own formats apply again afterwards.
        XCTAssertTrue(try scanner.scan(pixels, width: width, height: height, channels: 1).barcodes.isEmpty)
        XCTAssertThrowsError(try scanner.scan(
            pixels, width: width, height: height, channels: 1, options: ScanOptions(formats: []))
        ) { XCTAssertEqual(($0 as? TapirscanError)?.code, .invalidInput) }
    }

    func testParallelOptionLeavesResultsUnchanged() throws {
        let (pixels, width, height) = ean13(text)
        let formats: Formats = [.ean13, .qrCode, .dataMatrix]
        let reads = try [nil, false, true].map { parallel in
            try BarcodeScanner(options: ScannerOptions(formats: formats, parallel: parallel))
                .scan(pixels, width: width, height: height, channels: 1).values
        }
        XCTAssertEqual(reads, [[text], [text], [text]])
    }

    func testInspectionReportsTheSameBarcodesAndMetadata() throws {
        let (pixels, width, height) = ean13(text)
        let scanner = try BarcodeScanner(options: ScannerOptions(mode: .high))
        let inspection = try scanner.inspect(pixels, width: width, height: height, channels: 1)
        XCTAssertEqual(
            inspection.barcodes,
            try scanner.scan(pixels, width: width, height: height, channels: 1).barcodes)
        XCTAssertEqual(inspection.values, [text])
        XCTAssertEqual(inspection.best?.text, text)
        XCTAssertEqual(inspection.width, width)
        XCTAssertEqual(inspection.height, height)
        XCTAssertEqual(inspection.mode, .high)
        XCTAssertTrue(inspection.elapsedMs >= 0)
        XCTAssertTrue(inspection.diagnostics.hasPrefix("{"))
    }

    func testErrorsCarryTheStatusAndNamedCode() throws {
        let scanner = try BarcodeScanner()
        XCTAssertThrowsError(try scanner.scan([0, 0], width: 3, height: 3, channels: 1)) { error in
            let failure = error as? TapirscanError
            XCTAssertFalse(failure?.message.isEmpty ?? true)
            XCTAssertEqual(failure?.status, Int32(TAPIRSCAN_INVALID_ARGUMENT.rawValue))
            XCTAssertEqual(failure?.code, .invalidInput)
        }
        // Out-of-range sizes reach the library as errors instead of trapping.
        XCTAssertThrowsError(try scanner.scan([0, 0], width: 3, height: 3, channels: Int.max))
        XCTAssertThrowsError(try scanner.scan([0, 0], width: -3, height: 3, channels: 1))
        XCTAssertThrowsError(try BarcodeScanner(options: ScannerOptions(formats: []))) {
            XCTAssertEqual(($0 as? TapirscanError)?.code, .invalidInput)
        }
        XCTAssertEqual(TapirscanError(status: Int32(TAPIRSCAN_INVALID_HANDLE.rawValue)).code, .disposed)
        XCTAssertEqual(TapirscanError(status: Int32(TAPIRSCAN_INTERNAL_ERROR.rawValue)).code, .engine)
    }

    func testGeneratedFormatsMatchTheCHeader() {
        XCTAssertEqual(Formats.retail.rawValue, TAPIRSCAN_FORMATS_RETAIL)
        XCTAssertEqual(Formats.common1D.rawValue, TAPIRSCAN_FORMATS_COMMON_1D)
        XCTAssertEqual(Formats.common.rawValue, TAPIRSCAN_FORMATS_COMMON)
        XCTAssertEqual(Formats.linear.rawValue, TAPIRSCAN_FORMATS_LINEAR)
        XCTAssertEqual(Formats.matrix.rawValue, TAPIRSCAN_FORMATS_MATRIX)
        XCTAssertEqual(Formats.all.rawValue, TAPIRSCAN_FORMATS_ALL)
        for format in Format.allCases {
            XCTAssertEqual(format.name, String(cString: tapirscan_format_name(format.rawValue)))
        }
    }

    func testScansCGImages() throws {
        let (pixels, width, height) = ean13(text)
        let provider = CGDataProvider(data: Data(pixels) as CFData)!
        let image = CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 8,
            bytesPerRow: width, space: CGColorSpaceCreateDeviceGray(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.none.rawValue),
            provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent
        )!
        let scanner = try BarcodeScanner()
        XCTAssertEqual(try scanner.scan(image).values, [text])
        XCTAssertEqual(try scanner.inspect(image).values, [text])
    }

    func testScansCameraPixelBuffers() throws {
        let (pixels, width, height) = ean13(text)
        let scanner = try BarcodeScanner()
        for format in [kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVPixelFormatType_32BGRA] {
            var buffer: CVPixelBuffer?
            XCTAssertEqual(CVPixelBufferCreate(nil, width, height, format, nil, &buffer), kCVReturnSuccess)
            let frame = buffer!
            CVPixelBufferLockBaseAddress(frame, [])
            if format == kCVPixelFormatType_32BGRA {
                let stride = CVPixelBufferGetBytesPerRow(frame)
                let base = CVPixelBufferGetBaseAddress(frame)!.assumingMemoryBound(to: UInt8.self)
                for y in 0..<height {
                    for x in 0..<width {
                        let v = pixels[y * width + x], p = base + y * stride + x * 4
                        (p[0], p[1], p[2], p[3]) = (v, v, v, 255)
                    }
                }
            } else {
                let stride = CVPixelBufferGetBytesPerRowOfPlane(frame, 0)
                let base = CVPixelBufferGetBaseAddressOfPlane(frame, 0)!.assumingMemoryBound(to: UInt8.self)
                for y in 0..<height {
                    for x in 0..<width { base[y * stride + x] = pixels[y * width + x] }
                }
            }
            CVPixelBufferUnlockBaseAddress(frame, [])
            XCTAssertEqual(try scanner.scan(frame).values, [text], "format \(format)")
            XCTAssertEqual(try scanner.inspect(frame).values, [text], "format \(format)")
        }
    }

    func testParsesContent() throws {
        let wifi = try XCTUnwrap(try parseContent("WIFI:S:Café;T:WPA;P:secret;;"))
        XCTAssertEqual(wifi.kind, .wifi)
        XCTAssertEqual(wifi.kind.name, "WiFi")
        XCTAssertEqual(wifi["ssid"], "Café")
        XCTAssertNil(wifi["hidden"])
        XCTAssertEqual(try parseContent("0109506000134352", gs1: true)?.all("01"), ["09506000134352"])
        XCTAssertNil(try parseContent("0109506000134352"))
        let barcode = Barcode(
            text: "geo:47.37,8.54", format: .qrCode, polygon: [], support: 1, payloadBytes: nil,
            eanAddOn: nil, gs1: nil, readerInitialization: nil, structuredAppend: nil)
        XCTAssertEqual(try barcode.content?.kind, .geo)
    }

    func testContentKindNamesMatchTheNativeLibrary() {
        for kind in ContentKind.allCases {
            XCTAssertEqual(kind.name, String(cString: tapirscan_content_kind_name(kind.rawValue)))
        }
        XCTAssertNil(tapirscan_content_kind_name(0))
        XCTAssertNil(tapirscan_content_kind_name(UInt32(ContentKind.allCases.count + 1)))
    }
}

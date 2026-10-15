import CoreGraphics
import CoreVideo
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
        let scanner = try BarcodeScanner(formats: [.ean13, .qrCode])
        for _ in 0..<3 {
            let barcodes = try scanner.scan(pixels, width: width, height: height, channels: 1)
            XCTAssertEqual(barcodes.map(\.text), [text])
            XCTAssertEqual(barcodes.first?.format, "EAN13")
            XCTAssertEqual(barcodes.first?.polygon.count, 4)
        }
        let blank = [UInt8](repeating: 255, count: width * height)
        XCTAssertEqual(try scanner.scan(blank, width: width, height: height, channels: 1).count, 0)
    }

    func testRejectsInvalidImagesWithTheLibraryMessage() throws {
        let scanner = try BarcodeScanner()
        XCTAssertThrowsError(try scanner.scan([0, 0], width: 3, height: 3, channels: 1)) { error in
            XCTAssertFalse((error as? TapirscanError)?.message.isEmpty ?? true)
        }
        // Out-of-range sizes reach the library as errors instead of trapping.
        XCTAssertThrowsError(try scanner.scan([0, 0], width: 3, height: 3, channels: Int.max))
        XCTAssertThrowsError(try scanner.scan([0, 0], width: -3, height: 3, channels: 1))
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
        XCTAssertEqual(try BarcodeScanner().scan(image).map(\.text), [text])
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
            XCTAssertEqual(try scanner.scan(frame).map(\.text), [text], "format \(format)")
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
        let barcode = Barcode(text: "geo:47.37,8.54", format: "QRCode", polygon: [], support: 1, gs1: nil)
        XCTAssertEqual(barcode.content?.kind, .geo)
    }
}

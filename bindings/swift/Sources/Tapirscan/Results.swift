/// A point in source-image pixels, origin top-left, x rightward and y downward.
public struct Point: Equatable, Sendable {
    public let x: Double
    public let y: Double
}

/// Enclosing integer pixel bounds of a polygon.
public struct Rect: Equatable, Sendable {
    public let left: Int
    public let top: Int
    public let width: Int
    public let height: Int
}

/// Multipart sequence metadata. The index is one-based; sequences are not assembled.
public struct StructuredAppend: Equatable, Sendable {
    public let index: Int
    public let count: Int
    public let id: String?
    public let parity: Int?
}

/// A decoded physical instance. Equal values at distinct locations remain separate.
public struct Barcode: Equatable, Sendable {
    public let text: String
    public let format: Format
    /// Four corners in source-image pixels.
    public let polygon: [Point]
    /// Uncalibrated, reader-specific evidence used by `best`.
    public let support: UInt64
    /// Original decoded bytes where the reader reports them.
    public let payloadBytes: [UInt8]?
    /// The adjacent EAN/UPC supplement, when read.
    public let eanAddOn: String?
    /// Whether the reader identified GS1 data; `nil` when it does not report it.
    public let gs1: Bool?
    /// Whether the symbol programs the reader; `nil` when the reader does not report it.
    public let readerInitialization: Bool?
    public let structuredAppend: StructuredAppend?

    /// Enclosing integer pixel bounds, from floor(minimum) to ceil(maximum).
    public var rect: Rect {
        guard let minX = polygon.map(\.x).min(), let maxX = polygon.map(\.x).max(),
              let minY = polygon.map(\.y).min(), let maxY = polygon.map(\.y).max()
        else { return Rect(left: 0, top: 0, width: 0, height: 0) }
        let left = Int(minX.rounded(.down)), top = Int(minY.rounded(.down))
        return Rect(
            left: left, top: top,
            width: Int(maxX.rounded(.up)) - left, height: Int(maxY.rounded(.up)) - top
        )
    }

    /// Structured content of `text`, or `nil` for plain text. Throws what `parseContent` throws.
    public var content: Content? {
        get throws { try parseContent(text, gs1: gs1 ?? false) }
    }
}

/// A localized region without an accepted decode; a candidate, not a proven barcode.
public struct UndecodedRegion: Equatable, Sendable {
    public let format: Format?
    public let polygon: [Point]
}

private func highestSupport(_ barcodes: [Barcode]) -> Barcode? {
    var winner: Barcode?
    for barcode in barcodes {
        if let current = winner, barcode.support <= current.support { continue }
        winner = barcode
    }
    return winner
}

/// Decoded values and their source-image locations, valid after the scanner is released.
public struct ScanResult: Equatable, Sendable {
    public let barcodes: [Barcode]

    /// Decoded text in scanner order, including repeated values.
    public var values: [String] { barcodes.map(\.text) }

    /// Highest support, keeping the first read on ties; `nil` when nothing was decoded.
    public var best: Barcode? { highestSupport(barcodes) }
}

/// Scan output plus unread regions and diagnostics.
public struct InspectionResult: Equatable, Sendable {
    public let barcodes: [Barcode]
    /// Localized but unread regions.
    public let undecoded: [UndecodedRegion]
    /// Size of the supplied image in pixels.
    public let width: Int
    public let height: Int
    public let mode: Mode
    public let elapsedMs: Double
    /// Unstable engine diagnostics as JSON text.
    public let diagnostics: String

    public var values: [String] { barcodes.map(\.text) }
    public var best: Barcode? { highestSupport(barcodes) }
}

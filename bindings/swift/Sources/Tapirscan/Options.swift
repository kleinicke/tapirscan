/// Effort mode; `medium` is the default.
public enum Mode: UInt32, CaseIterable, Sendable, CustomStringConvertible {
    /// Least work, intended for clear images and inexpensive retries.
    case low = 0
    /// Default balance of work and recovery.
    case medium
    /// Additional effort and source-detail recovery.
    case high
    /// Highest available effort, including an additional localization grid.
    case veryHigh

    /// Stable name shared with the other language bindings, such as `"very-high"`.
    public var name: String {
        switch self {
        case .low: "low"
        case .medium: "medium"
        case .high: "high"
        case .veryHigh: "very-high"
        }
    }

    public var description: String { name }
}

/// Whether to read the adjacent two- or five-digit EAN/UPC supplement.
public enum EanAddOnPolicy: UInt32, Sendable {
    /// Decode the main retail value without searching for a supplement (default).
    case ignore = 0
    /// Read a supplement when possible; keep the main value if none is readable.
    case read
    /// Return retail reads only with a readable supplement; other formats are unaffected.
    case require
}

/// Scanner configuration, fixed when the scanner is created. Medium effort,
/// retail formats and ignored supplements by default.
public struct ScannerOptions: Equatable, Sendable {
    public var mode: Mode
    public var formats: Formats
    public var eanAddOnPolicy: EanAddOnPolicy

    public init(
        mode: Mode = .medium,
        formats: Formats = .retail,
        eanAddOnPolicy: EanAddOnPolicy = .ignore
    ) {
        self.mode = mode
        self.formats = formats
        self.eanAddOnPolicy = eanAddOnPolicy
    }
}

/// Optional overrides for one image.
public struct ScanOptions: Equatable, Sendable {
    /// Readers for this call; `nil` uses the scanner's formats. An empty set is an error.
    public var formats: Formats?

    public init(formats: Formats? = nil) {
        self.formats = formats
    }

    func mask() throws -> UInt32 {
        guard let formats else { return 0 }
        guard !formats.isEmpty else { throw TapirscanError.invalidInput("formats must not be empty") }
        return formats.rawValue
    }
}

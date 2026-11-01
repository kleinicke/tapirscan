/// Preview workflow selection. Single finishes on first acceptance; list finishes explicitly.
public enum ScanSessionMode: Sendable {
    case single
    case list
}

/// Accepted units of an exact format/text pair, not a physical-object count.
public struct ScanEntry: Equatable, Sendable {
    public let format: Format
    public let text: String
    public let quantity: UInt64
}

/// Preview acceptance state. Confine each instance to one actor or serial queue.
/// Owns no camera or native scanner. Returned arrays and entries are value snapshots.
public final class ScanSession {
    public let mode: ScanSessionMode
    public private(set) var finished = false
    public private(set) var entries: [ScanEntry] = []
    private struct Key: Equatable {
        let format: Format
        let text: String
    }
    private var history: [Key] = []

    public init(mode: ScanSessionMode = .single) { self.mode = mode }
    public var canUndo: Bool { !finished && !history.isEmpty }

    @discardableResult
    public func accept(_ barcode: Barcode, repeat: Bool = false) -> Bool {
        accept(format: barcode.format, text: barcode.text, repeat: `repeat`)
    }

    /// Explicit repeat acceptance adds one unit. Never enable it for every frame.
    /// Returns false for duplicates or a finished session. Quantity overflow is a precondition failure.
    @discardableResult
    public func accept(format: Format, text: String, repeat: Bool = false) -> Bool {
        guard !finished else { return false }
        if let index = entries.firstIndex(where: { $0.format == format && $0.text.utf8.elementsEqual(text.utf8) }) {
            guard `repeat` else { return false }
            let quantity = entries[index].quantity + 1
            precondition(quantity <= 9_007_199_254_740_991, "quantity exceeds cross-language safe integer range")
            entries[index] = ScanEntry(format: format, text: text, quantity: quantity)
        } else {
            entries.append(ScanEntry(format: format, text: text, quantity: 1))
        }
        history.append(Key(format: format, text: text))
        if mode == .single { finished = true }
        return true
    }

    /// Reverse the latest accepted unit; false when empty or finished.
    @discardableResult
    public func undo() -> Bool {
        guard canUndo, let key = history.popLast(),
              let index = entries.firstIndex(where: { $0.format == key.format && $0.text.utf8.elementsEqual(key.text.utf8) })
        else { return false }
        let entry = entries[index]
        if entry.quantity == 1 { entries.remove(at: index) }
        else { entries[index] = ScanEntry(format: entry.format, text: entry.text, quantity: entry.quantity - 1) }
        return true
    }

    /// Lock acceptance and return a value snapshot. Does not stop a camera.
    @discardableResult
    public func finish() -> [ScanEntry] { finished = true; return entries }

    /// Begin an empty session in the same mode; old snapshots stay unchanged.
    public func reset() { entries.removeAll(); history.removeAll(); finished = false }
}

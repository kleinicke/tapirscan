import TapirscanC

/// A failed call. `code` names the failure the same way the Python and
/// JavaScript bindings do; `status` is the C status.
public struct TapirscanError: Error, CustomStringConvertible, Sendable {
    public enum Code: String, Sendable {
        /// Invalid pixels, dimensions or options (status 1).
        case invalidInput = "invalid_input"
        /// A released handle (status 2).
        case disposed
        /// Any other native failure (statuses 3 and 4).
        case engine
    }

    public let code: Code
    /// Native status from `tapirscan.h`.
    public let status: Int32
    public let message: String

    public var description: String { "\(message) (status \(status))" }

    init(status: Int32, message: String? = nil) {
        self.status = status
        let detail = message ?? ""
        self.message = detail.isEmpty ? String(cString: tapirscan_status_message(status)) : detail
        switch status {
        case Int32(TAPIRSCAN_INVALID_ARGUMENT.rawValue): code = .invalidInput
        case Int32(TAPIRSCAN_INVALID_HANDLE.rawValue): code = .disposed
        default: code = .engine
        }
    }

    init(status: Int32, error: inout tapirscan_error) {
        let detail = withUnsafeBytes(of: &error.message) { bytes in
            String(decoding: bytes.prefix { $0 != 0 }, as: UTF8.self)
        }
        self.init(status: status, message: detail)
    }

    /// A failure detected before the call reaches the library.
    static func invalidInput(_ message: String) -> TapirscanError {
        TapirscanError(status: Int32(TAPIRSCAN_INVALID_ARGUMENT.rawValue), message: message)
    }

    /// A native value this binding does not know, such as a format added by a newer library.
    static func unknown(_ what: String, _ value: some CustomStringConvertible) -> TapirscanError {
        TapirscanError(
            status: Int32(TAPIRSCAN_INTERNAL_ERROR.rawValue), message: "Unknown native \(what) \(value)")
    }
}

func check(_ status: Int32) throws {
    if status != 0 { throw TapirscanError(status: status) }
}

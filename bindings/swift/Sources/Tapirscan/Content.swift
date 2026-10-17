import TapirscanC

/// One named value; names repeat where the payload repeats them.
public struct ContentField: Equatable, Sendable {
    /// GS1 application identifier such as `"01"`, otherwise a camelCase name such as `"ssid"`.
    public let name: String
    public let value: String
}

/// Structured content of decoded text, with fields in payload order.
public struct Content: Equatable, Sendable {
    public let kind: ContentKind
    public let fields: [ContentField]

    /// First value of a field.
    public subscript(name: String) -> String? { fields.first { $0.name == name }?.value }

    /// Every value of a field, in payload order.
    public func all(_ name: String) -> [String] { fields.filter { $0.name == name }.map(\.value) }
}

/// Structured content of decoded text, or `nil` for plain text. `gs1` parses the
/// text as a GS1 element string. See docs/CONTENT.md.
public func parseContent(_ text: String, gs1: Bool = false) throws -> Content? {
    let bytes = Array(text.utf8)
    var handle = tapirscan_content()
    var error = tapirscan_error()
    let status = tapirscan_parse_content(bytes, UInt64(bytes.count), gs1 ? 1 : 0, &handle, &error)
    guard status == 0 else { throw TapirscanError(status: status, error: &error) }
    guard handle != 0 else { return nil }
    defer { tapirscan_content_destroy(handle) }
    var summary = tapirscan_content_summary()
    try check(tapirscan_content_info(handle, &summary))
    guard let kind = ContentKind(rawValue: summary.kind) else {
        throw TapirscanError.unknown("content kind", summary.kind)
    }
    func copy(_ index: UInt64, _ part: UInt32, _ length: UInt64) throws -> String {
        var buffer = [UInt8](repeating: 0, count: Int(length) + 1)
        try check(tapirscan_content_copy(handle, index, part, &buffer, UInt64(buffer.count)))
        return String(decoding: buffer.dropLast(), as: UTF8.self)
    }
    var fields: [ContentField] = []
    for index in 0..<summary.field_count {
        var lengths = tapirscan_content_lengths()
        try check(tapirscan_content_field(handle, index, &lengths))
        fields.append(ContentField(
            name: try copy(index, TAPIRSCAN_CONTENT_FIELD_NAME, lengths.name_length),
            value: try copy(index, TAPIRSCAN_CONTENT_FIELD_VALUE, lengths.value_length)
        ))
    }
    return Content(kind: kind, fields: fields)
}

/// Structured content of a decoded barcode, using its text and GS1 flag.
public func parseContent(_ barcode: Barcode) throws -> Content? {
    try parseContent(barcode.text, gs1: barcode.gs1 ?? false)
}

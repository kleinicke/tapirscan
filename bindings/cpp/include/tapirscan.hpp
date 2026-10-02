// Tapirscan for C++17: a header-only wrapper over the native C ABI.
#pragma once
#include <tapirscan.h>
#include <tapirscan/format.hpp>

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace tapirscan {
static_assert(sizeof(void*) == 8, "Native ABI 5 requires a 64-bit target");

/// A native failure. `code` is the C status.
class Error : public std::runtime_error {
public:
    explicit Error(std::int32_t status)
        : std::runtime_error(tapirscan_status_message(status)), code(status) {}
    std::int32_t code;
};

/// Work effort. Medium is the default.
enum class Mode : std::uint32_t {
    Low = TAPIRSCAN_MODE_LOW,
    Medium = TAPIRSCAN_MODE_MEDIUM,
    High = TAPIRSCAN_MODE_HIGH,
    VeryHigh = TAPIRSCAN_MODE_VERY_HIGH,
};

/// Whether to read the adjacent two/five-digit EAN/UPC supplement.
enum class EanAddOnPolicy : std::uint32_t {
    Ignore = TAPIRSCAN_EAN_ADD_ON_IGNORE,
    Read = TAPIRSCAN_EAN_ADD_ON_READ,
    Require = TAPIRSCAN_EAN_ADD_ON_REQUIRE,
};

/// Stable name shared with the other bindings, such as "QRCode".
inline std::string_view to_string(Format format) {
    const char* name = tapirscan_format_name(static_cast<std::uint32_t>(format));
    return name ? name : "";
}

/// Stable mode name, such as "very-high".
inline std::string_view to_string(Mode mode) {
    switch (mode) {
    case Mode::Low: return "low";
    case Mode::Medium: return "medium";
    case Mode::High: return "high";
    case Mode::VeryHigh: return "very-high";
    }
    return "";
}

/// Nonempty format selection. Combine formats and presets with `|`.
class Formats {
    std::uint32_t bits_;
    constexpr explicit Formats(std::uint32_t bits) : bits_(bits) {}

public:
    constexpr Formats(Format format) : bits_(static_cast<std::uint32_t>(format)) {}
    /// EAN-13, UPC-A, EAN-8 and UPC-E.
    static constexpr Formats retail() { return Formats(TAPIRSCAN_FORMATS_RETAIL); }
    /// Retail formats plus Code 128, Code 39 and ITF.
    static constexpr Formats common_1d() { return Formats(TAPIRSCAN_FORMATS_COMMON_1D); }
    /// Common linear formats plus QR Code and Data Matrix.
    static constexpr Formats common() { return Formats(TAPIRSCAN_FORMATS_COMMON); }
    /// All supported linear formats.
    static constexpr Formats linear() { return Formats(TAPIRSCAN_FORMATS_LINEAR); }
    /// QR Code, Data Matrix, PDF417, Aztec and MaxiCode.
    static constexpr Formats matrix() { return Formats(TAPIRSCAN_FORMATS_MATRIX); }
    /// Every supported format. Formats outside Retail remain experimental.
    static constexpr Formats all() { return Formats(TAPIRSCAN_FORMATS_ALL); }
    /// A native TAPIRSCAN_FORMAT_* mask. Empty or unknown bits fail when scanning.
    static constexpr Formats from_bits(std::uint32_t bits) { return Formats(bits); }

    constexpr std::uint32_t bits() const { return bits_; }
    constexpr bool contains(Format format) const {
        return (bits_ & static_cast<std::uint32_t>(format)) != 0;
    }
    friend constexpr Formats operator|(Formats a, Formats b) { return Formats(a.bits_ | b.bits_); }
    friend constexpr bool operator==(Formats a, Formats b) { return a.bits_ == b.bits_; }
    friend constexpr bool operator!=(Formats a, Formats b) { return a.bits_ != b.bits_; }
};
constexpr Formats operator|(Format a, Format b) { return Formats(a) | Formats(b); }

/// Scanner configuration; per-image settings live in `ScanOptions`.
struct ScannerOptions {
    Mode mode = Mode::Medium;
    Formats formats = Formats::retail();
    EanAddOnPolicy ean_add_on_policy = EanAddOnPolicy::Ignore;
};

/// Borrowed gray8, RGB8 or RGBA8 pixels (alpha ignored). Pixels must outlive
/// the scan call only. `stride` is bytes per row; 0 means tightly packed.
struct Image {
    const std::uint8_t* data = nullptr;
    std::size_t length = 0;
    std::uint64_t width = 0;
    std::uint64_t height = 0;
    std::uint32_t channels = 1;
    std::uint64_t stride = 0;

    static Image gray(const std::uint8_t* data, std::size_t length, std::uint64_t width,
                      std::uint64_t height) {
        return {data, length, width, height, 1, 0};
    }
    static Image rgb(const std::uint8_t* data, std::size_t length, std::uint64_t width,
                     std::uint64_t height) {
        return {data, length, width, height, 3, 0};
    }
    static Image rgba(const std::uint8_t* data, std::size_t length, std::uint64_t width,
                      std::uint64_t height) {
        return {data, length, width, height, 4, 0};
    }
    static Image gray(const std::vector<std::uint8_t>& pixels, std::uint64_t width,
                      std::uint64_t height) {
        return gray(pixels.data(), pixels.size(), width, height);
    }
    static Image rgb(const std::vector<std::uint8_t>& pixels, std::uint64_t width,
                     std::uint64_t height) {
        return rgb(pixels.data(), pixels.size(), width, height);
    }
    static Image rgba(const std::vector<std::uint8_t>& pixels, std::uint64_t width,
                      std::uint64_t height) {
        return rgba(pixels.data(), pixels.size(), width, height);
    }
    Image with_stride(std::uint64_t bytes) const {
        Image image = *this;
        image.stride = bytes;
        return image;
    }
};

/// Optional overrides for one image.
struct ScanOptions {
    /// Readers for this call; empty uses the scanner's formats.
    std::optional<Formats> formats;
    /// Include unstable engine diagnostics as JSON in `ScanResult::debug`.
    bool debug = false;
    /// Allow reader-specific extra work. It is not a deadline or exhaustive search.
    bool extended_budget = false;
};

/// Source-image pixels, origin top-left, x rightward and y downward.
struct Point {
    double x = 0;
    double y = 0;
};
using Quad = std::array<Point, 4>;

/// Multipart sequence metadata. The index is one-based.
struct StructuredAppend {
    std::uint64_t index = 0;
    std::uint64_t count = 0;
    std::optional<std::string> id;
    std::optional<std::uint8_t> parity;
};

/// A decoded physical instance. Equal values at distinct locations remain separate.
struct Barcode {
    std::string text;
    Format format = Format::Ean13;
    Quad polygon{};
    /// Uncalibrated, reader-specific evidence; used by `ScanResult::best`.
    std::uint64_t support = 0;
    /// Original decoded bytes where the reader reports them.
    std::optional<std::vector<std::uint8_t>> payload_bytes;
    std::optional<std::string> ean_add_on;
    std::optional<bool> gs1;
    std::optional<bool> reader_initialization;
    std::optional<StructuredAppend> structured_append;

    /// Enclosing integer pixel bounds as {left, top, width, height}.
    std::array<double, 4> rect() const;
};

/// A localized region without an accepted decode; format is empty when unknown.
struct UndecodedRegion {
    std::optional<Format> format;
    Quad polygon{};
};

/// Owned scan output; it remains valid after its scanner is destroyed.
struct ScanResult {
    std::vector<Barcode> barcodes;
    /// Localized but unread regions. They are candidates, not proven barcodes.
    std::vector<UndecodedRegion> undecoded;
    std::uint64_t width = 0;
    std::uint64_t height = 0;
    Mode mode = Mode::Medium;
    double elapsed_ms = 0;
    /// The engine reported a work limit. False does not guarantee exhaustive scanning.
    bool unfinished = false;
    /// Engine diagnostics JSON, present only when `ScanOptions::debug` was set.
    std::optional<std::string> debug;
    std::optional<std::size_t> best_index;

    /// Highest support, keeping the first read on ties; null when empty.
    const Barcode* best() const { return best_index ? &barcodes[*best_index] : nullptr; }
    /// Decoded text of every barcode, in scanner order.
    std::vector<std::string> values() const {
        std::vector<std::string> values;
        values.reserve(barcodes.size());
        for (const auto& barcode : barcodes) values.push_back(barcode.text);
        return values;
    }
};

namespace detail {
inline void check(std::int32_t status) {
    if (status != TAPIRSCAN_OK) throw Error(status);
}
inline Quad quad(const tapirscan_point (&points)[4]) {
    return {Point{points[0].x, points[0].y}, Point{points[1].x, points[1].y},
            Point{points[2].x, points[2].y}, Point{points[3].x, points[3].y}};
}
inline std::optional<bool> tristate(std::int32_t value) {
    return value < 0 ? std::nullopt : std::optional<bool>(value != 0);
}
inline std::string copy(tapirscan_result result, std::uint64_t index, tapirscan_field field,
                        std::uint64_t length) {
    std::string value(static_cast<std::size_t>(length) + 1, '\0');
    check(tapirscan_result_copy(result, index, field,
                                reinterpret_cast<std::uint8_t*>(value.data()), value.size()));
    value.pop_back();
    return value;
}
inline std::optional<std::string> optional_copy(tapirscan_result result, std::uint64_t index,
                                                tapirscan_field field, std::uint64_t length) {
    if (length == TAPIRSCAN_ABSENT) return std::nullopt;
    return copy(result, index, field, length);
}

/// Owns a native result handle until its contents are copied.
class Result {
    tapirscan_result handle_;

public:
    explicit Result(tapirscan_result handle) : handle_(handle) {}
    ~Result() { tapirscan_result_destroy(handle_); }
    Result(const Result&) = delete;
    Result& operator=(const Result&) = delete;

    ScanResult read(bool debug) const {
        tapirscan_summary info{};
        check(tapirscan_result_info(handle_, &info));
        ScanResult result;
        result.width = info.width;
        result.height = info.height;
        result.mode = static_cast<Mode>(info.mode);
        result.elapsed_ms = info.elapsed_ms;
        result.unfinished = info.unfinished != 0;
        if (info.best_index >= 0) result.best_index = static_cast<std::size_t>(info.best_index);
        result.barcodes.reserve(static_cast<std::size_t>(info.barcode_count));
        for (std::uint64_t i = 0; i < info.barcode_count; ++i) {
            tapirscan_barcode b{};
            check(tapirscan_result_barcode(handle_, i, &b));
            Barcode barcode;
            barcode.text = copy(handle_, i, TAPIRSCAN_FIELD_TEXT, b.text_length);
            barcode.format = static_cast<Format>(b.format);
            barcode.polygon = quad(b.polygon);
            barcode.support = b.support;
            if (auto bytes = optional_copy(handle_, i, TAPIRSCAN_FIELD_PAYLOAD_BYTES,
                                           b.payload_bytes_length)) {
                barcode.payload_bytes.emplace(bytes->begin(), bytes->end());
            }
            barcode.ean_add_on =
                optional_copy(handle_, i, TAPIRSCAN_FIELD_EAN_ADD_ON, b.ean_add_on_length);
            barcode.gs1 = tristate(b.gs1);
            barcode.reader_initialization = tristate(b.reader_initialization);
            if (b.structured_append_count > 0) {
                StructuredAppend append;
                append.index = b.structured_append_index;
                append.count = b.structured_append_count;
                append.id = optional_copy(handle_, i, TAPIRSCAN_FIELD_STRUCTURED_APPEND_ID,
                                          b.structured_append_id_length);
                if (b.structured_append_parity >= 0) {
                    append.parity = static_cast<std::uint8_t>(b.structured_append_parity);
                }
                barcode.structured_append = std::move(append);
            }
            result.barcodes.push_back(std::move(barcode));
        }
        result.undecoded.reserve(static_cast<std::size_t>(info.undecoded_count));
        for (std::uint64_t i = 0; i < info.undecoded_count; ++i) {
            tapirscan_region r{};
            check(tapirscan_result_undecoded(handle_, i, &r));
            UndecodedRegion region;
            if (r.format != 0) region.format = static_cast<Format>(r.format);
            region.polygon = quad(r.polygon);
            result.undecoded.push_back(region);
        }
        if (debug) {
            std::string json(static_cast<std::size_t>(info.json_length) + 1, '\0');
            check(tapirscan_result_copy_json(handle_, reinterpret_cast<std::uint8_t*>(json.data()),
                                             json.size()));
            json.pop_back();
            result.debug = std::move(json);
        }
        return result;
    }
};
}  // namespace detail

inline std::array<double, 4> Barcode::rect() const {
    double left = polygon[0].x, top = polygon[0].y, right = left, bottom = top;
    for (const auto& p : polygon) {
        left = p.x < left ? p.x : left;
        top = p.y < top ? p.y : top;
        right = p.x > right ? p.x : right;
        bottom = p.y > bottom ? p.y : bottom;
    }
    return {std::floor(left), std::floor(top), std::ceil(right) - std::floor(left),
            std::ceil(bottom) - std::floor(top)};
}

/// A reusable scanner. Scans on one scanner serialize; separate scanners run
/// concurrently. Create once and reuse it across images.
class Scanner {
    tapirscan_scanner handle_ = 0;
    ScannerOptions options_;

public:
    explicit Scanner(ScannerOptions options = {}) : options_(options) {
        const auto abi = tapirscan_abi_version();
        if (abi != TAPIRSCAN_ABI_VERSION) {
            throw std::runtime_error("Native ABI mismatch: expected " +
                                     std::to_string(TAPIRSCAN_ABI_VERSION) + ", got " +
                                     std::to_string(abi) + ". Rebuild the native library.");
        }
        const tapirscan_scanner_options native{static_cast<std::uint32_t>(options.mode),
                                               options.formats.bits(),
                                               static_cast<std::uint32_t>(options.ean_add_on_policy)};
        detail::check(tapirscan_scanner_create(&native, &handle_));
    }
    ~Scanner() {
        if (handle_) tapirscan_scanner_destroy(handle_);
    }
    Scanner(const Scanner&) = delete;
    Scanner& operator=(const Scanner&) = delete;
    Scanner(Scanner&& other) noexcept
        : handle_(std::exchange(other.handle_, 0)), options_(other.options_) {}
    Scanner& operator=(Scanner&& other) noexcept {
        if (this != &other) {
            if (handle_) tapirscan_scanner_destroy(handle_);
            handle_ = std::exchange(other.handle_, 0);
            options_ = other.options_;
        }
        return *this;
    }

    const ScannerOptions& options() const noexcept { return options_; }

    /// Scan one image. No detection is a successful empty result.
    ScanResult scan(const Image& image, const ScanOptions& options = {}) const {
        if (!handle_) throw std::logic_error("Scanner was moved from");
        const tapirscan_image native_image{image.data,   image.length,   image.width,
                                           image.height, image.channels, image.stride};
        const tapirscan_scan_options native{options.formats ? options.formats->bits() : 0u,
                                            options.debug ? 1u : 0u,
                                            options.extended_budget ? 1u : 0u};
        tapirscan_result result = 0;
        detail::check(tapirscan_scan(handle_, &native_image, &native, &result));
        return detail::Result(result).read(options.debug);
    }
};

/// Scan one image with a temporary scanner. Reuse a `Scanner` for many images.
inline ScanResult scan(const Image& image, const ScannerOptions& scanner = {},
                       const ScanOptions& options = {}) {
    return Scanner(scanner).scan(image, options);
}
}  // namespace tapirscan

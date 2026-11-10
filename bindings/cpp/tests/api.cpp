#include <tapirscan.hpp>
#include <algorithm>
#include <cassert>
#include <string>
#include <type_traits>

template<class T, class = void> struct accepts_gray : std::false_type {};
template<class T> struct accepts_gray<T, std::void_t<decltype(tapirscan::Image::gray(std::declval<T>(), 3, 3))>> : std::true_type {};
template<class T, class = void> struct accepts_rgb : std::false_type {};
template<class T> struct accepts_rgb<T, std::void_t<decltype(tapirscan::Image::rgb(std::declval<T>(), 3, 3))>> : std::true_type {};
template<class T, class = void> struct accepts_rgba : std::false_type {};
template<class T> struct accepts_rgba<T, std::void_t<decltype(tapirscan::Image::rgba(std::declval<T>(), 3, 3))>> : std::true_type {};
using Pixels = std::vector<std::uint8_t>;
static_assert(accepts_gray<Pixels&>::value && accepts_rgb<const Pixels&>::value && accepts_rgba<Pixels&>::value);
static_assert(!accepts_gray<Pixels&&>::value && !accepts_rgb<Pixels&&>::value && !accepts_rgba<Pixels&&>::value);
static_assert(!accepts_gray<const Pixels&&>::value && !accepts_rgb<const Pixels&&>::value && !accepts_rgba<const Pixels&&>::value);

// best() returns a pointer into the result, so temporaries are rejected.
template<class T, class = void> struct result_best : std::false_type {};
template<class T> struct result_best<T, std::void_t<decltype(std::declval<T>().best())>> : std::true_type {};
static_assert(result_best<tapirscan::ScanResult&>::value && !result_best<tapirscan::ScanResult&&>::value);

int main() {
    tapirscan::ScanResult result;
    assert(!result.best());
    result.barcodes.resize(2);
    result.barcodes[0].support = 1;
    result.barcodes[1].support = 10;
    assert(result.best() == &result.barcodes[1]);
    result.barcodes[0].support = 20;
    assert(result.best() == &result.barcodes[0]);
    result.barcodes[1].support = 20;
    assert(result.best() == &result.barcodes[0]);
    result.barcodes.clear();
    assert(!result.best());
    for (std::uint64_t bits : {std::uint64_t{0}, std::uint64_t{0x8000}, std::uint64_t{1} << 40}) {
        try { (void)tapirscan::Formats::from_bits(bits); assert(false); }
        catch (const std::invalid_argument&) {}
    }
    assert(tapirscan::Formats::from_bits(0x80000000u).bits() == 0x80000000u);
    // Formats above bit 31 survive the mask, the native call and the result.
    constexpr std::uint64_t rm4scc = std::uint64_t{1} << 32;
    static_assert(static_cast<std::uint64_t>(tapirscan::Format::Rm4scc) == rm4scc);
    assert(tapirscan::to_string(tapirscan::Format::Rm4scc) == "RM4SCC");
    assert(tapirscan::Formats::from_bits(rm4scc | 1).bits() == (rm4scc | 1));
    assert(tapirscan::Formats(tapirscan::Format::Rm4scc).contains(tapirscan::Format::Rm4scc));
    {
        // RM4SCC "W1J0TR01" with its check character: ascender, descender, full or tracker bars.
        const std::string bars = "AAFDTTDAFADTFTTFFFDATFTADTTFFTDAFAFDTF";
        const std::size_t width = 12 * bars.size() + 40, height = 64;
        Pixels postal(width * height, 255);
        for (std::size_t i = 0; i < bars.size(); ++i) {
            const std::size_t top = bars[i] == 'A' || bars[i] == 'F' ? 12 : 26;
            const std::size_t bottom = bars[i] == 'D' || bars[i] == 'F' ? 52 : 38;
            for (std::size_t y = top; y < bottom; ++y)
                std::fill_n(postal.data() + y * width + 20 + 12 * i, 6, 0);
        }
        const auto symbol = tapirscan::Image::gray(postal, width, height);
        tapirscan::Scanner common;
        assert(common.scan(symbol).barcodes.empty());
        tapirscan::ScanOptions only;
        only.formats = tapirscan::Format::Rm4scc;
        const auto read = common.scan(symbol, only);
        assert(read.barcodes.size() == 1 && read.barcodes[0].text == "W1J0TR01");
        assert(read.barcodes[0].format == tapirscan::Format::Rm4scc);
    }
    assert(tapirscan::to_string(tapirscan::Format::RmQrCode) == "RMQRCode");
    Pixels pixels(9, 255);
    tapirscan::Scanner scanner;
    auto image = tapirscan::Image::gray(pixels, 3, 3);
    const auto barcodes = scanner.scan(image);
    assert(barcodes.barcodes.empty() && barcodes.values().empty() && !barcodes.best());
    static_assert(TAPIRSCAN_PARALLEL_DEFAULT == 0, "zeroed options must keep the default");
    const tapirscan_scanner_options zeroed{};
    assert(zeroed.parallel == TAPIRSCAN_PARALLEL_DEFAULT);
    for (const std::optional<bool> parallel : {std::optional<bool>(true), std::optional<bool>(false), std::optional<bool>()}) {
        tapirscan::ScannerOptions options;
        options.parallel = parallel;
        tapirscan::Scanner configured(options);
        assert(configured.scan(image).barcodes.empty());
    }
    image.length = 1;
    try { (void)scanner.scan(image); assert(false); }
    catch (const tapirscan::Error& error) {
        assert(error.code == TAPIRSCAN_INVALID_ARGUMENT);
        assert(std::string(error.what()).find("buffer") != std::string::npos);
    }

    const auto wifi = tapirscan::parse_content("WIFI:S:Cafe;T:WPA;P:secret;;");
    assert(wifi && wifi->kind == tapirscan::ContentKind::Wifi);
    assert(tapirscan::to_string(wifi->kind) == "WiFi");
    assert(wifi->fields.size() == 3 && wifi->get("password") == std::optional<std::string>("secret"));
    assert(!wifi->get("hidden"));
    tapirscan::Barcode gs1;
    gs1.text = "0109506000134352";
    assert(!tapirscan::parse_content(gs1));
    gs1.gs1 = true;
    const auto elements = tapirscan::parse_content(gs1);
    assert(elements && elements->kind == tapirscan::ContentKind::Gs1);
    assert(elements->get_all("01") == std::vector<std::string>{"09506000134352"});
    assert(!tapirscan::parse_content("plain text"));
    try { (void)tapirscan::parse_content(std::string("\xff")); assert(false); }
    catch (const tapirscan::Error& error) { assert(error.code == TAPIRSCAN_INVALID_ARGUMENT); }
}

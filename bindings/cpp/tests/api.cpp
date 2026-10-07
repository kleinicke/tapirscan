#include <tapirscan.hpp>
#include <cassert>
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

// best() returns a pointer into its argument, so temporaries are rejected.
template<class T, class = void> struct accepts_best : std::false_type {};
template<class T> struct accepts_best<T, std::void_t<decltype(tapirscan::best(std::declval<T>()))>> : std::true_type {};
template<class T, class = void> struct result_best : std::false_type {};
template<class T> struct result_best<T, std::void_t<decltype(std::declval<T>().best())>> : std::true_type {};
using Barcodes = std::vector<tapirscan::Barcode>;
static_assert(accepts_best<Barcodes&>::value && accepts_best<const Barcodes&>::value);
static_assert(!accepts_best<Barcodes&&>::value && !accepts_best<const Barcodes&&>::value);
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
    assert(tapirscan::best(result.barcodes) == result.best());
    result.barcodes.clear();
    assert(!result.best());
    for (auto bits : {0u, 0x80000000u}) {
        try { (void)tapirscan::Formats::from_bits(bits); assert(false); }
        catch (const std::invalid_argument&) {}
    }
    Pixels pixels(9, 255);
    tapirscan::Scanner scanner;
    auto image = tapirscan::Image::gray(pixels, 3, 3);
    const auto barcodes = scanner.scan(image);
    assert(barcodes.barcodes.empty() && barcodes.values().empty() && !barcodes.best());
    image.length = 1;
    try { (void)scanner.scan(image); assert(false); }
    catch (const tapirscan::Error& error) {
        assert(error.code == TAPIRSCAN_INVALID_ARGUMENT);
        assert(std::string(error.what()).find("buffer") != std::string::npos);
    }
}

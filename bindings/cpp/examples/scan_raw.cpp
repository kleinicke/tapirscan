// Scan a raw pixel file and print the typed result as JSON.
// usage: scan_raw mode width height channels stride pixels [debug formats]
#include <tapirscan.hpp>

#include <fstream>
#include <iomanip>
#include <iostream>
#include <iterator>
#include <sstream>
#include <string>

namespace {
std::string json_string(std::string_view text) {
    std::ostringstream out;
    out << '"';
    for (char raw : text) {
        auto c = static_cast<unsigned char>(raw);
        if (c == '"' || c == '\\') out << '\\' << raw;
        else if (c < 32) out << "\\u00" << std::hex << std::setw(2) << std::setfill('0') << +c << std::dec;
        else out << raw;
    }
    out << '"';
    return out.str();
}

std::string polygon(const tapirscan::Quad& quad) {
    std::ostringstream out;
    out << std::setprecision(17) << '[';
    for (std::size_t i = 0; i < quad.size(); ++i) {
        out << (i ? "," : "") << '[' << quad[i].x << ',' << quad[i].y << ']';
    }
    out << ']';
    return out.str();
}

tapirscan::Mode mode(std::string_view name) {
    if (name == "low") return tapirscan::Mode::Low;
    if (name == "high") return tapirscan::Mode::High;
    if (name == "very-high") return tapirscan::Mode::VeryHigh;
    return tapirscan::Mode::Medium;
}
}  // namespace

int main(int argc, char** argv) {
    if (argc != 7 && argc != 9) {
        std::cerr << "usage: scan_raw mode width height channels stride pixels [debug formats]\n";
        return 2;
    }
    try {
        std::ifstream file(argv[6], std::ios::binary);
        if (!file) throw std::runtime_error("Cannot read pixels");
        const std::vector<std::uint8_t> pixels((std::istreambuf_iterator<char>(file)), {});
        tapirscan::Image image = tapirscan::Image::gray(pixels, std::stoull(argv[2]), std::stoull(argv[3]));
        image.channels = static_cast<std::uint32_t>(std::stoul(argv[4]));
        image = image.with_stride(std::stoull(argv[5]));

        tapirscan::ScannerOptions scanner_options;
        scanner_options.mode = mode(argv[1]);
        tapirscan::ScanOptions options;
        if (argc == 9) {
            options.debug = std::string(argv[7]) == "1";
            options.formats = tapirscan::Formats::from_bits(static_cast<std::uint32_t>(std::stoul(argv[8])));
        }
        // Results own their data and survive the scanner.
        const auto result = tapirscan::Scanner(scanner_options).scan(image, options);

        std::cout << "{\"mode\":" << json_string(tapirscan::to_string(result.mode))
                  << ",\"unfinished\":" << (result.unfinished ? "true" : "false") << ",\"best\":";
        if (result.best()) std::cout << *result.best_index; else std::cout << "null";
        std::cout << ",\"barcodes\":[";
        for (std::size_t i = 0; i < result.barcodes.size(); ++i) {
            const auto& b = result.barcodes[i];
            std::cout << (i ? "," : "") << "{\"text\":" << json_string(b.text)
                      << ",\"format\":" << json_string(tapirscan::to_string(b.format))
                      << ",\"support\":" << b.support << ",\"polygon\":" << polygon(b.polygon) << '}';
        }
        std::cout << "],\"undecoded\":[";
        for (std::size_t i = 0; i < result.undecoded.size(); ++i) {
            const auto& r = result.undecoded[i];
            std::cout << (i ? "," : "") << "{\"format\":"
                      << json_string(r.format ? tapirscan::to_string(*r.format) : "Unknown")
                      << ",\"polygon\":" << polygon(r.polygon) << '}';
        }
        std::cout << "],\"debug\":" << result.debug.value_or("null") << "}\n";
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}

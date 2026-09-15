#pragma once
#include <tapirscan.h>
#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace tapirscan {
static_assert(sizeof(void*)==8, "Native ABI v3 requires a 64-bit target");
class Error : public std::runtime_error {
public:
    const int code;
    explicit Error(int status) : std::runtime_error("Barcode scanner error " + std::to_string(status)), code(status) {}
};
inline void check(int status) { if (status != BARCODE_OK) throw Error(status); }
struct ScanOptions { bool multiple = true; bool include_regions = false; std::uint32_t formats = 1; };
struct Barcode { std::string text; std::array<double,8> polygon; std::uint32_t support; std::string format; };
class Result {
    barcode_result handle_ = 0;
public:
    explicit Result(barcode_result handle) : handle_(handle) {}
    ~Result() { close(); }
    Result(const Result&) = delete;
    Result& operator=(const Result&) = delete;
    Result(Result&& other) noexcept : handle_(std::exchange(other.handle_,0)) {}
    Result& operator=(Result&& other) noexcept {
        if (this != &other) { close(); handle_ = std::exchange(other.handle_,0); }
        return *this;
    }
    void close() noexcept { if (handle_) barcode_result_destroy(std::exchange(handle_,0)); }
    barcode_result_metadata metadata() const {
        barcode_result_metadata value{}; check(barcode_result_info(handle_,&value)); return value;
    }
    std::string json() const {
        auto m = metadata(); std::string text(static_cast<std::size_t>(m.json_length)+1,'\0');
        check(barcode_result_copy_json(handle_,reinterpret_cast<std::uint8_t*>(text.data()),text.size()));
        text.pop_back(); return text;
    }
    std::vector<Barcode> barcodes() const {
        auto m = metadata(); std::vector<Barcode> reads;
        for(std::uint64_t i=0;i<m.barcode_count;i++) {
            barcode_read r{}; check(barcode_result_read(handle_,i,&r));
            std::string text(static_cast<std::size_t>(r.text_length)+1,'\0');
            check(barcode_result_copy_text(handle_,i,reinterpret_cast<std::uint8_t*>(text.data()),text.size()));
            text.pop_back();
            Barcode b{std::move(text),{},r.support,std::string(r.format)};
            for(std::size_t j=0;j<8;j++) b.polygon[j]=r.polygon[j];
            reads.push_back(std::move(b));
        }
        return reads;
    }
    std::optional<Barcode> best() const {
        std::optional<Barcode> best;
        for(auto& b:barcodes()) if(!best || b.support>best->support) best=std::move(b);
        return best;
    }
};
class Scanner {
    tapirscan_handle handle_ = 0;
public:
    Scanner() {
        if(barcode_abi_version()!=3) throw std::runtime_error("Unsupported scanner ABI");
        check(tapirscan_create(&handle_));
    }
    ~Scanner() { close(); }
    Scanner(const Scanner&) = delete;
    Scanner& operator=(const Scanner&) = delete;
    Scanner(Scanner&& other) noexcept : handle_(std::exchange(other.handle_,0)) {}
    Scanner& operator=(Scanner&& other) noexcept {
        if(this != &other) { close(); handle_=std::exchange(other.handle_,0); }
        return *this;
    }
    static const char* mode() { static const char* modes[] = {"low", "medium", "high", "very-high"}; return modes[barcode_mode()]; }
    void close() noexcept { if(handle_) tapirscan_destroy(std::exchange(handle_,0)); }
    Result scan(const std::uint8_t* pixels, std::size_t length, std::uint64_t width,
                std::uint64_t height, std::uint32_t channels, std::uint64_t stride, ScanOptions options = {}) {
        barcode_result result=0;
        auto flags = (options.multiple ? 0u : BARCODE_SINGLE) | (options.include_regions ? BARCODE_INCLUDE_REGIONS : 0u);
        check(barcode_scan_formats(handle_,pixels,length,width,height,channels,stride,flags,options.formats,&result));
        return Result(result);
    }
};
}

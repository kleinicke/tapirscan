#include <tapirscan/scanner.hpp>
#include <fstream>
#include <iostream>
#include <iomanip>
#include <iterator>
#include <cassert>
#include <limits>
#include <sstream>
std::string escape(const std::string& text) {
    std::ostringstream out;
    for(char raw:text) {
        auto c=static_cast<unsigned char>(raw);
        if(c=='"' || c=='\\') out << '\\' << raw;
        else if(c<32) out << "\\u00" << std::hex << std::setw(2) << std::setfill('0') << static_cast<unsigned>(c);
        else out << raw;
    }
    return out.str();
}
int main(int argc,char** argv) {
    if(argc!=6 && argc!=8 && argc!=9) { std::cerr << "usage: scan_raw width height channels stride pixels [multiple include_regions]\n"; return 2; }
    try {
        std::ifstream file(argv[5],std::ios::binary);
        if(!file) throw std::runtime_error("Cannot read image");
        std::vector<std::uint8_t> bytes((std::istreambuf_iterator<char>(file)),{});
        tapirscan::Scanner scanner;
        tapirscan::ScanOptions options;
        if(argc>=8) { options.multiple=std::string(argv[6])=="1"; options.include_regions=std::string(argv[7])=="1"; }
        if(argc==9) options.formats=static_cast<std::uint32_t>(std::stoul(argv[8]));
        const auto channels=std::stoul(argv[3]);
        if(channels>std::numeric_limits<std::uint32_t>::max()) throw std::out_of_range("channels");
        auto result=scanner.scan(bytes.data(),bytes.size(),std::stoull(argv[1]),std::stoull(argv[2]),static_cast<std::uint32_t>(channels),std::stoull(argv[4]),options);
        auto moved=std::move(result);
        scanner.close();
        for(const auto& b:moved.barcodes()) assert(!b.format.empty());
        auto best=moved.best();
        if(best) assert(!best->format.empty());
        std::cout << std::setprecision(17) << "{\"result\":" << moved.json() << ",\"typed\":[";
        bool first=true;
        for(const auto& b:moved.barcodes()) {
            if(!first) std::cout << ','; first=false;
            std::cout << "{\"text\":\"" << escape(b.text) << "\",\"support\":" << b.support << ",\"polygon\":[";
            for(std::size_t i=0;i<4;i++) { if(i) std::cout << ','; std::cout << '[' << b.polygon[2*i] << ',' << b.polygon[2*i+1] << ']'; }
            std::cout << "]}";
        }
        std::cout << "]}\n";
        moved.close();moved.close();
    } catch(const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
}

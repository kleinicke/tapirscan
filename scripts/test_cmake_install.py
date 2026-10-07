#!/usr/bin/env python3
"""Install, relocate and consume the C and C++ packages through find_package."""

import json
import subprocess
import tempfile
from pathlib import Path

from build import ROOT

CONSUMER = """#include <tapirscan.hpp>
#include <iostream>
#include <vector>
int main() {
    const std::vector<std::uint8_t> pixels(64 * 64, 255);
    const tapirscan::Mode modes[] = {tapirscan::Mode::Low, tapirscan::Mode::Medium,
                                     tapirscan::Mode::High, tapirscan::Mode::VeryHigh};
    std::cout << '[';
    for (auto mode : modes) {
        tapirscan::ScannerOptions options;
        options.mode = mode;
        const auto image = tapirscan::Image::gray(pixels, 64, 64);
        const auto result = tapirscan::inspect(image, options);
        if (!result.barcodes.empty()) return 1;
        std::cout << (mode == tapirscan::Mode::Low ? "" : ",") << '"'
                  << tapirscan::to_string(result.mode) << '"';
    }
    std::cout << "]\\n";
}
"""

C_CONSUMER = """#include <tapirscan.h>
#include <stdint.h>
#include <string.h>
int main(void) {
    uint8_t pixels[64 * 64];
    memset(pixels, 255, sizeof pixels);
    tapirscan_scanner scanner;
    if (tapirscan_scanner_create(NULL, &scanner, NULL) != TAPIRSCAN_OK) return 1;
    tapirscan_image image = {pixels, sizeof pixels, 64, 64, 1, 0};
    tapirscan_result result;
    uint64_t count = 1;
    if (tapirscan_scan(scanner, &image, NULL, &result, NULL) != TAPIRSCAN_OK) return 2;
    tapirscan_result_count(result, &count);
    tapirscan_result_destroy(result);
    tapirscan_scanner_destroy(scanner);
    return count == 0 ? 0 : 3;
}
"""


def run(*args: object) -> None:
    """Run a checked local command."""
    subprocess.run(list(map(str, args)), check=True)


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="tapirscan-cmake-install-") as temporary:
        temp = Path(temporary)
        prefix = temp / "original"
        moved = temp / "relocated"
        run("cmake", "--install", ROOT / "build/cpp", "--prefix", prefix)
        prefix.rename(moved)
        source = temp / "consumer"
        source.mkdir()
        (source / "CMakeLists.txt").write_text("""cmake_minimum_required(VERSION 3.20)
project(Consumer LANGUAGES C CXX)
find_package(Tapirscan CONFIG REQUIRED)
add_executable(consumer main.cpp)
target_link_libraries(consumer PRIVATE tapirscan::cpp)
add_executable(c_consumer main.c)
target_link_libraries(c_consumer PRIVATE tapirscan::native)
""")
        (source / "main.cpp").write_text(CONSUMER)
        (source / "main.c").write_text(C_CONSUMER)
        run("cmake", "-S", source, "-B", temp / "build", f"-DCMAKE_PREFIX_PATH={moved}")
        run("cmake", "--build", temp / "build")
        run(temp / "build/c_consumer")
        notices = moved / "share/licenses/tapirscan/THIRD_PARTY_NOTICES.md"
        if not notices.is_file():
            msg = "The native installation is missing the third-party notices"
            raise AssertionError(msg)
        output = subprocess.check_output([str(temp / "build/consumer")], text=True)
        if json.loads(output) != ["low", "medium", "high", "very-high"]:
            msg = f"Installed consumer returned unexpected modes: {output}"
            raise AssertionError(msg)
    print("The relocated C and C++ packages scanned; notices are installed.")

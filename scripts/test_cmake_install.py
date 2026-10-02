#!/usr/bin/env python3
"""Install, relocate and consume the C++ package through find_package."""

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
        const auto result = tapirscan::scan(image, options);
        if (!result.barcodes.empty()) return 1;
        std::cout << (mode == tapirscan::Mode::Low ? "" : ",") << '"'
                  << tapirscan::to_string(result.mode) << '"';
    }
    std::cout << "]\\n";
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
project(Consumer LANGUAGES CXX)
find_package(Tapirscan CONFIG REQUIRED)
add_executable(consumer main.cpp)
target_link_libraries(consumer PRIVATE tapirscan::cpp)
""")
        (source / "main.cpp").write_text(CONSUMER)
        run("cmake", "-S", source, "-B", temp / "build", f"-DCMAKE_PREFIX_PATH={moved}")
        run("cmake", "--build", temp / "build")
        output = subprocess.check_output([str(temp / "build/consumer")], text=True)
        if json.loads(output) != ["low", "medium", "high", "very-high"]:
            msg = f"Installed consumer returned unexpected modes: {output}"
            raise AssertionError(msg)
    print("The relocated C++ package scanned in all four modes.")

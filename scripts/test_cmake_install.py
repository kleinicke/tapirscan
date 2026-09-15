#!/usr/bin/env python3
"""Install, relocate and consume the C++ package through find_package."""

import json
import subprocess
import tempfile
from pathlib import Path

from build import ROOT


def run(*args: object) -> None:
    """Run a checked local command and return its decoded output."""
    subprocess.run(list(map(str, args)), check=True)


if __name__ == "__main__":
    for mode in ("low", "medium", "high", "very-high"):
        with tempfile.TemporaryDirectory(prefix="barcode-cmake-install-") as temporary:
            temp = Path(temporary)
            prefix = temp / "original"
            moved = temp / "relocated"
            run("cmake", "--install", ROOT / f"build/cpp-{mode}", "--prefix", prefix)
            prefix.rename(moved)
            source = temp / "consumer"
            source.mkdir()
            (
                source / "CMakeLists.txt"
            ).write_text("""cmake_minimum_required(VERSION 3.20)
project(Consumer LANGUAGES CXX)
find_package(Tapirscan CONFIG REQUIRED)
add_executable(consumer main.cpp)
target_link_libraries(consumer PRIVATE tapirscan::cpp)
""")
            (source / "main.cpp").write_text("""#include <tapirscan/scanner.hpp>
#include <iostream>
#include <vector>
int main() {
    std::vector<std::uint8_t> pixels(4096,255);
    tapirscan::Scanner scanner;
    auto result=scanner.scan(pixels.data(),pixels.size(),64,64,1,64);
    scanner.close();
    if(!result.barcodes().empty()) return 1;
    std::cout << result.json();
}
""")
            run(
                "cmake",
                "-S",
                source,
                "-B",
                temp / "consumer-build",
                f"-DCMAKE_PREFIX_PATH={moved}",
            )
            run("cmake", "--build", temp / "consumer-build")
            output = subprocess.check_output(
                [str(temp / "consumer-build/consumer")], text=True
            )
            result = json.loads(output)
            if result["mode"] != mode or result["scan"]["barcodes"] != []:
                msg = "Installed consumer returned an unexpected mode or barcode"
                raise AssertionError(msg)
    print("All four installed C++ modes loaded successfully after relocation.")

# C++17 binding

The header-only wrapper owns scanner and result handles using RAII. Both types are
move-only. `Result::barcodes()` returns typed values, `best()` applies support-based
ranking, and `json()` includes region evidence when requested before scanning.
Results remain valid after the scanner closes. Native status codes become `Error`.

```sh
python3 scripts/build_native.py low medium high very-high
cmake -S bindings/cpp -B build/cpp-medium -DBARCODE_MODE=medium
cmake --build build/cpp-medium
ctest --test-dir build/cpp-medium --output-on-failure
cmake --install build/cpp-medium --prefix /your/install/prefix
```

Use a different build/install directory for each mode. Each application links
one mode; do not link multiple implementations' identical C symbols together.
After installing, a consumer can use:

```cmake
find_package(Tapirscan CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE tapirscan::cpp)
```

```cpp
#include <tapirscan/scanner.hpp>
tapirscan::Scanner scanner;
auto result = scanner.scan(pixels.data(), pixels.size(), width, height, 4, stride,
    tapirscan::ScanOptions{true, false, 1u | 16u});
for (const auto& barcode : result.barcodes()) {
    // barcode.text, barcode.format, barcode.polygon (x0,y0,...,x3,y3)
}
auto best = result.best();
```

`close()` is idempotent; destructors close automatically. Synchronize C++ object
moves/close against other calls. Input pixels must remain valid and immutable during
scanning. All four installed mode packages are tested after moving their installation
prefix. A vcpkg/Conan recipe and Windows validation remain release work.

`ScanOptions{multiple, include_regions, formats}` defaults to `{true, false, 1}`. Select
`{false, true}` for one highest-support result plus all region evidence. One-result
selection follows the full scan, preserving normal scan effort. Typed `barcodes()`
returns a vector in either mode; it is empty when no barcode decoded. With regions
disabled, JSON omits localization, search windows and candidate evidence. The
native ABI is now version 4; rebuild clients and libraries together.

## API reference

The example assumes decoded RGBA bytes in `pixels`; dimensions are in pixels and
stride is bytes between row starts. It selects EAN13 and Code128 (`1u | 16u`).
See [format bits](../../docs/FORMATS.md); omit options for EAN13 defaults.

| Call                                                                          | Purpose                                                               |
| ----------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `Scanner()`                                                                   | Create a scanner in the linked mode.                                  |
| `Scanner::mode()`                                                             | Name of that mode.                                                    |
| `scanner.scan(pixels, length, width, height, channels, stride, options = {})` | Synchronous scan; channels 1/3/4 mean gray/RGB/RGBA. Alpha ignored.   |
| `result.barcodes()`                                                           | Copy typed reads into a vector.                                       |
| `result.best()`                                                               | Return the highest-support read as an optional.                       |
| `result.metadata()`                                                           | Native count, JSON length, unfinished and localization-limited flags. |
| `result.json()`                                                               | Copy the shared JSON result, including requested diagnostics.         |
| `scanner.close()`, `result.close()`                                           | Release their respective handles; destructors normally do this.       |

Barcode fields are `text`, `format`, `polygon` and `support`. Polygons use input
coordinates; support is not a probability. Repeated `barcodes()`/`best()` calls
copy reads again. Store the returned vector when reusing it. There are no image
loading, values-only or rectangle helpers. Decode image files before scanning.

The CMake install is version 1.1.0 and includes the matching ABI-4 library. Effort
is selected by linking one mode, unlike Python/Java's runtime selection. CMake
installation works locally; Conan/vcpkg distribution is not yet provided.

## Finishing candidate work

Enable `ScanOptions.finish_candidates = true` to remove shared frame retry and association budgets
for EAN13/UPC-A candidates. Default is disabled; selected formats must include
EAN13 or UPCA. Per-candidate effort and other limits remain; unfinished work is
still reported. See [API design](../../docs/API_DESIGN.md) for scope and cost.

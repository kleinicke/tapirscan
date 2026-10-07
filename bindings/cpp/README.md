# Tapirscan for C++

`scan(image)` returns `ScanResult`, with decoded text, format and
source-image polygons. Use `result.values()` for strings and `result.barcodes`
for located reads; both are empty when nothing was decoded.

```cpp
auto result = tapirscan::scan(tapirscan::Image::gray(pixels, width, height));
for (const auto& barcode : result.barcodes) std::cout << barcode.text << '\n';
```

Reuse `Scanner::scan` across images. Call `inspect` for an `InspectionResult` with unread
regions, work status, timing and diagnostics. There is no debug flag.

Both operations accept decoded pixels and return source-image barcode geometry.
Inspection also includes undecoded proposals and reported work limits. Defaults are Medium effort and
retail formats (EAN13, UPCA, EAN8 and UPCE). The header-only C++17 wrapper uses
one shared library that contains all four effort modes.

```cpp
#include <tapirscan.hpp>
#include <iostream>

std::vector<std::uint8_t> pixels(640 * 480, 255);
auto result = tapirscan::inspect(tapirscan::Image::gray(pixels, 640, 480));
for (const auto& barcode : result.barcodes) {
    std::cout << barcode.text << ' ' << tapirscan::to_string(barcode.format) << '\n';
}
std::cout << result.undecoded.size() << " undecoded; unfinished: " << result.unfinished << '\n';
```

No detection is an empty `barcodes` vector. Invalid input and engine failures
throw `tapirscan::Error`, whose `code` is the native status. Results are owned
values and survive the scanner. Equal payloads at distinct locations remain
separate physical instances.

## Reuse and configuration

```cpp
tapirscan::ScannerOptions options;
options.mode = tapirscan::Mode::High;
options.formats = tapirscan::Format::Ean13 | tapirscan::Format::QrCode;
tapirscan::Scanner scanner(options);

tapirscan::ScanOptions scan;
scan.extended_budget = true;
const auto barcodes = scanner.scan(tapirscan::Image::rgba(pixels, width, height), scan);
if (const auto* best = tapirscan::best(barcodes)) std::cout << best->text << '\n';
```

Reuse a scanner across images; it is move-only and its destructor releases it.
Scans on one scanner serialize; separate scanners run concurrently.
`tapirscan::inspect(image, options)` creates a temporary scanner for one image.

| Scanner option      | Default                  | Choices                                     |
| ------------------- | ------------------------ | ------------------------------------------- |
| `mode`              | `Mode::Medium`           | `Low`, `Medium`, `High`, `VeryHigh`         |
| `formats`           | `Formats::retail()`      | One format, combinations with `\|`, presets |
| `ean_add_on_policy` | `EanAddOnPolicy::Ignore` | `Ignore`, `Read`, `Require`                 |

`Formats::from_bits` rejects empty or unknown bits with `std::invalid_argument`.

Presets: `Formats::retail()`, `common_1d()`, `common()`, `linear()`, `matrix()`
and `all()`. Retail formats are enabled by default; additional formats are supported when selected.
See [format coverage](../../docs/FORMATS.md).

| Per-scan option   | Default        | Meaning                                |
| ----------------- | -------------- | -------------------------------------- |
| `formats`         | `std::nullopt` | Override readers for this call         |
| `extended_budget` | `false`        | Allow extra reader work for any format |

`extended_budget` can cost more time and does not promise exhaustive decoding;
see [API design](../../docs/API_DESIGN.md).

## Results

`ScanResult` provides decoded barcodes, `values()` and `best()`.
No detections produce empty collections; `best()` returns the language’s empty value.

`InspectionResult` exposes `barcodes`, `undecoded`, `width`, `height`, `mode`,
`elapsed_ms`, `unfinished` and `diagnostics` JSON. `values()` returns decoded
text. `tapirscan::best(barcodes)` points at the largest-support read of any
barcode vector, keeping first-read ties, or is null; `InspectionResult::best()` is the
same for inspection results. Both reject temporaries, whose pointer would
dangle. Support is reader-specific and not comparable confidence across formats.

`Barcode` contains `text`, `format`, `polygon` (four `Point`s in source-image
pixels, top-left origin), `support`, and optional `payload_bytes`, `ean_add_on`,
`gs1`, `reader_initialization` and `structured_append`. An empty optional means
the reader did not report it. `rect()` returns `{left, top, width, height}`
enclosing integer pixel bounds. `to_string(format)` gives names such as `"QRCode"`.

`undecoded` contains localized proposals without accepted decodes. These can be
false candidates or deferred work; an empty list and `unfinished == false` do
not guarantee exhaustive coverage. Debug JSON schemas are unstable.

## Images

`Image::gray`, `Image::rgb` and `Image::rgba` take a `std::vector<std::uint8_t>`
or a pointer and length, plus width and height; `.with_stride(bytes_per_row)`
describes padded rows. Alpha is ignored. Keep the backing buffer alive and do not reallocate it until the scan call
returns. Image factories reject temporary vectors. Images are at least 3×3 and at most 32 megapixels. The addressed
layout, `(height - 1) * stride + width * channels` bytes, must fit in the buffer and in 128 MiB; a larger backing
buffer, such as a frame around a crop, is accepted. Decode
image files and convert BGR, planar, float or 16-bit pixels before scanning.

## Building and installing

```sh
python3 scripts/build_native.py
cmake -S bindings/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
cmake --install build/cpp --prefix /your/install/prefix
```

`TAPIRSCAN_NATIVE_DIR` selects another library directory. The installed package
is relocatable; consumers use:

```cmake
find_package(Tapirscan CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE tapirscan::cpp)
```

The package also installs the C header for C consumers. The wrapper targets
64-bit platforms; macOS arm64 is validated locally and Linux in CI. Conan and
vcpkg recipes are not provided yet.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

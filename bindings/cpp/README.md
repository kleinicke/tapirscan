# Tapirscan for C++

`scan(image)` returns `ScanResult`, with decoded text, format and
source-image polygons. Use `result.values()` for strings and `result.barcodes`
for located reads; both are empty when nothing was decoded.

```cpp
auto result = tapirscan::scan(tapirscan::Image::gray(pixels, width, height));
for (const auto& barcode : result.barcodes) std::cout << barcode.text << '\n';
```

Reuse `Scanner::scan` across images. Call `inspect` for an `InspectionResult`
with unread regions, timing and diagnostics. The free functions `scan` and
`inspect` take `(image, ScannerOptions, ScanOptions)`, with the options optional.

Defaults are Medium effort and retail formats (EAN13, UPCA, EAN8 and UPCE). The
header-only C++17 wrapper uses one shared library that contains all four effort
modes.

```cpp
#include <tapirscan.hpp>
#include <iostream>

std::vector<std::uint8_t> pixels(640 * 480, 255);
auto result = tapirscan::inspect(tapirscan::Image::gray(pixels, 640, 480));
for (const auto& barcode : result.barcodes) {
    std::cout << barcode.text << ' ' << tapirscan::to_string(barcode.format) << '\n';
}
std::cout << result.undecoded.size() << " undecoded\n";
```

If nothing is decoded, `barcodes` is empty. Results are owned values and outlive
the scanner. Support, undecoded regions and duplicate payloads are described in
[API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md).

Exceptions:

- `tapirscan::Error` (derived from `std::runtime_error`, with the native status
  in `code`) for invalid images, options and engine failures.
- `std::invalid_argument` from `Formats` for an empty or unsupported selection.
- `std::runtime_error` when the native library's ABI differs from the header.
- `std::logic_error` when a moved-from `Scanner` is used.

## Reuse and configuration

```cpp
tapirscan::ScannerOptions options;
options.mode = tapirscan::Mode::High;
options.formats = tapirscan::Format::Ean13 | tapirscan::Format::QrCode;
tapirscan::Scanner scanner(options);

tapirscan::ScanOptions scan;
scan.formats = tapirscan::Formats(tapirscan::Format::QrCode); // this call only
const auto result = scanner.scan(tapirscan::Image::rgba(pixels, width, height), scan);
if (const auto* best = result.best()) std::cout << best->text << '\n';
```

Reuse a scanner across images; it is move-only and its destructor releases it.
Scans on one scanner serialize; separate scanners run concurrently.
`tapirscan::inspect(image, scanner_options, scan_options)` creates a temporary
scanner for one image.

| Scanner option      | Default                  | Choices                                     |
| ------------------- | ------------------------ | ------------------------------------------- |
| `mode`              | `Mode::Medium`           | `Low`, `Medium`, `High`, `VeryHigh`         |
| `formats`           | `Formats::retail()`      | One format, combinations with `\|`, presets |
| `ean_add_on_policy` | `EanAddOnPolicy::Ignore` | `Ignore`, `Read`, `Require`                 |
| `parallel`          | unset (process default)  | `true`, `false`; `std::optional<bool>`      |

`parallel` runs one scan's formats on separate threads where the host allows it;
results are identical either way. Unset is on unless the process environment sets
`TAPIRSCAN_THREADS` to `0` or `1`.

`Formats::from_bits` rejects empty or unknown bits with `std::invalid_argument`.

Presets: `Formats::retail()`, `common_1d()`, `common()`, `linear()`,
`matrix()` and `all()`. See [format and keyword-group matrix](https://github.com/kleinicke/tapirscan/blob/main/docs/FORMATS.md#format-matrix).

| Per-scan option | Default        | Meaning                                                |
| --------------- | -------------- | ------------------------------------------------------ |
| `formats`       | `std::nullopt` | Readers for this call, replacing the scanner's formats |

Per-call options never change the scanner's configuration.

## Results

`ScanResult` provides `barcodes`, `values()` and `best()`. `best()` returns a
pointer to the barcode with the largest `support` (first read wins ties), or
null when nothing was decoded. It is deleted for temporaries, whose pointer
would dangle; bind the result to a variable first.

`InspectionResult` has the same members plus `undecoded`, `width`, `height`,
`mode`, `elapsed_ms` and `diagnostics` (JSON text with an unstable schema).

`Barcode` contains `text`, `format`, `polygon` (four `Point`s in source-image
pixels, top-left origin), `support`, and optional `payload_bytes`, `ean_add_on`,
`gs1`, `reader_initialization`, `structured_append` and `codabar_start_stop`
(Codabar start and stop characters such as `"AB"`, which `text` omits). An empty
optional means the reader did not report it. `rect()` returns a `tapirscan::Rect` with
`std::int32_t` `left`, `top`, `width` and `height`.

`to_string(format)` gives names such as `"QRCode"`.

## Structured content

`tapirscan::parse_content(barcode)` returns the named fields of GS1, Wi-Fi,
contact, payment and similar payloads.

```cpp
if (auto content = tapirscan::parse_content(barcode); content && content->kind == tapirscan::ContentKind::Wifi)
    connect(*content->get("ssid"), content->get("password").value_or(""));
```

Kinds and fields: [structured content](../../docs/CONTENT.md).

## Images

`Image::gray`, `Image::rgb` and `Image::rgba` take a `std::vector<std::uint8_t>`
or a pointer and length, plus width and height; `.with_stride(bytes_per_row)`
describes padded rows. The vector factories reject temporaries. The pointer
overloads do not copy: keep the buffer alive and unchanged until the scan call
returns. Size limits and conversion rules are in
[API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md#images).

### OpenCV

`cv::Mat` frames are BGR, which has no layout here. Convert to RGBA, the cheapest
input (gray and RGB are copied to RGBA internally for the retail formats), and pass
the row stride:

```cpp
cv::Mat rgba;
cv::cvtColor(frame, rgba, cv::COLOR_BGR2RGBA);
const auto image = tapirscan::Image::rgba(rgba.data, rgba.step * rgba.rows, rgba.cols, rgba.rows)
                       .with_stride(rgba.step);
const auto result = scanner.scan(image);
```

### Fast scanning

Create one `tapirscan::Scanner` and reuse it for every frame; the free functions
create a scanner per call. Select only the formats you need, since each enabled 2D
format searches the whole image. Medium suits camera frames too: `Mode::Low` is
faster per frame but reads fewer blurred and small codes. Use one
scanner per thread; scans with several formats also use extra threads
internally (`options.parallel = false` keeps each scan on one thread). See
[fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md).

## Prebuilt libraries

The `tapirscan-VERSION-PLATFORM.zip` archives of each
[GitHub release](https://github.com/kleinicke/tapirscan/releases) are relocatable install trees with both headers and
the CMake package, so no Rust toolchain is needed. Unpack one and add its
directory to `CMAKE_PREFIX_PATH`:

```sh
cmake -S . -B build -DCMAKE_PREFIX_PATH=/path/to/tapirscan-1.5.0-linux-x64
```

The [C binding](https://github.com/kleinicke/tapirscan/blob/main/bindings/c/README.md#prebuilt-libraries)
lists the platforms.

## Building and installing

```sh
python3 scripts/build_native.py
cmake -S bindings/cpp -B build/cpp
cmake --build build/cpp --config Release
ctest --test-dir build/cpp --output-on-failure -C Release
cmake --install build/cpp --prefix /your/install/prefix --config Release
```

`--config` and `-C` select the configuration for multi-configuration generators
such as Visual Studio and are ignored elsewhere. On Windows the build copies
`tapirscan.dll` next to the test executables; your own programs need it next to
the executable or on `PATH`.

`TAPIRSCAN_NATIVE_DIR` selects another library directory. The installed package
is relocatable; consumers use:

```cmake
find_package(Tapirscan CONFIG REQUIRED)
target_link_libraries(my_app PRIVATE tapirscan::cpp)
```

The package also installs the C header for C consumers. The wrapper targets
64-bit platforms.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

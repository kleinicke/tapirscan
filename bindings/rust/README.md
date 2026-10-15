# Tapirscan for Rust

`scan(image)` returns `Result<ScanResult, Error>`: decoded text, format and
source-image geometry. `result.values()` borrows decoded strings and
`result.barcodes` holds the located reads. Use `inspect(image)` for an
`InspectionResult` that adds unread regions, timing and diagnostics. Both have
`_with_options` variants. The crate requires Rust 1.91 (`rust-version = 1.91`).

```sh
cargo add tapirscan
```

On x86-64, Cargo targets the oldest x86-64 CPUs by default. Building with
`RUSTFLAGS="-C target-cpu=x86-64-v2"` (SSE4.2; every Intel CPU since 2008 and AMD
since 2011) makes scans 20-30% faster with identical results; the prebuilt native
library for C, C++, Java and Python uses it. To keep it for every build, add to
`.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=x86-64-v2"]
```

Use the matching target name on Windows (`x86_64-pc-windows-msvc`) or macOS
(`x86_64-apple-darwin`).

```rust
use tapirscan::Image;

fn main() -> Result<(), tapirscan::Error> {
    let pixels = vec![255; 640 * 480];
    let result = tapirscan::scan(Image::gray(&pixels, 640, 480))?;
    for barcode in &result.barcodes {
        println!("{} {} {:?}", barcode.text, barcode.format, barcode.rect());
    }
    Ok(())
}
```

Defaults are Medium effort and retail formats (EAN13, UPCA, EAN8 and UPCE). If
nothing is decoded, `barcodes` is empty. Invalid input or engine failures are
`Error` values. Results own their data and outlive the input and scanner.
Support, undecoded regions and image limits are described in
[API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md).

## Fast scanning

Reuse one `Scanner` per thread; the free `scan` and `inspect` functions create one
per call. Select only the formats you need, since each enabled 2D format searches
the whole image, and use `Mode::Low` for camera frames. Pass the 8-bit buffer you
already have; packed RGBA is the cheapest input for the retail formats (gray and
RGB are copied to RGBA internally), so do not convert to gray for scanning. Scans with several 2D formats
also use extra threads internally; set `TAPIRSCAN_THREADS=1` when you already run
one scanner per core. See
[fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md).

## Reuse and configuration

```rust
use tapirscan::{Format, Image, ScanOptions, Scanner, ScannerOptions};

fn main() -> Result<(), tapirscan::Error> {
    let mut scanner = Scanner::new(ScannerOptions {
        formats: Format::Ean13 | Format::QrCode,
        ..ScannerOptions::default()
    });
    let pixels = vec![255; 320 * 240];
    let result = scanner.inspect_with_options(
        Image::gray(&pixels, 320, 240),
        ScanOptions { formats: Some(Format::QrCode.into()) },
    )?;
    println!("{} reads", result.barcodes.len());
    Ok(())
}
```

`Scanner::scan` and `inspect` take `&mut self`: a scanner serves one thread at a
time, so create one scanner per thread. The free functions `scan` and `inspect`
create a scanner for each call; reuse a `Scanner` across frames. Scans borrow
pixels synchronously and have no timeout. `scanner.options()` returns the
configuration and `Scanner::default()` uses the defaults.

| Scanner option      | Default                  | Choices                                     |
| ------------------- | ------------------------ | ------------------------------------------- |
| `mode`              | `Mode::Medium`           | `Low`, `Medium`, `High`, `VeryHigh`         |
| `formats`           | `Formats::RETAIL`        | One format, combinations with `\|`, presets |
| `ean_add_on_policy` | `EanAddOnPolicy::Ignore` | `Ignore`, `Read`, `Require`                 |

Presets: `Formats::RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX`, `ALL`
(see [format coverage](https://github.com/kleinicke/tapirscan/blob/main/docs/FORMATS.md)). `Read` accepts a supplement when
present; `Require` drops retail reads without one. Non-retail formats are
unaffected.

| Per-scan option | Default | Meaning                                                |
| --------------- | ------- | ------------------------------------------------------ |
| `formats`       | `None`  | Readers for this call, replacing the scanner's formats |

Per-call options never change the scanner's configuration.

## Results

`ScanResult` provides `barcodes`, `values()` and `best()`, which returns
`Option<&Barcode>`. `InspectionResult` has the same members plus `undecoded`,
`image_size` (`[width, height]`), `mode`, `elapsed` (`Duration`) and
`diagnostics` (`Option<Diagnostics>`, unstable JSON in `raw`).

`Barcode` contains `text`, `format`, `polygon`, `support` and optional metadata
(original payload bytes, supplement text, structured append, GS1 and
reader-initialization flags). `rect()` returns a `Rect { left, top, width,
height }` of `i32` pixel bounds. `None` flags mean unavailable, not false.
Structured append indices are one-based; the caller assembles messages. Main
geometry excludes supplements.

`Format`, `Barcode`, `StructuredAppend`, `Rect` and `ScanResult` implement serde
`Serialize` and `Deserialize`; `UndecodedRegion` implements `Deserialize` only.
`InspectionResult` is not serializable.

## Structured content

`Barcode::content` recognizes GS1 elements, Wi-Fi, contacts, payment slips,
HIBC, VINs and driver's licenses in the decoded text; `parse_content(text, gs1)`
parses any text.

```rust
if let Some(content) = barcode.content() {
    println!("{}: {:?}", content.kind, content.get("01"));
}
```

Kinds and field names are listed in [structured content](../../docs/CONTENT.md).

## Images

The default `image` feature accepts `&image::GrayImage`, `&image::RgbImage` and
`&image::RgbaImage` without copying pixels, and `Image::try_from(&dynamic_image)?`
borrows a `DynamicImage` in one of those formats, such as a decoded JPEG from
`image::open(path)?`. The application decodes image files and converts other
formats, for example with `to_rgb8()`. To omit the image dependency, disable the default
features and list the modes you need; see [build features](#build-features-and-webassembly).

Raw buffers use `Image::gray(data, width, height)`, `Image::rgb(...)` or
`Image::rgba(...)`, optionally `.with_stride(bytes_per_row)`. RGB order is
interleaved; alpha is ignored, including zero alpha. Composite transparency
before scanning if needed. BGR, planar, float and 16-bit pixels need conversion.

Construction borrows without validation; scanning validates before access.
Size limits are in [API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md#images); final-row
padding is optional and extra bytes are ignored. `Error` implements `std::error::Error` with `InvalidImage`,
`InvalidOptions` and `Engine` variants.

## Build features and WebAssembly

The default features include all four effort modes and the `image` integration.
Each mode is a feature (`mode-low`, `mode-medium`, `mode-high`, `mode-very-high`)
that only adds code, so features enabled by other dependencies never remove a
mode. For a smaller binary, disable the defaults and list what you need:

```toml
tapirscan = { version = "1.3", default-features = false, features = ["mode-medium"] }
```

Scanning with a mode that is not compiled in returns `Error::InvalidOptions`.

On `wasm32-unknown-unknown`, `elapsed` is zero because no platform clock is
available.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

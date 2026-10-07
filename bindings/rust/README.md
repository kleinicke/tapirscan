# Tapirscan for Rust

`scan(image)` returns `Result<ScanResult, Error>`: decoded text, format and
source-image geometry. `result.values()` borrows decoded strings and
`result.barcodes` holds the located reads. Use `inspect(image)` for an
`InspectionResult` that adds unread regions, timing and diagnostics. Both have
`_with_options` variants.

```rust
use tapirscan::Image;
let pixels = [255; 64 * 64];
for barcode in tapirscan::scan(Image::gray(&pixels, 64, 64))?.barcodes {
    println!("{} {:?}", barcode.text, barcode.polygon);
}
# Ok::<(), tapirscan::Error>(())
```

Defaults are Medium effort and retail formats (EAN13, UPCA, EAN8 and UPCE).

```rust
use tapirscan::Image;
let pixels = vec![255; 640 * 480];
let result = tapirscan::inspect(Image::gray(&pixels, 640, 480))?;
for barcode in &result.barcodes {
    println!("{} {:?} {:?}", barcode.text, barcode.format, barcode.polygon);
}
println!("{} undecoded", result.undecoded.len());
# Ok::<(), tapirscan::Error>(())
```

If nothing is decoded, `barcodes` is empty. Invalid input or engine failures are
`Error` values. Results own their data and outlive the input and scanner. Equal
payloads at distinct locations are reported as separate barcodes.

## Reuse and configuration

```rust
use tapirscan::{Format, Image, ScanOptions, Scanner, ScannerOptions};
let mut scanner = Scanner::new(ScannerOptions {
    formats: Format::Ean13 | Format::QrCode,
    ..ScannerOptions::default()
});
let pixels = vec![255; 320 * 240];
let result = scanner.inspect_with_options(Image::gray(&pixels, 320, 240), ScanOptions {
    extended_budget: true,
    ..ScanOptions::default()
})?;
# Ok::<(), tapirscan::Error>(())
```

`scan`/`inspect` use default per-call options; the `_with_options` forms take a
`ScanOptions`. Both exist as free functions and as `Scanner` methods.

Reuse a scanner across frames. Scans borrow pixels synchronously and have no
timeout. `scanner.options()` returns the configuration; `Scanner::default()` uses
the defaults.

| Scanner option      | Default                  | Choices                                     |
| ------------------- | ------------------------ | ------------------------------------------- |
| `mode`              | `Mode::Medium`           | `Low`, `Medium`, `High`, `VeryHigh`         |
| `formats`           | `Formats::RETAIL`        | One format, combinations with `\|`, presets |
| `ean_add_on_policy` | `EanAddOnPolicy::Ignore` | `Ignore`, `Read`, `Require`                 |

Presets: `Formats::RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX`, `ALL`.
Retail formats are enabled by default; select additional supported formats as
needed. `Read` accepts a supplement when present; `Require` drops retail reads
without one. Non-retail formats are unaffected.

| Per-scan option   | Default | Meaning                                |
| ----------------- | ------- | -------------------------------------- |
| `formats`         | `None`  | Override readers for this call         |
| `extended_budget` | `false` | Allow extra reader work for any format |

`extended_budget: true` allows additional reader work for any selected format.
It can cost more time and does not promise exhaustive decoding. Per-call
options never change the scanner's configuration.

## Results

`ScanResult` provides `barcodes`, `values()` and `best()`, which returns
`Option<&Barcode>`: the read with the largest `support` (first read wins ties),
`None` when nothing was decoded.

`InspectionResult` has the same members plus `undecoded`, `image_size`, `mode`,
`elapsed` (`Duration`) and `diagnostics` (`Option<Diagnostics>`, unstable JSON in
`raw`).
`Barcode` implements serde `Serialize` and `Deserialize`.

`Barcode` contains `text`, `format`, `polygon`, `support` and optional metadata
(original payload bytes, supplement text, structured append, GS1 and
reader-initialization flags). `support` is reader-specific evidence, a ranking
heuristic and not a confidence. `rect()` returns `[left, top, width, height]`,
the enclosing integer pixel bounds. Coordinates refer to the supplied image with
a top-left origin. `None` flags mean unavailable, not false. Structured append
indices are one-based; the caller assembles messages. Main geometry excludes
supplements.

`undecoded` contains localized regions without an accepted decode. These can be
false candidates or overlap, and an empty list does not guarantee that every
barcode was found.

## Images

The default `image` feature accepts `&image::GrayImage`, `&image::RgbImage` and
`&image::RgbaImage` without copying pixels. The application decodes image files
and converts other formats. Use `default-features = false` to omit the image
dependency.

Raw buffers use `Image::gray(data, width, height)`, `Image::rgb(...)` or
`Image::rgba(...)`, optionally `.with_stride(bytes_per_row)`. RGB order is
interleaved; alpha is ignored, including zero alpha. Composite transparency
before scanning if needed. BGR, planar, float and 16-bit pixels need conversion.

Construction borrows without validation; scanning validates before access.
Dimensions are at least 3×3 and at most 32 megapixels. Stride is at least
`width * channels`. The buffer must address `(height - 1) * stride + width *
channels` bytes, at most 128 MiB; final-row padding is optional. Extra bytes are
ignored. `Error` implements `std::error::Error` with `InvalidImage`,
`InvalidOptions` and `Engine` variants.

## Build features and WebAssembly

All four effort modes are available by default. To build a smaller binary, select
one or more of `mode-low`, `mode-medium`, `mode-high` and `mode-very-high`.
Scanning with an excluded mode returns `Error::InvalidOptions`.
`default-features = false` only disables the `image` integration and still
includes every mode.

On `wasm32-unknown-unknown`, `elapsed` is zero because no platform clock is
available.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

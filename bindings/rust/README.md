# Tapirscan for Rust

Scan decoded pixels and receive every accepted barcode, source-image geometry,
undecoded proposals and reported work limits. Defaults are Medium effort and
retail formats (EAN13, UPCA, EAN8 and UPCE).

```rust
use tapirscan::Image;
let pixels = vec![255; 640 * 480];
let result = tapirscan::scan(Image::gray(&pixels, 640, 480))?;
for barcode in &result {
    println!("{} {:?} {:?}", barcode.text, barcode.format, barcode.polygon);
}
println!("{} undecoded; work limited: {}", result.undecoded.len(), result.unfinished);
# Ok::<(), tapirscan::Error>(())
```

No detection is an empty `barcodes` vector. Invalid input or engine failures are
`Error` values. Results own their data and survive the input and scanner.
Equal payloads at distinct locations remain separate physical instances.

## Reuse and configuration

```rust
use tapirscan::{Format, Image, ScanOptions, Scanner, ScannerOptions};
let mut scanner = Scanner::new(ScannerOptions {
    formats: Format::Ean13 | Format::QrCode,
    ..ScannerOptions::default()
});
let pixels = vec![255; 320 * 240];
let result = scanner.scan_with_options(Image::gray(&pixels, 320, 240), ScanOptions {
    extended_budget: true,
    ..ScanOptions::default()
})?;
# Ok::<(), tapirscan::Error>(())
```

`scan(image)` uses default per-image options; `scan_with_options(image, options)`
sets overrides. Both forms are available as free functions and scanner methods.

Reuse a scanner across frames; ordinary RAII releases resources. Scans borrow
pixels synchronously and have no wall-clock timeout. `scanner.options()` returns
configuration. `Scanner::default()` needs no configuration.

| Scanner option      | Default                  | Choices                                     |
| ------------------- | ------------------------ | ------------------------------------------- |
| `mode`              | `Mode::Medium`           | `Low`, `Medium`, `High`, `VeryHigh`         |
| `formats`           | `Format::Ean13.into()`   | One format, combinations with `\|`, presets |
| `ean_add_on_policy` | `EanAddOnPolicy::Ignore` | `Ignore`, `Read`, `Require`                 |

Presets: `Formats::RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX`, `ALL`.
Retail formats (EAN13, UPCA, EAN8 and UPCE) are supported. Other readers remain experimental. Effort tunes EAN13/UPCA, common linear and
QR readers; other matrix readers retain fixed effort. `Read` preserves the main
barcode without a readable supplement; `Require` filters retail reads without
one. Nonretail formats are unaffected.

| Per-scan option   | Default | Meaning                                |
| ----------------- | ------- | -------------------------------------- |
| `formats`         | `None`  | Override readers for this call         |
| `debug`           | `false` | Retain raw engine evidence             |
| `extended_budget` | `false` | Allow extra reader work for any format |

`extended_budget: true` allows additional reader work for any selected format.
Exact budgets and search stages may evolve without changing this option. Today
it relaxes shared EAN/UPC retry and association caps; other readers retain their
current budgets. Per-candidate effort, weak-candidate deferral and other limits
remain. It can cost more time and does not promise exhaustive decoding. Per-call
options never change scanner configuration.

## Results

`ScanResult` exposes `barcodes`, `undecoded`, `image_size`, `mode`, `elapsed`
(`Duration`), `unfinished` and optional `debug`. Iterate by reference or consume
it to move barcodes. `values()` borrows text. `best()` borrows the
largest-support read, keeping first-read ties. Support is reader-specific and
not comparable confidence across formats or efforts.

`Barcode` contains `text`, `format`, `polygon`, `support` and optional payload metadata. `rect()` returns
`[left, top, width, height]` enclosing integer pixel bounds. Coordinates refer to
the supplied image, top-left origin. Metadata includes support, original optional
payload bytes, supplement text, structured append and optional GS1/initialization
flags. `None` flags mean unavailable, not false. Structured append indices are
one-based; the caller assembles messages. Main geometry excludes supplements.

`undecoded` contains localized proposals without accepted decodes, independently
of debug. These can be false candidates, overlapping regions or deferred work.
They do not prove a real barcode is unreadable. An empty list and `unfinished:
false` do not guarantee exhaustive coverage. Debug raw schemas are unstable.

## Images

The default `image` feature accepts borrowed `GrayImage`, `RgbImage`, `RgbaImage`
without copying pixels. The application decodes image files. Convert other
formats explicitly. Use `default-features = false` to omit the image dependency.

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

This API also powers the C and JavaScript bindings. The WASM adapter performs
memory transfer and serialization; scanning and reconciliation stay in Rust.

Without an explicit mode feature, all four efforts are available. To build a
smaller adapter, select `mode-low`, `mode-medium`, `mode-high` or `mode-very-high`;
multiple selections are supported. Scanning with an excluded mode returns
`Error::InvalidOptions`. `default-features = false` alone still includes every
mode and only disables the optional image integration.

On `wasm32-unknown-unknown`, Rust `elapsed` is zero because no platform clock is
imported. The JavaScript adapter measures the complete synchronous scan call.

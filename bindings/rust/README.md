# Rust

`python3 scripts/build_native.py MODE` creates a local Cargo package at `build/MODE/rust` using
this facade and the exact mode-specific patched core. For example, run `python3 scripts/build_native.py medium` from the repository
root, then add this dependency to another local project:

```toml
[dependencies]
tapirscan = { path = "/absolute/path/to/tapirscan/build/medium/rust" }
```

```rust
use tapirscan::{Format, Image, ScanOptions, Scanner};
let mut scanner = Scanner::default();
let pixels = vec![255; 64 * 64];
let result = scanner.scan_formats(Image {
    data: &pixels, width: 64, height: 64, channels: 1, stride: 64,
}, ScanOptions::default(), Format::Ean13).unwrap();
assert!(result.barcodes().is_empty());
for barcode in result.barcodes() {
    println!("{} {:?} {:?}", barcode.text, barcode.format, barcode.polygon);
}
```

Mode selection is currently at build time. The facade connects the existing safe
localizer, shear refinement and region scanner, mirroring the selected browser mode's refinement budget, full-frame search,
retry mask and bounded source-detail recovery. Supplied-region callers can
use the re-exported `RegionScanner`. Inputs are borrowed; results own their data.
Before crates.io publication, replace generated path-based packages with a
maintainable versioned crate layout while preserving measured mode behavior.

`Result::to_json(mode, elapsed_ms)` serializes full native evidence for the C ABI.
The native build helper refreshes this facade without changing the core snapshot.

`scan(image)` defaults to multiple results and no exposed region evidence. Use
`scan_with_options(image, ScanOptions { multiple: false, include_regions: true })`
to choose before scanning. `result.barcodes()` always returns a slice; single mode
selects the highest-support read after full scanning, with stable tie ordering.
`result.regions()` is `Some` only when requested; it exposes localized proposals,
the full-frame search window and all per-candidate evidence. The underlying
undecoded evidence is retained internally. `unfinished()` and
`localization_limited()` are always available. Serialized results use schema 2.

## Functions and options

| Call                                                   | Result                                                                   |
| ------------------------------------------------------ | ------------------------------------------------------------------------ |
| `Scanner::default()`                                   | Scanner for this compiled mode.                                          |
| `scan(image)`                                          | `Result<tapirscan::Result, Error>` for EAN13 with defaults.              |
| `scan_with_options(image, options)`                    | Typed EAN13 result with explicit output options.                         |
| `scan_formats(image, options, mask)`                   | `Result<DecodedResult, Error>` with typed formats, strings and polygons. |
| `result.barcodes()`, `result.best()`                   | Barcode slice and optional highest-support read.                         |
| `result.unfinished()`, `result.localization_limited()` | Separate completion flags.                                               |
| `result.regions()`                                     | Optional borrowed diagnostic evidence.                                   |
| `result.to_json(mode, elapsed_ms)`                     | Native JSON serialization; caller supplies mode label and timing.        |

`Image` requires `data`, `width`, `height`, `channels` (1/3/4), and `stride` in
bytes. Use decoded gray/RGB/RGBA pixels; alpha is ignored. No image codec is
bundled. `ScanOptions` contains `multiple` (default true) and `include_regions`
(default false). Polygons are in source-image coordinates; support is a ranking
heuristic, not a probability.

For additional formats:

```rust
use tapirscan::Formats;
let result = scanner.scan_formats(image, ScanOptions::default(), Formats::ALL)?;
let values: Vec<&str> = result.values().collect();
let best = result.best();
println!("{values:?} {best:?}");
```

`Formats::LINEAR`, `MATRIX`, `RETAIL` and `ALL` are supported presets. Compose
individual formats with `Format::Ean13 | Format::Code128`, or use
`Formats::try_from(mask)` to validate native format bits. `Format` is a typed enum;
see [coverage](../../docs/FORMATS.md). `DecodedResult` exposes `barcodes()`,
`values()` (iterator), `best()`, `image_size()` ([width, height]), `unfinished()`
and `localization_limited()`. Each `DecodedBarcode` has `text`, `format` and
`polygon`. Results own their data and survive scanner destruction.

`debug()` returns optional raw evidence when `include_regions` was requested;
`json()` exposes the underlying schema-2 payload and reader metadata explicitly.
Typed result construction currently converts the shared internal JSON payload;
removing that internal conversion is a future optimization. Ordinary Rust callers
do not need to parse JSON. `scan_formats_json(image, options, mask)` retains the
raw path used by the C ABI.

The older EAN13-only `scan`/`scan_with_options` methods retain the lower-level
research result type, where text is accessed through `barcode.detection.text`.
Use `scan_formats` for the consistent typed multi-format interface, even for EAN13.
`MODE` and `MODE_ID` identify the compiled mode. Rust's native API is separate from
the stable C ABI: there is no promise of a stable Rust binary ABI across compilers.

This is a generated local facade, not a published crates.io package. Keep its
referenced core directories alongside it; copying just the generated `rust/`
directory is insufficient. See [build and release status](../../docs/RELEASING.md).

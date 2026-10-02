# Migration to the uniform API

## Barcode lists and explicit inspection

`scan` now returns `Vec<Barcode>` (Rust), `std::vector<Barcode>` (C++),
`List<Barcode>` (Java), `list[Barcode]` (Python), or a frozen `Barcode[]` (JS).
Replace `scan(image).barcodes` with `scan(image)`. Map the list to obtain text.
If a caller uses timing, work status, unread regions or diagnostic evidence,
replace `scan` with `inspect`, remove the debug option, and rename the report's
`debug` field to `diagnostics`. Rust also exposes `inspect_with_options`.

The unreleased 1.3.0 API retains native ABI 6. It removes the debug field from `tapirscan_scan_options`, adds
`tapirscan_inspect` and `tapirscan_result_count`, and makes summary/unread-region
accessors inspection-only. Rebuild native consumers together. WASM ABI 2 makes ordinary wire results barcode-only. The host also accepts
historical ABI 1 assets for frozen demo comparisons.

## 1.3.0: C, C++ and Java

C, C++ and Java now use the same scanner, options and result model as Rust,
Python and JavaScript. Native ABI 6 replaces ABI 4, and one library
(`libtapirscan.so`, `.dylib` or `tapirscan.dll`) contains all four effort modes.
Rebuild applications against the new header; there are no deprecated aliases.
Python, JavaScript and Rust applications are unaffected, and scan results of
the stable modes are unchanged.

| Previous (ABI 4)                                                             | Replacement (ABI 5)                                                    |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| One library per mode, `libtapirscan_<mode>`                                  | One `libtapirscan` library; choose `mode` in the scanner options       |
| `barcode_abi_version`, `barcode_mode`                                        | `tapirscan_abi_version`; read `mode` from the result summary           |
| `barcode_capabilities`                                                       | Removed; `extended_budget` is always available                         |
| `tapirscan_create`, `tapirscan_destroy`                                      | `tapirscan_scanner_create(options, &out)`, `tapirscan_scanner_destroy` |
| `barcode_scan`, `barcode_scan_with_options`, `barcode_scan_formats`          | `tapirscan_scan(scanner, &image, options, &result)`                    |
| `BARCODE_SINGLE`                                                             | `best_index` in the result summary                                     |
| `BARCODE_INCLUDE_REGIONS`                                                    | Typed `tapirscan_result_undecoded`; evidence via `inspect`             |
| `BARCODE_READ_EAN_ADDON`, `BARCODE_REQUIRE_EAN_ADDON`                        | Scanner option `ean_add_on_policy`                                     |
| `BARCODE_FINISH_CANDIDATES`                                                  | Scan option `extended_budget`                                          |
| `barcode_result_info`, `barcode_result_metadata`                             | `tapirscan_result_info`, `tapirscan_summary`                           |
| `barcode_result_read`, `barcode_read`                                        | `tapirscan_result_barcode`, `tapirscan_barcode` (all metadata)         |
| `barcode_result_copy_text`                                                   | `tapirscan_result_copy(result, i, TAPIRSCAN_FIELD_TEXT, ...)`          |
| `barcode_result_copy_json`, `barcode_result_destroy`                         | `tapirscan_result_copy_json`, `tapirscan_result_destroy`               |
| `BARCODE_OK`, `BARCODE_*` statuses, format mask numbers                      | `TAPIRSCAN_OK`, `TAPIRSCAN_*`, `TAPIRSCAN_FORMAT_*` constants          |
| C++ `#include <tapirscan/scanner.hpp>`                                       | `#include <tapirscan.hpp>`                                             |
| C++ `scanner.scan(pixels, length, w, h, channels, stride, ScanOptions{...})` | `scanner.scan(Image::rgba(pixels, w, h), ScanOptions)`                 |
| C++ `result.barcodes()`, `result.json()`                                     | `result.barcodes`, `result.diagnostics` (when requested)               |
| CMake `-DBARCODE_MODE=<mode>`, `BARCODE_NATIVE_DIR`                          | Mode is a scanner option; `TAPIRSCAN_NATIVE_DIR`                       |
| Java `new Tapirscan(libraryDir, Mode)`                                       | `new Scanner(ScannerOptions)` or `Tapirscan.scan(image)`               |
| Java `scan(pixels, w, h, channels, stride, options)`                         | `scanner.scan(Image.rgba(pixels, w, h).withStride(stride))`            |
| Java `ScanOptions(multiple, includeRegions, formats)`                        | `ScanOptions` with `formats`, `extendedBudget`                         |
| Java `Barcode.format()` as a string                                          | `Format` enum; `toString()` gives the shared name                      |

Java locates the library through the `tapirscan.library` system property, the
`TAPIRSCAN_LIBRARY_DIR` environment variable or the system library path.

## 1.2.0: Rust, Python and JavaScript

These are intentional breaking changes in version 1.2.0, an explicit early-library
exception to the compatibility policy. Do not
publish them as a compatible 1.1.x update. Engine recipes and native ABI 4 are
unchanged; this migration changes the application interfaces.

| Previous                                               | Replacement                                        |
| ------------------------------------------------------ | -------------------------------------------------- |
| Python `finish_candidates=False` / `True`              | `extended_budget=False` / `True`                   |
| JavaScript `finishCandidates: false` / `true`          | `extendedBudget: false` / `true`                   |
| Rust `finish_candidates: false` / `true`               | `extended_budget: false` / `true`                  |
| Rust `scan_all(image)`                                 | `scan(image)?`                                     |
| Rust `scan_one(image)`                                 | Scan, then select a barcode from the result        |
| Rust `scan_detailed(image, options)`                   | `scan_with_options(image, options)`                |
| Debug-only undecoded geometry                          | `result.undecoded`, always available               |
| Rust `barcode.metadata.field`                          | `barcode.field`, matching the other bindings       |
| Rust false for unavailable GS1/initialization metadata | `None`; known flags use `Some(bool)`               |
| Rust tight floating-point `rect()`                     | Enclosing integer pixel bounds, matching Python/JS |

The Rust changes apply to free functions and reusable scanner methods. Selection
by highest support still scans all selected candidates; it is not a cross-format
confidence estimate. There are no deprecated aliases for the replaced APIs.

A proposal without an accepted decode is called **undecoded**, not unreadable.
It could be a false candidate or need work the selected policy did not perform.
`unfinished` reports limits and deferrals, not a count of missed barcodes.

The extended-budget flag is valid for all formats and requests additional reader
work. Exact budgets and stages are implementation details that may evolve. Today
it relaxes shared EAN/UPC retries; other readers keep their current budgets. False
preserves the default decoding policy. Undecoded geometry is retained on normal
scans, which may add result-conversion cost; no speed improvement is claimed.
Raw traces require `inspect`; they are exposed as `diagnostics`.

The intermediate `candidate_budget` / `candidateBudget` API and `CandidateBudget`
enum are removed. Replace `"shared"` with false and `"per_candidate"` with true
using `extended_budget` / `extendedBudget`. There is no EAN/UPC format restriction.

Rust `scan(image)` now supplies default scan options automatically. Use
`scan_with_options(image, options)` for overrides on free functions or scanners.
The support-based convenience selection is named `best` in all three APIs.

## Native ABI 6 refinements

Rebuild the shared library and every native consumer together. Scanner creation
and scanning now accept a final optional `tapirscan_error *` for caller-owned
UTF-8 error details (pass `NULL` to discard). Initialize C scanner options with
`TAPIRSCAN_SCANNER_OPTIONS_INIT`; zero initialization is only the per-scan default.

`tapirscan_summary` no longer contains `json_length`. Call
`tapirscan_result_json_length(result, &length)` before copying JSON. Typed result
access does not serialize JSON. Ordinary scan JSON is a barcode array. Inspection returns schema-2 JSON with
unread geometry and engine evidence. Python also avoids engine diagnostics
during ordinary scans. Rust exposes `localization_limited` to
preserve the separate localization flag across native serialization.

C++ rejects temporary vectors in image factories and rejects empty/unknown format
masks at construction. Keep the backing pixel buffer alive until scanning returns.
C++ `best_index` and Java's `bestIndex` record component were removed; use `best()`.
Java's `ScanResult` constructor no longer takes an index. Java barcode equality
and hashing compare payload bytes by content.

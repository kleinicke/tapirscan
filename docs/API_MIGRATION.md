# API migration

## Upgrading from 1.2.2

Version 1.3.0 changes the application API in every binding, as an explicit
early-library exception to the [compatibility policy](../CONTRIBUTING.md#api-stability).
Migrate callers before upgrading; pin 1.2.2 until they are ready.
Medium effort, Retail formats and ignored supplements remain the defaults.
Scanner improvements may change reads, geometry, ordering and runtime.

### Values, locations and inspection

`scan` returns a lightweight `ScanResult` with `values`, `barcodes` and `best`
(properties in JavaScript/Python; `values()` and `best()` in Rust/C++/Java).
Each barcode includes its text, format, polygon and enclosing rectangle.
C retains its owned result handle and typed accessors.

| In 1.2.2                                             | In 1.3.0                                                  |
| ---------------------------------------------------- | --------------------------------------------------------- |
| `result.values` after scanning                       | Unchanged                                                 |
| `result.barcodes` for text and locations             | Unchanged                                                 |
| Timing, `unfinished` or `undecoded` on a scan result | Call `inspect` instead of `scan`                          |
| `debug` scan option and result field                 | Remove the option; use `inspect` and `result.diagnostics` |
| Explicit rich `ScanResult` type annotation           | Use `InspectionResult` when calling `inspect`             |

The usual values-and-locations case stays short:

```js
const result = await scan(image);
console.log(result.values);
for (const barcode of result.barcodes) {
  console.log(barcode.text, barcode.polygon, barcode.rect);
}
```

```python
result = tapirscan.scan(image)
print(result.values)
for barcode in result.barcodes:
    print(barcode.text, barcode.polygon, barcode.rect)
```

Python and Rust results remain iterable. Reusable scanners return the same
result types as their one-shot helpers. Rust also exposes `scan_with_options`
and `inspect_with_options` for per-call settings.

Use `inspect` only when you need timing, undecoded proposals, work status or
engine diagnostics. Inspection collects additional evidence; ordinary scans
avoid that collection while returning the same decoded values and locations
for the same options. There is no `debug` argument or deprecated alias.

`best` keeps the first read on equal support and returns the binding's empty
value when nothing was decoded. Support is not cross-format confidence.
Separate barcodes with the same text remain separate entries, including in
`values`; use `barcodes` to keep each value paired with its location.

JavaScript `JSON.stringify(result)` and Python `result.as_dict()` still produce
objects, but ordinary results contain only barcodes, values and best. Consumers
of stored timing or diagnostic fields must switch to inspection and update
schemas as needed. Python `as_dict()` excludes diagnostics; use
`report.diagnostics.to_raw_dict()` for engine evidence.

### Browser applications

The new `tapirscan/browser` entry accepts files and browser image sources and
runs the core in a bundled worker. Use `new Scanner(options)` and await `scan`
or `inspect`. The core `tapirscan` entry still uses `await Scanner.create(options)`
and synchronous reusable scans over decoded pixels. Both expose one-shot helpers
and `dispose()`. See [browser setup](../bindings/javascript/README.md#browser-apps-react-and-svelte).

### Native ABI 4 to ABI 6

Rebuild the native library and C, C++, Python and Java consumers together. One
`libtapirscan.so`, `libtapirscan.dylib` or `tapirscan.dll` contains all four modes.
ABI 5 and earlier drafts of ABI 6 were development revisions, not published
migration targets.

| Previous interface                                                  | Replacement                                                                    |
| ------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Per-mode `libtapirscan_<mode>` libraries                            | One library; select mode in scanner options                                    |
| `barcode_abi_version`                                               | `tapirscan_abi_version`                                                        |
| `tapirscan_create`, `tapirscan_destroy`                             | `tapirscan_scanner_create(options, &out, &error)`, `tapirscan_scanner_destroy` |
| `barcode_scan`, `barcode_scan_with_options`, `barcode_scan_formats` | `tapirscan_scan(scanner, &image, options, &result, &error)`                    |
| Include-regions/debug flags                                         | `tapirscan_inspect`; summary and undecoded accessors require inspection        |
| Single-result flag or best index                                    | Select from the barcode list; C++ and Java provide `best`                      |
| Supplement flags                                                    | Scanner option `ean_add_on_policy`                                             |
| Finish-candidates flag                                              | Scan option `extended_budget`                                                  |
| `barcode_result_read`, `barcode_result_copy_text`                   | `tapirscan_result_barcode`, `tapirscan_result_copy`                            |
| `barcode_result_copy_json`, `barcode_result_destroy`                | `tapirscan_result_copy_json`, `tapirscan_result_destroy`                       |
| C++ `<tapirscan/scanner.hpp>`                                       | `<tapirscan.hpp>`                                                              |
| C++ raw-pixel scan arguments                                        | `scanner.scan(Image::rgba(pixels, w, h), options)`                             |
| CMake `BARCODE_MODE`, `BARCODE_NATIVE_DIR`                          | Runtime mode option, `TAPIRSCAN_NATIVE_DIR`                                    |
| Java `new Tapirscan(libraryDir, Mode)`                              | `new Scanner(ScannerOptions)`                                                  |
| Java string format identifiers                                      | `Format` enum; `toString()` gives the shared name                              |

Initialize C scanner options with `TAPIRSCAN_SCANNER_OPTIONS_INIT`; zero
initialization is only the per-scan default. The final `tapirscan_error *`
argument is optional: pass `NULL` to discard details. Always destroy result and
scanner handles. `tapirscan_result_count` and barcode/copy accessors work on both
ordinary and inspection results. Query `tapirscan_result_json_length` before
copying JSON; ordinary JSON is a barcode array, inspection JSON is a schema-2
report. Typed access does not serialize JSON.

C++ image factories reject temporary vectors; keep the pixel buffer alive until
scanning returns. C++ `best` rejects temporary barcode vectors because its
pointer would dangle. Java `Image` accepts byte arrays or `MemorySegment` pixels;
keep native segments alive and unchanged while scanning. Java locates the library
through `tapirscan.library`, `TAPIRSCAN_LIBRARY_DIR` or the system library path.
See the [C](../bindings/c/README.md), [C++](../bindings/cpp/README.md) and
[Java](../bindings/java/README.md) guides for complete signatures and ownership.

WASM ABI 2 makes ordinary wire results barcode-only. The JavaScript host also
accepts historical ABI 1 assets for frozen demo comparisons.

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
preserves the default decoding policy. In 1.2.x, normal scans retained undecoded geometry in their result report.
In the new API, use `inspect` for this report and its `diagnostics`.

The intermediate `candidate_budget` / `candidateBudget` API and `CandidateBudget`
enum are removed. Replace `"shared"` with false and `"per_candidate"` with true
using `extended_budget` / `extendedBudget`. There is no EAN/UPC format restriction.

Rust `scan(image)` now supplies default scan options automatically. Use
`scan_with_options(image, options)` for overrides on free functions or scanners.
The support-based convenience selection is named `best` in all three APIs.

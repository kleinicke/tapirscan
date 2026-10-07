# Native bindings

C, C++, Python and Java share the public Rust `Scanner` through native ABI 6.
One shared library, built with `python3 scripts/build_native.py`, contains all
four effort modes; every binding selects the mode when it creates a scanner.
The header is [`tapirscan.h`](../bindings/c/include/tapirscan.h). Format bits and
presets are generated from `config/formats.json` into `tapirscan_formats.h`,
the C++ `Format` enum and the Java `Format` enum.

Per-binding guides: [C](../bindings/c/README.md), [C++](../bindings/cpp/README.md),
[Java](../bindings/java/README.md) and [Python](../bindings/python/README.md).
Shared semantics (support, undecoded regions, image limits) are in
[API design](API_DESIGN.md); the raw WebAssembly adapter has its own ABI in
[bindings/wasm](../bindings/wasm/README.md).

## Result fields

Scanner options are mode, formats and supplement policy; per-call options are
formats only. Variable-length C fields are copied into caller buffers with
explicit lengths and preserve embedded NUL bytes. This table is the single
reference for naming differences.

| Language   | Values                              | Best                  | Rect                 | Elapsed                        | Image size                           | Errors                                                |
| ---------- | ----------------------------------- | --------------------- | -------------------- | ------------------------------ | ------------------------------------ | ----------------------------------------------------- |
| C          | `tapirscan_result_copy` per barcode | none                  | none (use `polygon`) | `tapirscan_summary.elapsed_ms` | `tapirscan_summary.width`, `.height` | `tapirscan_status` return, optional `tapirscan_error` |
| C++        | `values()`                          | `best()` (pointer)    | `rect()` → `Rect`    | `elapsed_ms`                   | `width`, `height`                    | `tapirscan::Error` (`code`)                           |
| Java       | `values()`                          | `best()` (`Optional`) | `rect()` → `Rect`    | `elapsedMs()`                  | `width()`, `height()`                | `ScannerException` (`code`)                           |
| Rust       | `values()`                          | `best()` (`Option`)   | `rect()` → `Rect`    | `elapsed` (`Duration`)         | `image_size` (`[width, height]`)     | `tapirscan::Error`                                    |
| Python     | `values`                            | `best`                | `rect`               | `elapsed_ms`                   | `image`                              | `ScannerError`                                        |
| JavaScript | `values`                            | `best`                | `rect`               | `elapsedMs`                    | `image`                              | `ScannerError`                                        |

`Rect` has integer `left`, `top`, `width` and `height` in every language that
provides it.

## Threading and ownership

Input is borrowed for the duration of a call. Calls on one scanner serialize;
separate scanners run concurrently. Results are independently owned. In C each
result and scanner must be destroyed; the other bindings release them through
destructors, `close` or `dispose`.

Status failures never unwind through the C ABI. Out-of-memory and invalid raw
memory are outside that guarantee.

## Tests

Build and test steps are in [validation](VALIDATION.md). `scripts/test_bindings.py`
compares results from Python, Rust, C++, Java and JavaScript in every mode and
requires JDK 22+.

Python wheels bundle the native library; the Java JAR and the C/C++ package ship
it separately.

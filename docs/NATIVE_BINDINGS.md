# Native bindings and result contract

C, C++, Python and Java share the public Rust `Scanner` through native ABI 6.
One shared library, built with `python3 scripts/build_native.py`, contains all
four effort modes; every binding selects the mode when it creates a scanner.
The header is [`tapirscan.h`](../bindings/c/include/tapirscan.h). Format bits and
presets are generated from `config/formats.json` into `tapirscan_formats.h`,
the C++ `Format` enum and the Java `Format` enum.

See the per-binding guides for build steps, usage and limits:
[C](../bindings/c/README.md), [C++](../bindings/cpp/README.md),
[Java](../bindings/java/README.md) and [Python](../bindings/python/README.md).

## Result contract

Scanner options are mode, formats and supplement policy; per-call options are
formats and extended budget. Ordinary scans return a list of typed barcodes with
payload metadata and positions. Inspection adds undecoded regions, image size,
mode, timing and diagnostics. Retail formats are the default selection; see
[format coverage](FORMATS.md).

Variable-length fields are copied into caller buffers with explicit lengths and
preserve embedded NUL bytes. JSON is serialized only when requested: scans give
a barcode array, inspections give a diagnostics report whose schema is unstable.
`support` is reader-specific evidence, not a confidence comparable across formats.

## Images

Inputs are gray8, RGB8 or RGBA8 pixels, at least 3×3 and at most 32 megapixels,
with an addressed layout of at most 128 MiB; the backing buffer may be larger.
Stride and byte length are explicit and checked; a zero stride means packed
rows. Alpha is ignored. Python also accepts common image-library objects with
the conversion rules in its guide.

## Threading and ownership

Input is borrowed for the duration of a call. Calls on one scanner serialize;
separate scanners run concurrently. Results are independently owned and must
each be destroyed. Native panics are caught at the ABI boundary.

## Tests

```sh
python3 scripts/build_native.py
cmake -S bindings/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
python3 scripts/build_java.py
python3 scripts/test_bindings.py
python3 scripts/test_cmake_install.py
```

`test_bindings.py` compares results from Python, Rust, C++, Java and JavaScript
in every mode and requires JDK 22+. The tests check binding parity on synthetic
inputs; they make no accuracy or performance claim. Python wheels bundle the
native library; the Java JAR and the C/C++ package ship it separately.

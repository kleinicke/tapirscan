# Native bindings and result contract

C, C++, Python and Java share the public Rust `Scanner` through native ABI 6.
One shared library, built with `python3 scripts/build_native.py`, contains all
four effort modes; every binding selects the mode when it creates a scanner.
The header is [`tapirscan.h`](../bindings/c/include/tapirscan.h). Format bits and
presets are generated from `config/formats.json` into `tapirscan_formats.h`,
the C++ `Format` enum and the Java `Format` enum.

The ABI mirrors the Rust API: scanner options (mode, formats, supplement policy),
a borrowed image, and per-call options (formats and extended budget).
Ordinary scans expose an owned list of typed barcodes with payload metadata and
positions. Inspection additionally exposes undecoded regions, image size,
mode, timing and `unfinished`. Variable fields are copied into caller
buffers with explicit lengths, preserving embedded NUL bytes. Retail (mask 15)
is the default selection; see [format coverage](FORMATS.md).

JSON is serialized only when its length or bytes are requested. Scanning returns
a barcode array and retains no engine diagnostics. Inspection returns schema-2
JSON, including EAN localization proposals, work limits, search windows,
candidates and additional readers' `scan.regions`. Search windows
represent attempted coverage, not an exhaustive-search guarantee. Support is
uncalibrated and is not comparable between engines as confidence.

Inputs are gray8, RGB8 or RGBA8 pixels, at least 3×3 and at most 32 megapixels,
with an addressed layout of at most 128 MiB; the backing buffer may be larger.
The Rust `Image` validation owns these rules for every native binding. Stride and byte length are explicit and
checked; a zero stride means packed rows. RGB conversion ignores alpha. Python
also supplies optional image adapters with their own documented conversion rules.

Handles are checked registry IDs. Input is borrowed synchronously. Calls on one
scanner serialize; separate scanners can run concurrently. Destruction prevents
new calls while an already-started scan may finish. Results have independent
ownership and must each be destroyed. Copy accessors check capacities and return
explicit errors; no raw result-memory pointer escapes. Native panics are caught
at the ABI boundary. Valid pointer ranges remain the C caller's responsibility.

Native compilation uses a separate prepared source tree. Algorithms and pinned
WASM inputs remain unchanged. Native builds enable unwind for panic containment.
See [promotion provenance](../core/README.md).

After the root build commands:

```sh
python3 scripts/build_native.py
cmake -S bindings/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
python3 scripts/build_java.py
python3 scripts/test_bindings.py
python3 scripts/test_cmake_install.py
# Optional validation dependencies: numpy and zxing-cpp (encoder only).
python3 scripts/test_multiformat.py
```

`test_bindings.py` compares typed results and engine evidence from Python, Rust,
C++, Java and JavaScript in every mode. Java requires JDK 22+. macOS arm64 is
locally validated and Linux CI is configured; Windows/MSVC validation remains
pending. Python wheels bundle the native library; the JAR and the C/C++ package
ship it separately. These tests establish binding parity on synthetic inputs,
not a dataset accuracy or performance claim.

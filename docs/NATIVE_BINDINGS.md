# Native bindings and result contract

C, C++, Python and Java share the safe Rust facade and native ABI 4. Build the
four independent libraries with `python3 scripts/build_native.py low medium high very-high`.
Python and Java select a mode at runtime; C/C++ link one selected library.
Mode IDs are low=0, medium=1, high=2 and very-high=3. Medium is the default.

Retail (EAN13, UPCA, EAN8 and UPCE; mask 15) is the default selection. `barcode_scan_formats` accepts an explicit nonempty
format mask; see [format coverage](FORMATS.md). The typed C read contains a
source-coordinate polygon, support, UTF-8 byte length and format name. Copy the
complete payload using `barcode_result_copy_text`, allocating `text_length + 1`
bytes for the trailing NUL. Use the explicit length to preserve embedded NULs.
C++/Python/Java do this automatically. ABI 1/2 consumers must be rebuilt.

JSON schema remains 2. It preserves mode, multiple, elapsedMs,
localizationLimited, scan.barcodes and scan.unfinished. Each decoded barcode
includes text, format, polygon and support. Optional evidence includes EAN
localization proposals, omitted/work-limited metadata, search windows, candidates,
and the additional readers' decoded and undecoded `scan.regions`. Search windows
represent attempted coverage, not an exhaustive-search guarantee. Some internal
limits in the additional readers are not yet reflected in unfinished.

`multiple=false` selects the highest-support read after full scanning, preserving
the first on a tie. Region evidence is independently opt-in. It may describe
other symbols even when the selected decoded list has at most one result.
Support is uncalibrated and is not comparable between engines as confidence.

Inputs are gray8, RGB8 or RGBA8 pixels, at least 3×3, at most 32 megapixels,
and at most 128 MiB at the C boundary. Stride and byte length are explicit and
checked. RGB conversion ignores alpha. Python also supplies optional image
adapters with their own documented conversion rules.

Handles are checked registry IDs. Input is borrowed synchronously. Calls on one
scanner serialize; separate scanners can run concurrently. Destruction prevents
new calls while an already-started scan may finish. Results have independent
ownership and must each be destroyed. Copy accessors check capacities and return
explicit errors; no raw result-memory pointer escapes. Native panics are caught
at the ABI boundary. Valid pointer ranges remain the C caller's responsibility.

Native compilation uses a separate prepared source tree. One source adapter
makes the existing secondary localization helper public within the dependency;
the recovery dependency is also renamed to prevent Cargo feature unification.
Algorithms and pinned WASM inputs remain unchanged. Native builds enable unwind
for panic containment. See [promotion provenance](PROMOTION_DETAIL_20260914.md).

After the root build commands:

```sh
for mode in low medium high very-high; do
  cmake -S bindings/cpp -B build/cpp-$mode -DBARCODE_MODE=$mode
  cmake --build build/cpp-$mode
  ctest --test-dir build/cpp-$mode --output-on-failure
done
python3 scripts/build_java.py
python3 scripts/test_bindings.py
python3 scripts/test_cmake_install.py
# Optional validation dependencies: numpy and zxing-cpp (encoder only).
python3 scripts/test_multiformat.py
```

Java requires JDK 22+. macOS arm64 is locally validated. Linux CI is configured;
remote CI and Windows/MSVC validation remain pending. Python wheels bundle all four native modes. JAR native assets are still supplied
separately. These tests establish extraction and binding
parity on synthetic inputs, not a dataset accuracy or performance claim.

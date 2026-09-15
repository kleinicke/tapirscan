# Validation and supported environments

Tapirscan separates algorithm reproduction, API correctness, package installation,
and real-device behavior. Passing one category does not establish the others.

## Current evidence

The algorithm promotion during 0.1.2 preparation, retained for 1.0.0, rebuilt all
selected WASM hashes and passed native/WASM and
cross-language checks on macOS arm64. The new detail pipeline matched the research
implementation on 36 generated browser scans; seven results came from recovery.
The native port matched Node/WASM on the same inputs. See
[the promotion record](PROMOTION_DETAIL_20260914.md).

Release preparation also verified the actual npm tarball and macOS arm64 Python
wheel in isolated installations: all four modes decoded the generated EAN-13
fixture, without loading libraries from the checkout. Wheel metadata passed
`twine check`. The JavaScript suite, cross-language binding suite, Python lint/type
checks, and image-adapter checks passed; the CUDA-only adapter test was skipped
because this machine has no CUDA device. All 130 imported source hashes matched.

Chrome is used for automated browser integration. Simulated camera checks do not
establish physical iPhone/Android autofocus or camera resolution. Linux and other
native targets must pass CI and package-installation checks before publication.

## Native binding documentation audit

The native-language guides are checked against the current ABI-3 implementation.
On macOS arm64, C/C++ consumers build in all four modes, Java compiles with
`--release 22`, and cross-language tests compare decoded results with Python and
WASM. C++ installation tests use a separate consumer after relocating each mode's
install prefix. C, C++, Java and Rust README fragments are also compiled and run
with temporary blank pixel buffers; decoding fixtures provide separate positive
coverage. Rust multi-format parity exercises its typed result accessors.

These checks do not establish Maven Central, crates.io, Conan or vcpkg packaging,
or native compatibility on untested platforms. The Java binding requires JDK 22+
and does not target Android. Build-time Rust/C++ mode selection remains a current
API limitation.

## Installed Python wheel audit

A fresh 1.0.0 macOS arm64 wheel was installed with uv into a separate temporary
Python 3.12 environment and exercised from outside the checkout, with no
`TAPIRSCAN_LIBRARY_DIR` override. The import resolved to that environment's
site-packages and used the bundled native libraries. Wheel metadata passed
`twine check`; the inspected native deployment baseline was macOS 11.0.

The installed package passed JPEG/Pillow, TIFF/NumPy, four effort modes, blank
images, output geometry, debug selection, result lifetime, scanning after close,
one-shot scanning, the pyzbar migration alias, tensor/autograd preservation,
Apple MPS input, and Code128/QRCode samples. CUDA was unavailable. This is
installation and functional evidence, not an accuracy benchmark or verification
of Windows/Linux/Intel wheels.

## Checks to run

| Change                   | Checks                                                            |
| ------------------------ | ----------------------------------------------------------------- |
| Imported algorithm       | `verify_import.py`, selected `build.py` recipes, `test_detail.py` |
| JavaScript binding       | Package build and `npm test`; production demo-worker tests        |
| Native ABI or facade     | `build_native.py`, C/C++ tests, Java build, `test_bindings.py`    |
| Format integration       | `test_multiformat.py` with test-only `zxing-cpp` encoder          |
| Python images or loading | `test_python_images.py`, installed-wheel smoke test               |
| Native installation      | `test_cmake_install.py`; clean installed-wheel test               |
| Maintained source        | `node tools/quality/release.mjs all`                              |

The binding checks need built C++ examples and Java classes:

```sh
for mode in low medium high very-high; do
  cmake -S bindings/cpp -B build/cpp-$mode -DBARCODE_MODE=$mode
  cmake --build build/cpp-$mode
  ctest --test-dir build/cpp-$mode --output-on-failure
done
python3 scripts/build_java.py
python3 scripts/test_bindings.py
python3 scripts/test_cmake_install.py
```

Install Pillow, NumPy and `zxing-cpp` for image/format checks, and PyTorch for tensor
adapters. Reference encoders are test dependencies only. Set `QUALITY_PYTHON` to
that environment and `JAVA_HOME` to your JDK for the maintained binding gate.

```sh
python3 scripts/test_python_images.py
python3 scripts/test_multiformat.py
python3 scripts/test_detail.py
pnpm --dir demo check
pnpm --dir demo build
pnpm --dir demo test
```

The optional research/browser parity command is documented in the promotion
record. All generated test images remain temporary.

## What these checks do not claim

Synthetic regression tests are not an independent accuracy holdout. Timing during
builds or CI is not a performance benchmark. Browser and native interpolation can
differ slightly across platforms. Additional-reader internal work limits are not
fully propagated yet. Consult [format limitations](FORMATS.md) and
[benchmark methodology](BENCHMARKS.md) when deciding whether the library fits your use.

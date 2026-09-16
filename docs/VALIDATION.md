# Validation

The release is validated with reproducible mode builds, native/WASM parity,
public API tests and clean package installations. These checks establish behavior
on the tested inputs; they do not establish exhaustive decoding or general accuracy.

The native bindings share ABI 4. Python tests cover pixel inputs, float ranges,
layouts, optional BGR conversion, tensor ownership, serialization, diagnostics,
error handling and resource lifetime. JavaScript tests cover typed results,
format subsets, optional supplement policies, WASM loading and resource lifetime.
Installed-package checks also exercise browser workers and relocated assets.

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
differ slightly across platforms. Consult [format limitations](FORMATS.md) and
[benchmark methodology](BENCHMARKS.md) when deciding whether the library fits your use.

## Supplement policies

CI builds the test-only Zint 2.16.0 encoder from commit
`55541e139e62b9209b71cd9b0ba9010cec28b1d9` and runs the same checks against
the installed Python wheel and built JavaScript API. Locally, after building
native libraries and JavaScript, run:

```sh
python3 scripts/test_supplements.py --encoder /path/to/zint --library-dir build/native
```

This generates fixtures, asserts independently specified payloads and physical
geometry, and compares Python/JS metadata and coordinates across all modes and
policies with diagnostics on/off. It covers missing, erased, two-/five-digit and
different supplements on identical main payloads. Zint and Pillow are test-only
dependencies.

## Local development records

Put machine-specific audit notes and measurement reports in `docs/internal/`,
which is ignored by Git. Keep public API contracts, reproducible validation
commands, benchmark methodology and algorithm provenance in tracked files.
Generated fixtures, binaries and timing samples belong in `build/`.

To measure optional supplement overhead separately from correctness checks:

```sh
python3 scripts/benchmark_supplements.py build/supplement-fixtures/manifest.json
node bindings/javascript/test/benchmark-supplements.mjs build/supplement-fixtures/manifest.json
```

Run benchmarks sequentially after build activity finishes. They report warm-process
creation and repeated-scan timings; they do not measure browser downloads or
first-time compilation. Interpret results for the tested images and hardware.

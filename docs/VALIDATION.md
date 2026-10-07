# Validation

The release is validated with reproducible mode builds, native/WASM parity,
public API tests and clean package installations. The tests live in
`scripts/test_*.py` and the binding test directories (`bindings/*/test*`). Static
checks and their environment are in [quality checks](QUALITY.md).

`test_bindings.py` compares native bindings exactly. Against WASM, only polygon
coordinates (including those in diagnostics) may differ, by at most `1e-9`
source-image pixels of floating-point roundoff; all other output stays exact.

## Checks to run

| Change                   | Checks                                                           |
| ------------------------ | ---------------------------------------------------------------- |
| Core algorithm           | `verify_sources.py`, selected `build.py` modes, `test_detail.py` |
| JavaScript binding       | Package build, `npm test` and `npm run test:browser` (Chrome)    |
| Native ABI or facade     | `build_native.py`, C/C++ tests, Java build, `test_bindings.py`   |
| Format integration       | `test_multiformat.py` with test-only `zxing-cpp` encoder         |
| Python images or loading | `test_python_images.py`, installed-wheel smoke test              |
| Native installation      | `test_cmake_install.py`; clean installed-wheel test              |
| Browser parity           | `compare_scanners.py --backend browser`                          |
| Maintained source        | `node tools/quality/release.mjs all`                             |

```sh
cmake -S bindings/cpp -B build/cpp
cmake --build build/cpp
ctest --test-dir build/cpp --output-on-failure
python3 scripts/build_java.py
python3 scripts/test_bindings.py
python3 scripts/test_cmake_install.py
python3 scripts/test_python_images.py
python3 scripts/test_multiformat.py
python3 scripts/test_detail.py
npm test --prefix bindings/javascript
npm run test:browser --prefix bindings/javascript   # installed Google Chrome
```

The demo has its own checks in the web repository.

Install Pillow, NumPy and `zxing-cpp` for image and format checks and PyTorch for
tensor adapters; reference encoders are test dependencies only. Browser parity
and paired timings are described in [comparing scanners](COMPARING_SCANNERS.md).
Generated test images are temporary. These checks establish behavior on the tested inputs; for
what they do not show, see [benchmarks](BENCHMARKS.md).

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
policies with diagnostics on and off. It covers missing, erased, two-/five-digit and
different supplements on identical main payloads. Supplement timing, like any
recorded timing, follows the [benchmark guide](BENCHMARKS.md).

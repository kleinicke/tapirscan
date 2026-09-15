# Python quality checks

Pinned tools: Ruff 0.16.7, ty 0.0.80, mypy 2.3.1.

Ruff previously enabled 106 rules (`E4`, `E7`, `E9`, `F`, `I`, `UP`). It now
selects `ALL`: 799 of this release's 812 stable rules before file-specific
exceptions. Preview rules (141) and removed rules (17) are not enabled.
The count comes from `ruff check --show-settings` for `results.py` and
`ruff rule --all --output-format json`, not a count of selected prefixes.
Pinned versions make rule changes reviewable on upgrade.

Global exceptions are documented in [ruff.toml](../ruff.toml): conflicting
pydocstyle conventions, the formatter-incompatible rules in
[Astral's formatter guide](https://docs.astral.sh/ruff/formatter/#conflicting-lint-rules),
and CPY001 while copyright/license metadata remains undecided. No copyright
ownership was invented to satisfy a linter.

Narrow file exceptions preserve deliberate behavior:

- Existing unittest tests keep unittest assertions/context managers, not pytest rewrites.
- Command-line build/test tools print progress and execute trusted local programs.
- Test entry points can select source vs installed-wheel import paths.
- Optional image dependencies are imported inside the relevant adapter only.
- Scan calls retain flat keyword options instead of requiring an options object.

Pinned imported `core/` snapshots and generated artifacts stay outside lint scope.
All other repository Python is linted and formatted. No blanket noqa was added.

`ty.toml` enables every ty diagnostic as an error and checks the package, scripts,
and static consumer tests. Intentional negative tests have specific ty ignores;
the unused-ignore check ensures those negative cases still produce diagnostics.
Mypy strict independently checks the package and static consumer tests. Neither
checker proves arbitrary runtime input valid: shape/range and native-ABI validation
remain necessary. Typed optional-image adapters do not use Any parameters.

The first ty run caught unsound image-adapter returns, missing override decorators,
and a best-result inference issue accepted by mypy. These were fixed, not suppressed.
Further checks caught optional-result accesses in tests, which now narrow explicitly.
Dynamic raw JSON comparisons in parity tests remain an explicit typed cast boundary.

Run from the repository root with development and imaging dependencies installed:

```sh
ruff check .
ruff format --check .
ty check
MYPYPATH=bindings/python/src mypy --config-file bindings/python/pyproject.toml bindings/python/src/tapirscan bindings/python/tests/typecheck_api.py
```

Use `ty check --python /path/to/environment` when the imaging packages are in a
different environment. CI runs all checks alongside runtime image/native parity
tests. Local validation includes CPU and Apple MPS input; CUDA requires hardware
and remains a conditional test.

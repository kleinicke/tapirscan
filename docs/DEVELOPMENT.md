# Build and develop Tapirscan

## Prerequisites

- [rustup](https://rustup.rs), Python **3.10+** and Node **24**. `rust-toolchain.toml`
  selects Rust 1.91.1 with the WASM target; rustup installs it on first use.
- CMake and a C/C++ compiler for native examples; JDK **22+** for Java (CI uses JDK 25).
- Environment for the quality gate (`QUALITY_PYTHON`, `JAVA_HOME`,
  `.quality-tools/environment.json`): see [quality checks](QUALITY.md#environment).

## Quick start

Run commands from the repository root. The fetch step needs network access; the
builds afterwards run offline.

```sh
cargo fetch --locked --manifest-path multiformat/Cargo.toml   # dependencies, once
cargo fetch --locked --manifest-path bindings/rust/Cargo.toml
node tools/quality/install.mjs        # quality tools and formatting hook
python3 scripts/build_native.py       # native library with all four modes
python3 scripts/build_wasm.py         # WASM files into bindings/javascript/wasm/
npm ci --prefix bindings/javascript && npm run build --prefix bindings/javascript
node tools/quality/all.mjs            # all static checks
```

## Build the library

`build_native.py` and `build_wasm.py` build the prepared public Rust package. The
native library contains all four modes and ordinary Rust packages include them by
default; each WASM file contains one. `build_wasm.py` writes `low.wasm`,
`medium.wasm`, `high.wasm`, `very-high.wasm` and `experimental-turbo{2,4,8,16}.wasm`
to `bindings/javascript/wasm/`. `config/modes.json` lists the modes (their order
defines engine IDs) and Turbo presets; per-mode budgets live in
`bindings/rust/src/effort.rs` and Turbo recipes in `scripts/build_turbo.py`. The
`wasm/build.json` manifest
records a digest of the WASM source inputs (compiled sources, build tools and
`config/modes.json`), the file hashes and each preset's compile-time settings. A
rebuild from unchanged source reproduces the same hashes, and `npm pack` refuses
WASM files whose digest differs from the source tree.

`core/src` is directly editable production source. `python3 scripts/build.py MODE`
runs the selected core's tests. Plain Cargo defaults to Medium; use
`--no-default-features --features mode-low` for another mode. See
[core architecture and mode differences](../core/README.md).

The public package is assembled under `build/crates/tapirscan` by
`scripts/prepare_rust.py`; `--refresh` verifies the repository boundary and updates
generated sources. Generated sources are build outputs, not a second implementation
to edit. Native and WASM builds hold an exclusive lock on that package; a competing
build fails with the lock path, and after an interruption you can remove the lock
once its `owner` process has stopped.

### Reproducible WASM builds

`scripts/wasm_rustc.py` is a Cargo rustc wrapper that replaces path-dependent
symbol metadata so WASM hashes agree between checkout paths; `scripts/test_wasm_rustc.py`
tests it. Native builds use Cargo's standard compiler invocation.

### Development builds

```sh
python3 scripts/build_wasm.py --development [medium]
npm run build --prefix bindings/javascript
```

Development assets go to `build/wasm-development/assets/` with plain mode names and
leave the package assets and `build.json` untouched. A mode argument builds only
that mode; in a package build it records only files built from the current source,
so `npm pack` fails until every file is rebuilt. `TAPIRSCAN_LOW_CLASSIC=1` with a
`low` development build produces the core's Low Classic policy for comparisons.

## Install local packages

```sh
# npm tarball; install the printed filename into your app.
mkdir -p build/packages
(cd bindings/javascript && npm pack --pack-destination ../../build/packages)

# Platform wheel; the native library must already be built.
python3 -m pip wheel --no-deps --wheel-dir build/wheels bindings/python
python3 -m pip install build/wheels/tapirscan-*.whl
```

Python wheels include the native library. In a source checkout, use
`library_dir="build/native"` or `TAPIRSCAN_LIBRARY_DIR`. The wheel builder accepts
`TAPIRSCAN_NATIVE_DIR` for a prebuilt directory and fails if the native library is
missing.
Linux release wheels must be built for the advertised manylinux baseline; see
[release preparation](RELEASING.md).

## Run the demo

The application lives in `../tapirscan-web/demo`; application changes belong in
that repository.

```sh
corepack enable
pnpm --dir ../tapirscan-web/demo install --frozen-lockfile
pnpm --dir ../tapirscan-web/demo build
pnpm --dir ../tapirscan-web/demo preview
```

The demo's `-next` readers use the local library build, so build the WASM files and
the JavaScript package first. Its main readers use the latest npm release. Asset
preparation copies both sets of engines, creates the synthetic example, and copies
the independent comparison engines. See [camera behavior and hosting](../../tapirscan-web/demo/README.md).

## Validate changes

```sh
python3 scripts/verify_sources.py
node tools/quality/all.mjs
```

The gate needs the optional image libraries in `QUALITY_PYTHON` and a JDK in
`JAVA_HOME`. See [quality checks](QUALITY.md) and
[validation](VALIDATION.md) for the focused tests and reproduction commands.
Scanner changes also follow [Changing the scanner](../CONTRIBUTING.md#changing-the-scanner).

## Maintained runtime boundaries

The public Rust package lives in `bindings/rust/api`; `bindings/rust/src` is the
internal per-mode engine assembled by the build scripts. Keep public result types
in the API layer. The engine's `pipeline.rs` orders scanner stages, `detail.rs`
handles crop recovery, `geometry.rs` owns overlap calculations, `read.rs` holds
typed reader evidence and `formats.rs` reconciles it. `result.rs` serializes
optional diagnostics. Public results cross the API boundary as Rust types; scanning
does not construct or parse JSON when diagnostics are disabled.

In JavaScript, `index.ts` exposes the API and `rust-session.ts` owns the WASM
session. Keep scanner decisions in Rust so changes apply to every binding.

Format names, native bits, ordered presets and reserved add-on flags are declared
in `config/formats.json`. Run `python3 scripts/generate_formats.py` after changing
it, then `python3 scripts/generate_formats.py --check`. Generated Rust and
TypeScript declarations are checked in; `verify_sources.py` also checks for drift.
Changing a format bit is an API/ABI change, not a routine registry edit. Adding a
format also needs a reader in `multiformat/`.

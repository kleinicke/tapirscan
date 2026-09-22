# Build and develop Tapirscan

Run commands from the repository root unless a section says otherwise. The first
build needs network access for dependencies; recipe compilation then runs offline.
Allow at least 10 GiB of free disk space for the build tools' safety reserve,
plus space for compiled modes.

## Prerequisites

- Rust **1.91.1**, including the `wasm32-unknown-unknown` target.
- Python **3.10+**, Node **24**, and the `patch` command.
- CMake and a C/C++ compiler for native examples.
- JDK **22+** for Java; CI uses JDK 25.

```sh
rustup toolchain install 1.91.1 --profile minimal --target wasm32-unknown-unknown
cargo +1.91.1 fetch --locked --manifest-path multiformat/Cargo.toml
cargo +1.91.1 fetch --locked --manifest-path bindings/rust/Cargo.toml
python3 scripts/verify_import.py
```

## Build the library

```sh
python3 scripts/build_native.py low medium high very-high
python3 scripts/build_wasm.py
npm ci --prefix bindings/javascript
npm run build --prefix bindings/javascript
npm test --prefix bindings/javascript
```

Both adapters build the prepared public Rust package. `build_wasm.py` verifies
source and binary identities in `provenance/wasm-api-20260922.json`; `--record`
records an intentionally changed build after review and validation.

Use `scripts/build.py MODE`, not a base Cargo feature alias, to reproduce an
individual historical decoder recipe independently. Its artifacts go to
`build/recipe-wasm`, outside the distribution. `--prepare-only` prepares a fresh mode directory without compiling;
`--resume` verifies and finishes an existing build. The builder refuses to overwrite
an existing prepared output. Keep previous build directories when changing recipes.

## Install local packages

```sh
# npm tarball; install the printed filename into your app.
mkdir -p build/packages
(cd bindings/javascript && npm pack --pack-destination ../../build/packages)

# Platform wheel; all four native modes must already be built.
python3 -m pip wheel --no-deps --wheel-dir build/wheels bindings/python
python3 -m pip install build/wheels/tapirscan-*.whl
```

Python wheels include the native libraries. In a source checkout, you can instead
use `library_dir="build/native"` or `TAPIRSCAN_LIBRARY_DIR`. The wheel builder
accepts `TAPIRSCAN_NATIVE_DIR` for a prebuilt directory and fails if a mode is missing.

Linux release wheels must be built/repaired for the advertised manylinux baseline;
a plain `linux_x86_64` development wheel is not the public PyPI artifact.
See [release preparation](RELEASING.md).

## Run the demo

```sh
npm install --global pnpm@10.15.1
pnpm --dir demo install --frozen-lockfile
pnpm --dir demo build
pnpm --dir demo preview
```

The demo uses the locally packaged scanner. Its asset preparation checks WASM
hashes, creates the synthetic example, and copies the independent comparison
engines. See [camera behavior and hosting](../demo/README.md).

## Validate changes

```sh
python3 scripts/verify_import.py
node tools/quality/install.mjs
node tools/quality/all.mjs
```

The gate needs the demo dependencies, optional image libraries in `QUALITY_PYTHON`
and a JDK in `JAVA_HOME`. You can save local tool paths in the ignored
`.quality-tools/environment.json`. See [quality tools](QUALITY.md) and [validation](VALIDATION.md)
for the focused tests, platform matrix, and reproduction commands.

The public package is assembled under `build/crates/tapirscan` by
`scripts/prepare_rust.py`. `--refresh` revalidates recipes and updates generated
sources while preserving compilation caches. The C and WASM adapters select one
mode through Cargo features; ordinary Rust packages include all modes by default.
Generated sources are build outputs, not a second implementation to edit.

Algorithm changes are promoted from exact experiments. Follow
[PROMOTING_CHANGES.md](PROMOTING_CHANGES.md); do not edit frozen inputs or rewrite
hashes simply to make verification pass.

## Maintained runtime boundaries

The public Rust package lives in `bindings/rust/api`; `bindings/rust/src` is the
internal per-mode engine assembled by the build scripts. Keep public result types
in the API layer. The engine's `pipeline.rs` orders scanner stages, `detail.rs`
handles crop recovery, `geometry.rs` owns overlap calculations, and `result.rs`
assembles engine evidence. Public results cross the API boundary as Rust types.

In JavaScript, `index.ts` exposes the API and `rust-session.ts` owns the WASM
session. Keep scanner decisions in Rust so experiments apply to every binding.

Format names, native bits, ordered presets and reserved add-on flags are declared
in `config/formats.json`. Run `python3 scripts/generate_formats.py` after changing
it, then `python3 scripts/generate_formats.py --check`. Generated Rust and
TypeScript declarations are checked in; `verify_import.py` also checks for drift.
Changing a format bit is an API/ABI change, not a routine registry edit. The pinned
decoder implementation still needs its own promotion when adding a new format.

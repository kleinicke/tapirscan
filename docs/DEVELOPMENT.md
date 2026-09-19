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
python3 scripts/verify_import.py
```

## Build the library

```sh
for mode in low medium high very-high; do
  python3 scripts/build.py "$mode"
done
python3 scripts/build_multiformat.py
python3 scripts/build_native.py low medium high very-high
npm ci --prefix bindings/javascript
npm run build --prefix bindings/javascript
npm test --prefix bindings/javascript
```

Use `scripts/build.py`, not a base Cargo feature alias, to reproduce the selected
algorithms. `--prepare-only` prepares a fresh mode directory without compiling;
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

Shared-retail WASMs compile their path dependency inside a generated Cargo
workspace under `build/<mode>/wasm-source`. This keeps dependency identities
independent of the checkout path. Shared WASMs compile only the reader modules
they use, excluding serialization and host-dependent procedural macros; the
reader functions are copied unchanged. The original recipe sources remain hash-verified before
this build-only adapter is applied. Artifact names and before/after hashes are
recorded in `provenance/wasm-reader-subset-20260919.json`.

Algorithm changes are promoted from exact experiments. Follow
[PROMOTING_CHANGES.md](PROMOTING_CHANGES.md); do not edit frozen inputs or rewrite
hashes simply to make verification pass.

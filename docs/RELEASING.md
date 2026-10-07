# Releasing Tapirscan

A release publishes the npm package, the Python wheels, the Rust crate and the
demo. C, C++ and Java use the same scanner, options and result model as Rust,
Python and JavaScript. One native library (ABI 6) contains all four effort modes.
`scan` returns a lightweight values-and-locations result; `inspect` returns the
detailed report. Link [API migration](API_MIGRATION.md) in the release notes.

All release-owned manifests and artifact names use the release version. The demo is
public at [tapirscan.f-kleinicke.de](https://tapirscan.f-kleinicke.de). Publishing the
library, publishing a GitHub release, and updating the demo are separate actions.

The license is **MIT OR Apache-2.0** (at the recipient’s option), copyright © 2026 **Florian Nick**. License files and
author metadata are included in the release packages. Published API compatibility is governed by the [compatibility policy](../CONTRIBUTING.md#api-stability).

## Registry setup

The public repository is [kleinicke/tapirscan](https://github.com/kleinicke/tapirscan).
The package manifests include its URLs. Run `node scripts/check_release.mjs`
to check publication metadata.

For PyPI, sign in as the package owner and verify the trusted publisher in the
project’s publishing settings:

| Field             | Value         |
| ----------------- | ------------- |
| PyPI project      | `tapirscan`   |
| GitHub owner      | `kleinicke`   |
| Repository        | `tapirscan`   |
| Workflow filename | `publish.yml` |
| Environment       | `pypi`        |

The PyPI project exists. No API token is needed with trusted publishing.

For npm, verify the GitHub trusted publisher in the package settings: owner `kleinicke`, repository `tapirscan`, workflow
`publish.yml`, environment `npm`, with direct publishing allowed. The workflow can then publish without a stored npm token.

Official setup: [PyPI pending publishers](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/)
and [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/).
Never commit credentials or paste authentication tokens into issues or chat.

## Build the exact release

1. Run the [development build](DEVELOPMENT.md) and
   [validation checks](VALIDATION.md) at the intended release commit.
2. Verify the [mode settings](../config/modes.json), package
   versions, README examples and changelog. Describe implemented formats as supported, retain documented variant limitations, and label Turbo presets experimental.
3. Run `python3 scripts/build_wasm.py`, then pack npm from `bindings/javascript`.
   Its `prepack` step rebuilds TypeScript and rejects missing WASM files, files
   that differ from `wasm/build.json`, and a build whose source digest differs from
   the current source tree. The package version and git commit identify the release.
4. Build Python wheels with the native library. Install each artifact in
   an isolated environment using `scripts/test_installed_wheel.py`; it decodes
   a known barcode in every mode without `library_dir` or environment overrides.

```sh
python3 scripts/verify_sources.py
node scripts/check_release.mjs
# From the repository root:
mkdir -p build/packages
(cd bindings/javascript && npm pack --pack-destination ../../build/packages)
# From the repository root, after building native modes:
python3 -m pip wheel --no-deps --wheel-dir build/wheels bindings/python
python3 scripts/test_installed_wheel.py build/wheels/EXACT_WHEEL_FILENAME.whl
```

On macOS, set `MACOSX_DEPLOYMENT_TARGET=11.0` for **both** native compilation and
wheel creation when advertising that baseline. Confirm the binary deployment
versions; never lower the wheel tag below a contained library's requirements.

## Platform artifacts

The manual **Build Python release wheels** workflow prepares:

| Platform            | Wheel target      | Validation requirement                                      |
| ------------------- | ----------------- | ----------------------------------------------------------- |
| macOS Apple Silicon | macOS 11+, arm64  | Native tests and isolated installed-wheel scan              |
| macOS Intel         | macOS 11+, x86_64 | Same, on the Intel runner                                   |
| Linux x86_64        | manylinux_2_28    | Build in manylinux, repair/audit, then installed-wheel scan |
| Linux ARM64         | manylinux_2_28    | Same, on the ARM64 runner                                   |
| Windows x64         | win_amd64         | MSVC build and installed-wheel scan                         |

The workflow configuration is not evidence that a target passes. Run it in GitHub
and publish only successful artifacts. These are Python-independent `py3-none`
platform wheels using ctypes, with Python 3.10+ declared in metadata.

The PyPI distribution is wheel-only. Do not upload a Python-only sdist
that cannot reproduce its native libraries. Unsupported platforms can build from
the full Git checkout; musllinux and Windows ARM64 are not currently advertised.

Normal CI also checks the bindings and demo and uploads local npm/wheel artifacts.
These two build workflows **do not publish** to package registries.
The separate `publish.yml` workflow publishes only when explicitly selected.

## Prepare and publish the tested artifacts

1. Push the intended release commit and wait for **Validate scanner and bindings**
   to succeed. Run **Build Python release wheels** on the same commit and wait
   for all five targets to succeed.
2. Run **Prepare or publish release** (`publish.yml`) on that commit, entering
   the validation and wheel workflow run IDs. Leave `publish` set to `none`.
   It checks the source commit, workflow identity, versions, native libraries,
   and all five wheel platforms. It also checks Python distribution metadata and
   installs the npm tarball. Download the resulting `release-bundle` artifact:
   it contains `npm/`, `wheels/`, and `SHA256SUMS`.
3. Review this exact bundle, then tag the validated commit `vX.Y.Z` and push the
   tag. Run the publication workflow **from that tag**, supplying the same two
   successful build run IDs. Select `pypi`, `npm`, or `both` once the corresponding
   trusted publishers are configured. Jobs use the `pypi` and `npm` GitHub
   environments. Publication from an unversioned branch is rejected.
4. If trusted publishing is unavailable, an authenticated local session can publish
   the reviewed npm artifact instead:

```sh
# From the downloaded release-bundle directory:
npm login
npm publish npm/tapirscan-X.Y.Z.tgz --access public
```

This publishes the already-tested tarball without rebuilding it. Use only the
reviewed bundle; do not substitute local development wheels. PyPI uses
five platform wheels and no source distribution. Each release number is final;
corrections use a new patch version.

Create the GitHub release with the changelog, actual platform support, demo link,
and documented format limitations. Verify fresh `npm install tapirscan` and
`pip install tapirscan` installations after publishing. Maven, vcpkg and Conan publication are not part of this process.
Rust publication is described below.

## Rust crate

The Rust crate uses the same version. Preparation packages all four exact
mode recipes and the multiformat readers into one crate. Internal source copies
are generated only for distribution; edit `bindings/rust/api` for the public API
and keep scanner changes under the normal promotion procedure.

```sh
# Use a fresh destination each time. This verifies the repository boundary first.
python3 scripts/prepare_rust.py build/crates/tapirscan
cargo +1.91.1 test --release --manifest-path build/crates/tapirscan/Cargo.toml
cargo +1.91.1 test --release --no-default-features --manifest-path build/crates/tapirscan/Cargo.toml
cargo +1.91.1 clippy --all-targets --manifest-path build/crates/tapirscan/Cargo.toml -- -D warnings -W clippy::all -W clippy::pedantic
# Requires built native libraries, Pillow, and the pinned test-only Zint encoder.
python3 scripts/test_rust_package.py build/crates/tapirscan
cargo +1.91.1 publish --dry-run --manifest-path build/crates/tapirscan/Cargo.toml
```

Inspect `target/package/tapirscan-X.Y.Z.crate` inside the prepared package. It must
contain only Rust sources, manifests, license, README, tests, small text fixtures
and provenance. No native binaries, private images, model weights, credentials or
repository-relative dependencies belong in the archive. The generated build
script only emits fixed private cfg flags; it never downloads or patches code.

After the macOS/Linux CI checks pass on the release commit and publication is
authorized, authenticate locally with `cargo login`, then publish that prepared
source package:

```sh
cargo +1.91.1 publish --locked --manifest-path build/crates/tapirscan/Cargo.toml
```

Cargo credentials stay outside the repository. Verify a fresh consumer using
`tapirscan = "X.Y.Z"` from crates.io after publication. Publication is permanent
for a version; fixes need a new version. See the
[Cargo publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html).

## Demo deployment

After publishing a new npm version, add it to the demo with one alias, for
example `pnpm --dir ../tapirscan-web/demo add tapirscan-1-3-0@npm:tapirscan@1.3.0`. The newest
alias becomes the demo's main Tapirscan and earlier ones move under
"Previous releases"; the `-next` readers always show the current repository build.
Rebuild and test `../tapirscan-web/demo/dist`, then deploy to your selected Netlify site:

```sh
pnpm --dir ../tapirscan-web/demo build
pnpm --dir ../tapirscan-web/demo test
cd ../tapirscan-web/demo
netlify deploy --prod --dir dist --site YOUR_SITE_ID
```

The maintainer’s existing deployment details are in the optional, ignored
`MAINTAINER.local.md`. The demo includes only explicitly authorized photos. Its assets and comparison
engines stay out of npm and Python wheels. Do not introduce research datasets or
unapproved photographs during a release.

## Web application repository

The demo is maintained and built in `../../tapirscan-web/demo`. Library release
checks no longer require the demo or synchronize its application version. Follow
the web repository README for app validation. Deployment remains a separate,
explicitly authorized operation. Run demo commands from the web repository.

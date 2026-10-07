# Releasing Tapirscan

The `publish.yml` workflow publishes the npm package, the Python wheels and the
Rust crate from tested CI artifacts. The GitHub release and the demo update are
separate steps. Link [API migration](API_MIGRATION.md) in the release notes when
the API changes.

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

npm and crates.io use the same trusted-publisher fields: owner `kleinicke`,
repository `tapirscan`, workflow `publish.yml`, with environment `npm` for npm
and `crates` for crates.io. Configure them in the
[npm package settings](https://docs.npmjs.com/trusted-publishers/) and the
[crates.io crate settings](https://crates.io/docs/trusted-publishing). No stored
registry token is needed.

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
   it contains `npm/`, `wheels/`, `crates/` and `SHA256SUMS`.
3. Review this exact bundle, then tag the validated commit `vX.Y.Z` and push the
   tag. Run the publication workflow **from that tag**, supplying the same two
   successful build run IDs, and select `all` (or `pypi`, `npm`, `crates` to
   publish one registry). Publication from an unversioned branch is rejected.
   Each release number is final; corrections use a new patch version.

Create the GitHub release with the changelog, actual platform support, demo link,
and documented format limitations. After publishing, verify fresh installs with
`npm install tapirscan`, `pip install tapirscan` and `cargo add tapirscan`.

## Rust crate

The crate has the same version. CI assembles it with `scripts/prepare_rust.py`
(one package containing all four modes and the format readers), tests and lints
it, and uploads the packaged `.crate`, which `publish.yml` publishes. To check it
locally:

```sh
python3 scripts/prepare_rust.py build/crates/tapirscan   # fresh destination
cargo +1.91.1 test --release --manifest-path build/crates/tapirscan/Cargo.toml
python3 scripts/test_rust_package.py build/crates/tapirscan
cargo +1.91.1 package --manifest-path build/crates/tapirscan/Cargo.toml
```

The archive contains only Rust sources, manifests, license, README, tests, small
text fixtures and provenance.

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

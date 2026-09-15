# Releasing Tapirscan

The first npm/PyPI release is **1.0.0**. The demo is already public at
[tapirscan.netlify.app](https://tapirscan.netlify.app). Publishing the library,
publishing a GitHub release, and updating the demo are separate actions.

The license is **MIT**, copyright © 2026 **Florian Nick**. License files and
author metadata are included in the release packages. The documented 1.0 API
is frozen under the [compatibility policy](../CONTRIBUTING.md#api-stability).

## Registry setup

The public repository is [kleinicke/tapirscan](https://github.com/kleinicke/tapirscan).
Both package manifests include its URLs. Run `node scripts/check_release.mjs`
to check publication metadata.

For PyPI, sign in as the package owner and add a pending publisher at
<https://pypi.org/manage/account/publishing/>:

| Field             | Value         |
| ----------------- | ------------- |
| PyPI project      | `tapirscan`   |
| GitHub owner      | `kleinicke`   |
| Repository        | `tapirscan`   |
| Workflow filename | `publish.yml` |
| Environment       | `pypi`        |

The first successful upload creates the PyPI project. Pending publisher setup
is not a name reservation. No API token is needed.

For npm's first publication, log in with `npm login` and publish the reviewed
release tarball using the command below. Complete interactive authentication
when requested. Once the package exists, add a GitHub trusted publisher in its
npm package settings: owner `kleinicke`, repository `tapirscan`, workflow
`publish.yml`, environment `npm`, with direct publishing allowed. Subsequent
releases can use the workflow without a stored npm token.

Official setup: [PyPI pending publishers](https://docs.pypi.org/trusted-publishers/creating-a-project-through-oidc/)
and [npm trusted publishing](https://docs.npmjs.com/trusted-publishers/).
Never commit credentials or paste authentication tokens into issues or chat.

## Build the exact release

1. Run the [development build](DEVELOPMENT.md) and
   [validation checks](VALIDATION.md) at the intended release commit.
2. Verify the [mode manifest](../provenance/modes.json), source hashes, package
   versions, README examples and changelog. Keep additional formats experimental.
3. Pack npm from `bindings/javascript`. Its `prepack` step rebuilds TypeScript
   and rejects stale mode selections, missing recovery files, or incorrect WASMs.
4. Build Python wheels with all four native libraries. Install each artifact in
   an isolated environment using `scripts/test_installed_wheel.py`; it decodes
   a known barcode in every mode without `library_dir` or environment overrides.

```sh
python3 scripts/verify_import.py
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

| Platform            | Planned wheel target | Validation requirement                                      |
| ------------------- | -------------------- | ----------------------------------------------------------- |
| macOS Apple Silicon | macOS 11+, arm64     | Native tests and isolated installed-wheel scan              |
| macOS Intel         | macOS 11+, x86_64    | Same, on the Intel runner                                   |
| Linux x86_64        | manylinux_2_28       | Build in manylinux, repair/audit, then installed-wheel scan |
| Linux ARM64         | manylinux_2_28       | Same, on the ARM64 runner                                   |
| Windows x64         | win_amd64            | MSVC build and installed-wheel scan                         |

Only macOS arm64 has been checked locally in this preparation. The workflow
configuration is not evidence that the other targets pass. Run it in GitHub and
publish only successful artifacts. These are Python-independent `py3-none`
platform wheels using ctypes, with Python 3.10+ declared in metadata.

The first PyPI distribution is wheel-only. Do not upload a Python-only sdist
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
3. Review this exact bundle, then tag the validated commit `v1.0.0` and push the
   tag. Run the publication workflow **from that tag**, supplying the same two
   successful build run IDs. Select `pypi`, `npm`, or `both` once the corresponding
   trusted publishers are configured. Jobs use the `pypi` and `npm` GitHub
   environments. Publication from an unversioned branch is rejected.
4. For the first npm upload, use an authenticated local session instead:

```sh
# From the downloaded release-bundle directory:
npm login
npm publish npm/tapirscan-1.0.0.tgz --access public
```

This publishes the already-tested tarball without rebuilding it. Use only the
reviewed bundle; do not substitute local development wheels. PyPI starts with
five platform wheels and no source distribution. Each release number is final;
use a new patch version for subsequent corrections.

Create the GitHub release with the changelog, actual platform support, demo link,
and experimental format limitations. Verify fresh `npm install tapirscan` and
`pip install tapirscan` installations after publishing. Crates.io/Maven/vcpkg/
Conan publication is outside the initial npm/PyPI launch.

## Demo deployment

Rebuild and test `demo/dist`, then deploy to your selected Netlify site:

```sh
pnpm --dir demo build
pnpm --dir demo test
cd demo
netlify deploy --prod --dir dist --site YOUR_SITE_ID
```

The maintainer’s existing deployment details are in the optional, ignored
`MAINTAINER.local.md`. The demo retains the four explicitly authorized photos. Its assets and comparison
engines stay out of npm and Python wheels. Do not introduce research datasets or
unapproved photographs during a release.

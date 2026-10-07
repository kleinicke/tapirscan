# Contributor and coding-agent guide

Tapirscan contains the release library and language bindings. The browser demo
and benchmark applications live in `../tapirscan-web`. Application builds and
publication are managed in that repository.
Start with [development](docs/DEVELOPMENT.md), [quality checks](docs/QUALITY.md)
and the guide for the binding you change.

## Scanner invariants

Preserve multiple-barcode scanning, undecoded coverage, source-image geometry,
explicit work limits and support-based ranking. Do not silently introduce a
neural model or reference-decoder fallback. Performance claims need reproducible
paired evidence.

`core/src` and `multiformat/` are maintained production source: format and lint
them like any other source. In isolated experiment worktrees, edit them
directly. To select a mode, use one `mode-*` feature or `scripts/build.py MODE`.
Read [the core guide](core/README.md) for stage boundaries and mode differences.
The four Turbo presets (2/4/8/16) are public experimental API; keep all four
building and tested.

Builds compile current production source; the package version and git commit are
the release identity. `scripts/verify_sources.py` checks the repository boundary
and generated formats. See [Changing the scanner](CONTRIBUTING.md#changing-the-scanner)
for where experiments live and how a change is integrated.

## Changes and verification

Implement the documented APIs. `scripts/check_release.mjs` checks that all
language packages carry the same version. Follow the compatibility policy in
[CONTRIBUTING.md](CONTRIBUTING.md#api-stability) and [API design](docs/API_DESIGN.md).

- Follow [quality checks](docs/QUALITY.md); format a coherent batch before running relevant checks.
- C, C++, Python and Java share native ABI 6: one library containing every mode.
  ABI changes need cross-language parity and installation tests; preserve
  ownership and error behavior.
- Keep research datasets, private labels, model weights and generated build outputs
  out of Git. Public demo assets have separate provenance and usage information.
- Before any model training or scanner evaluation, consult
  `../tapirscan-datasets/datasets/TESTSET_OVERVIEW.md` and
  `../tapirscan-datasets/datasets/metadata/testset_catalog.json`. The all-format
  variation test set and other listed suites are development evaluation data,
  not untouched holdouts. Exclude their source groups and derivatives from
  training when claiming unseen results.
- The demo is a separate application and is excluded from language packages.
  Local builds do not publish it. Release publication is a separate operation.

## Optional local setup

If `MAINTAINER.local.md` exists at the repository root, read it for machine-specific
paths and maintainer workflow notes. It is ignored by Git, optional, and not a
prerequisite for contributing. It must not contain credentials. Public project
requirements belong in this file or the linked documentation.

## Geometry invariants

Keep display geometry separate from physical ownership: bounded barcode-profile
agreement may widen a displayed outline but never changes ownership decisions.
The demo's Turbo readers use the current build's `experimental-turbo*.wasm`.

## Related repositories

- `../tapirscan-web`: demo and benchmarks. Reserved release-benchmark data may
  only be used to evaluate a version after it has been released.
- `../tapirscan-experiments`: scanner experiments and their evidence.
- `../tapirscan-datasets`: shared data, labels and saved observations.

Read the relevant repository's `AGENTS.md` when working there.

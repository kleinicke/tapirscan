# Contributor and coding-agent guide

Tapirscan contains the release library, language bindings and browser demo.
Start with [development](docs/DEVELOPMENT.md), [quality checks](docs/QUALITY.md)
and the guide for the binding you change.

## Scanner invariants

Preserve multiple-barcode scanning, undecoded coverage, source-image geometry,
explicit work limits and support-based ranking. Do not silently introduce a
neural model or reference-decoder fallback. Performance claims need reproducible
paired evidence.

`core/src` and `multiformat/` are maintained production source. Edit them directly in isolated
experiment worktrees; select one `mode-*` feature or use `scripts/build.py MODE`.
Read [the core guide](core/README.md) for stage boundaries and mode differences.
Format and lint both like any other source. Research archives and unused prototypes belong in the
separate experiment workspace; see [repository boundaries](docs/RESEARCH_BOUNDARY.md).
Preserve all Turbo variants: they are intentionally retained for future API work.

Builds compile current production source; the package version and git commit are
the release identity. `scripts/verify_sources.py` checks the repository boundary
and generated formats. Follow [promotion](docs/PROMOTING_CHANGES.md) when
integrating experimental algorithm changes.

## Changes and verification

Implement the documented APIs and keep the language package versions
synchronized. Follow the compatibility policy in
[CONTRIBUTING.md](CONTRIBUTING.md#api-stability) and [API design](docs/API_DESIGN.md).

- Follow `docs/QUALITY.md`; format a coherent batch before running relevant checks.
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
Recovery crops use exact integer interpolation; preserve its byte-equivalence
tests. The demo's Turbo readers use the current build's `experimental-turbo*.wasm`.

## Reserved retail production-release benchmark

`../tapirscan-datasets/datasets/release-benchmarks/retail-food-20261005/` is reserved for production releases. Read its `AGENTS.md` and `training-exclusions.json` before selecting scanner inputs. **Never use its source hashes, product groups, aliases or derivatives for training, optimization, development experiments, or casual benchmarks**, even via older datasets or manifests. Only the expressly authorized initial reference baseline and authorized frozen production-release evaluations are permitted. Historical exposure is documented; do not call it an untouched holdout.

## Required runtime for comparisons

All benchmark scanner measurements must use the library's JavaScript API within installed Google Chrome, including WASM loaded by that API. Native Chrome uses its JavaScript BarcodeDetector API. Do not substitute Node, Python, native command-line or direct Rust timing. Record Chrome version, exact package/artifact identity and scan timing boundaries. Node may orchestrate Chrome; it must not perform the measured scanning.

# Contributor and coding-agent guide

Tapirscan contains the release library, language bindings and browser demo.
Start with [development](docs/DEVELOPMENT.md), [quality checks](docs/QUALITY.md)
and the guide for the binding you change.

## Scanner invariants

Preserve multiple-barcode scanning, undecoded coverage, source-image geometry,
explicit work limits and support-based ranking. Do not silently introduce a
neural model or reference-decoder fallback. Performance claims need reproducible
paired evidence.

`core/src` is the maintained production algorithm. Edit it directly in isolated
experiment worktrees; select one `mode-*` feature or use `scripts/build.py MODE`.
Read [the core guide](core/README.md) for stage boundaries and mode differences.
`historical/`, `multiformat/` and imported JavaScript hosts remain hash-pinned.
Never edit or format those historical inputs to make a build pass.

Development builds verify frozen history and compile current production source.
After parity and performance validation, record a new runtime source snapshot and
new immutable WASM identities. `scripts/verify_import.py` checks the release
snapshot as well as history. Follow [promotion](docs/PROMOTING_CHANGES.md) when
integrating experimental algorithm changes.

## Changes and verification

This checkout prepares version 1.2.2, following the API revision described in
`docs/API_MIGRATION.md`. Implement the current documented APIs and preserve the
synchronized release versions. Follow the compatibility policy in
[CONTRIBUTING.md](CONTRIBUTING.md#api-stability) and [API design](docs/API_DESIGN.md).

- Follow `docs/QUALITY.md`; format a coherent batch before running relevant checks.
- C, C++, Python and Java share the native ABI. ABI changes need cross-language
  parity and installation tests; preserve ownership and error behavior.
- Keep research datasets, private labels, model weights and generated build outputs
  out of Git. Public demo assets have separate provenance and usage information.
- The demo is a separate application and is excluded from language packages.
  Local builds do not publish it. Release publication is a separate operation.

## Optional local setup

If `MAINTAINER.local.md` exists at the repository root, read it for machine-specific
paths and maintainer workflow notes. It is ignored by Git, optional, and not a
prerequisite for contributing. It must not contain credentials. Public project
requirements belong in this file or the linked documentation.

## Current selected scanner (2026-09-24)

The canonical source includes `59d8a55` (resolution-aware full barcode footprints),
selected by the user as the next production candidate before the remaining broad
checks. Use this as the current baseline; do not restore the older footprint code.
Validation status: [footprint promotion](docs/FOOTPRINT_PROMOTION_20260924.md).
Turbo is a separate demo scanner pinned by `demo/src/lib/turbo.json`; preserve it.

# Contributor and coding-agent guide

Tapirscan contains the release library, language bindings and browser demo.
Start with [development](docs/DEVELOPMENT.md), [quality checks](docs/QUALITY.md)
and the guide for the binding you change.

## Scanner invariants

Preserve multiple-barcode scanning, undecoded coverage, source-image geometry,
explicit work limits and support-based ranking. Do not silently introduce a
neural model or reference-decoder fallback. Performance claims need reproducible
paired evidence.

`core/` and `bindings/javascript/src/host.ts` are hash-pinned imported snapshots.
Changes require synchronized provenance, mode recipes and parity evidence.
Use `scripts/build.py` for selected modes; plain Cargo aliases in the imported
base crate do not include the selected patches. Follow
[the promotion procedure](docs/PROMOTING_CHANGES.md) when importing research changes.

## Changes and verification

The documented 1.0 API is frozen. Follow the 1.x compatibility policy in
[CONTRIBUTING.md](CONTRIBUTING.md#api-stability); avoid further API redesign
during release preparation.

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

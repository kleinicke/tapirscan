# Changelog

## 1.0.0 — unreleased

- License the project under MIT, copyright 2026 Florian Nick.
- Freeze the documented 1.0 API and add JavaScript ZXing migration guidance.

- Add typed Rust multi-format results, a format enum and composable format presets;
  retain raw schema-2 JSON through `scan_formats_json` for the unchanged C ABI.
- Refresh Java, C++, C and Rust integration guides and test their code snippets;
  document how to scaffold and validate additional language bindings.

- Set the first public release version to 1.0.0.
- Add the development story and simplify effort-mode guidance for users.

- Promote the complete source-detail Medium, High and Very high pipelines, with
  unchanged Low mode and default EAN-13 format selection.
- Preserve recovered barcode geometry and undecoded crop evidence across native
  bindings and the browser demo.
- Bundle all four native modes into platform-specific Python wheels and load them
  automatically from an installed package.
- Add developer guides, a source-backed ZXing/ZBar comparison, release packaging
  checks and clean-install smoke tests.
- Keep the public [camera/photo demo](https://tapirscan.netlify.app) available with
  separate ZXing and ZBar comparisons.

See [the algorithm promotion](docs/PROMOTION_DETAIL_20260914.md) for exact hashes
and the scope of validation. Package publication is still pending.

## 0.1.1 — local Medium promotion, unreleased

- Select `medium-evidence64-pinned-20260914` for Medium/default; keep prior assets.
- Preserve the public API and other effort levels. Measured tradeoff: 21.2% lower P95 and 6.5% lower mean, with eight lost detections on the 4,117-image paired research run.
- Pin Rust 1.91.1 and record toolchain-alias-only byte differences plus focused parity.
- See `docs/PROMOTION_MEDIUM_20260914.md`. This is a local promotion, not publication.

## 0.1.0 — unreleased

- Adopt Tapirscan branding and `tapirscan` package/import names.
- Promote the pinned low, medium (default), high and very-high EAN13 effort modes.
- Add opt-in experimental linear and matrix readers, including QRCode.
- Preserve full UTF-8 payloads and format names across native ABI 3 and all bindings.
- Add a public camera/photo demo using the release JavaScript API, with three
  authorized example photos and separate ZXing/ZBar comparisons.
- Extend provenance, cross-language parity, package installation checks and CI.

See [promotion notes](docs/PROMOTION_20260913.md) for provenance and limitations.
These historical entries describe unpublished development snapshots. The demo is
now public; current publication requirements are in [the release checklist](docs/RELEASING.md).

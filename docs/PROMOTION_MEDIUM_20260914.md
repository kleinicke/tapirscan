# Medium evidence64 promotion, 14 September 2026

User-approved local promotion on the existing `feat/tapirscan-promotion` branch.
Current tag: `medium-evidence64-pinned-20260914`. Previous:
`very-fast-v2-lint-20260913`. Other efforts and public binding APIs are unchanged.
JavaScript/demo package version is 0.1.1 (unreleased).

The recipe under `core/experiments/` is an exact patch against the declared
frozen base. No base scanner files were overwritten. `provenance/import.json`
records only intentional new recipe/helper hashes; the prior import manifest is
preserved in `provenance/import-before-medium-20260914.json`.

Research binary SHA-256: `647a6f8b759b93133d9f9b4dd2e36d050edc7b8b008fbb9ac4d1d8349ffcb038`.
Pinned binary SHA-256: `b94b7f4f18b6b9ba2669586e5aaa7897d3848b3f3285691fb03df16fa2af07d8`.
All executable sections are identical. Only 54 data bytes differ, replacing
embedded `stable` Rust paths with `1.91.1`; normalization makes files identical.
An 18-image parity check matched proposals, candidates, work, barcodes and
reconciliation exactly. No source-level decoder change accompanied this build.

The user accepted eight known regressions for 6.5% lower mean and 21.2% lower
P95 in a 4,117-image paired browser study. The 1.5× ZXing target remains unmet.
Full research/manual evidence remains in the workbench, not in this release tree:
`benchmark/reports/medium_evidence64_manual_review_20260914.md`.
No datasets, private images, labels or neural weights were imported.

Old build artifacts are archived at `build/medium-before-evidence64-20260914`.
The local demo keeps the prior Medium WASM asset for frozen comparisons. Cached
historical dataset results must not be relabeled as the new implementation.

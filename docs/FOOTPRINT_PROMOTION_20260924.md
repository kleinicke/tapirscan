# Current footprint version

Selected source: `59d8a55f040a91f413b645eceac8997159a2c1ad`, integrated into `main`.
Build ID: `1.2.2+footprint-fast.20260924`. Package version remains the unreleased 1.2.2.

The user requested selection before the remaining broad checks. The source,
four-mode runtime binaries, SDK defaults and local demo default select this
version. Runtime manifests and demo/release packaging updates remain local
working-tree changes; the source commit is independently available on `main`.

## Validation on 2026-09-24

- All four modes, Retail and Common1D: 11,656 control comparisons and 13,168
  private gallery comparisons against `434b2c3`; no decoded-value or count changes.
- Controls: no per-image geometry-match losses at IoU 0.01, 0.3 or 0.5.
  Additional matches at IoU 0.5 (Retail / Common1D): Low 9 / 13,
  Medium 11 / 21, High 6 / 15, Very High 6 / 15.
- Native/WASM replay: 1,384 comparisons. Strict parity has two pre-existing
  mismatches on one crowded UPC-A image in High (Retail and Common1D), unchanged
  on baseline and candidate. Both backends return the same 31 values; support,
  one polygon and undecoded regions differ. This is not a clean strict-parity pass.
- 1,249 Rust unit tests, 7 API tests and 2 doc tests passed; strict all-target
  Clippy passed. JS tests, demo worker checks, Svelte checks, package identities,
  release metadata and source provenance checks passed.
- Desktop/mobile browser checks passed for the local version selector, real
  scans, switching back and version-specific WASM requests.
- Paired browser timing: 49 images, 3 warmups, 5 repetitions, 1,960 paired
  measurements. Group mean runtime changes range from 0.84% faster to 0.05%
  slower; essentially unchanged overall on this cohort. All 392 timing-cohort
  correctness comparisons preserve decoded values and counts.

TS-Turbo remains separately pinned by `demo/src/lib/turbo.json`; its manifest and
WASM bytes are unchanged. No package tag, push or public deployment is part of
this update.

Retained local evidence is in the sibling experiments workspace under
`retained/footprint-fast-promotion-20260924/`; the preceding implementation
investigation is `retained/footprint-fast-20260924/REPORT.md`.

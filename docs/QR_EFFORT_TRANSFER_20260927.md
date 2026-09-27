# QR effort transfer — 2026-09-27

Baseline: `42a219a47933047e477ec496282d798914c96aec`. Experiment:
`qr-effort-transfer-20260927` in the sibling experiment workspace.

## Implemented

High and Very High now use the maintained QR frontend's exact alignment sample
coordinate cache and early version-header rejection. Their original extra
thresholds, integer sharpening, search order, ranking, source geometry and work
limits remain unchanged. Very High retains its 24-attempt curved-grid recovery.
The sharpening and curved sampler bodies were relocated unchanged from the
frozen reader; frozen source files were not edited.

This applies to QR-only reader groups, including QR alongside linear formats.
Groups containing QR and another matrix symbology still use the existing shared
multiformat reader. This is not an all-format speed claim.

## Paired browser measurements

Identical decoded pixels, QRCode selection, three repetitions per image and
alternating engine order in a quiet Chrome session on the recorded local host.
The 466-image screen reuses the prior study's development cases, including hard
and reference-only cases; it is not an unseen or representative phone benchmark.
Fetch and image decoding are excluded; scanner API upload and result assembly
are included. Tables use the median repetition for each image.

| Mode      | Median before → after | p95 before → after | Total time reduction |
| --------- | --------------------: | -----------------: | -------------------: |
| high      |        36.5 → 30.1 ms |     94.4 → 76.6 ms |                16.2% |
| very-high |        38.0 → 32.0 ms |     95.0 → 78.2 ms |                15.8% |

On all 4,619 development images, every returned field apart from elapsed time
is identical before and after in all four public modes (18,476 comparisons).
High retains 4,128 and Very High 4,131 of 5,142 known instances.
This transfer improves speed, not High/Very High detection quality. Public WASM
assets grow by about 17–20 kB (0.9–1.2%) from retaining both reader paths.

## Private Turbo builds

All five fresh private recipes inherit the earlier Low QR improvements. Their
QR outputs match current public Low on all 466 screen images per tier. The
numbered tiers keep their own linear policies; their numbers are not QR speed
multipliers. A separate 156-image, three-repeat browser comparison against the
immutable September 25 Turbo pins measures the cumulative QR improvement:

| Private recipe | Total QR time reduction vs pinned September 25 build | Gained / lost known instances |
| -------------- | ---------------------------------------------------: | ----------------------------: |
| original       |                                                15.2% |                         1 / 0 |
| 2              |                                                14.0% |                         1 / 0 |
| 4              |                                                14.1% |                         1 / 0 |
| 8              |                                                13.9% |                         1 / 0 |
| 16             |                                                13.8% |                         1 / 0 |

Build current artifacts with `python3 scripts/build_turbo.py TIER --native --wasm`.
Outputs are under `build/private-turbo/TIER/`; select these files explicitly using
the Low adapter ABI. The historical demo pins in `demo/src/lib/turbo.json` remain
unchanged. Nothing was published.

## Validation and retained evidence

- Exact full-corpus native output parity in all four public modes.
- Exact High/Very High parity on 756 repeated-symbol, polarity, nested, damaged
  and negative controls (1,512 comparisons).
- Native/WASM parity on 132 raw-pixel inputs (264 comparisons), including
  grayscale/RGB/RGBA, padded rows, mixed QR/linear formats and extended budgets.
  The same inputs also preserve the frozen native baseline exactly.
- 297 core tests; strict core checks in all four configurations; strict public
  Rust/C/WASM checks; public Rust and JavaScript tests; immutable asset parity.
- Fresh native and WASM artifacts for original Turbo, Turbo2, Turbo4, Turbo8 and
  Turbo16; 2,330 native QR comparisons against current Low.

Browser observations and environment identities are retained in the dataset
workspace at `datasets/metadata/qr-effort-transfer-20260927/`. Compact comparisons,
policy audit, hashes and exact commands are in the experiment workspace under
`retained/qr-effort-transfer-20260927/` and `experiments/qr-effort-transfer-20260927/`.
These are development evaluation suites; no model was trained.

Full release-snapshot verification passes in the clean experiment worktree. The
canonical checkout retains unrelated pre-existing edits to
`demo/scripts/prepare.mjs`, which its strict snapshot check flags; those edits
are preserved and are not absorbed into the scanner provenance.

## Next quality experiment

Medium recovers 103 known instances absent from High and 100 absent from Very
High; 102 and 100, respectively, occur on empty higher-mode results. A bounded
Medium-style empty-result fallback is promising, but it needs a separate paired
quality/runtime trial. It is not included in this exact-output optimization.

# High / Very High QR recovery — 2026-09-27

Baseline: `a52f0068f73f68e9f339bc2124a38386c07b2ad6`. Experiment:
`qr-high-quality-20260927` in the sibling experiment workspace.

## Recovery policy

High and Very High retain their existing QR threshold, sharpening and curved-grid
searches. They now also use the bounded original-gray grid and foreground
recovery paths previously available to Medium. Existing successful reads are
retained; unresolved physical regions remain eligible for grid recovery.

Higher-mode original-gray recovery has a one-million-sample budget, considers
up to eight pending QR regions and sixteen qualified grids, and tries nine
sampling phases including independent horizontal/vertical shifts. Low and Medium
retain their original 250,000-sample, eight-grid, three-phase policy.

For QR-only requests with no decoded result, both higher modes can revisit a
large source at a 1200-pixel maximum side. Very High can additionally try 1600
pixels when 1200 remains unresolved. The existing 960-pixel recovery is retained.
The new scale passes include qualified gray-grid sampling and foreground
thresholds before mapping every result back into source coordinates.

For unresolved QR-only images of at most 2,097,152 pixels, higher modes can use
stronger sharpening, histogram normalization and reversed polarity. These are
bounded retries, not a learned restoration model or a reference-decoder fallback.
No new full-frame retry suppresses earlier detections or terminates an ordinary
multi-symbol scan merely because it found one result. QR crop retries also use
the existing optimized maintained frontend; other matrix crops retain their
shared reader.

Original-gray recovery applies to QR-only reader groups, including QR alongside
linear formats. The new full-frame restoration and resolution retries require
an explicit QRCode-only format selection and ignored EAN supplements. This is
not a general all-format quality or speed claim. Public APIs, ownership, ranking
and the separately pinned private Turbo policies are unchanged.

## Evaluation

Final measurements are recorded with the selected source and immutable assets.
The study reuses 4,619 development images with 5,142 known QR instances. It uses
physical one-to-one matching, exact payloads and IoU of at least 0.5 where polygon
truth exists. The paired browser screen contains 645 deliberately difficult and
control images; it is not representative of a phone-camera workload or an unseen
holdout. No training is involved.

Three newly recovered screen results reveal annotation issues (HTML entities,
a trailing newline, and an omitted URL colon). Independent ZXing decoding of
cropped/resized or sampled source pixels verifies their payloads. The frozen
labels remain unchanged and these results are excluded from scored gains.
One is a stylized QR with text over its modules; this single example does not
establish general stylized-QR coverage.

See the retained experiment report for measured quality, runtime, ablations,
source identities, controls and validation outcomes.

## Measured result

| Mode      | Before | After | Gained / lost |
| --------- | -----: | ----: | ------------: |
| medium    |   4206 |  4206 |         0 / 0 |
| high      |   4128 |  4341 |       213 / 0 |
| very-high |   4131 |  4346 |       215 / 0 |

The full replay contains 4,619 development images and 5,142 known QR instances.
High and Very High miss 0 and 0 Medium instances respectively. Very High adds 5 over High, with 0 lost High cases.

| Mode      | Browser median before → after | p95 before → after | Total time ratio |
| --------- | ----------------------------: | -----------------: | ---------------: |
| high      |              30.10 → 41.00 ms |  76.72 → 136.86 ms |            1.632 |
| very-high |              32.00 → 44.20 ms |  80.28 → 187.86 ms |            1.615 |

These are paired 645-image challenge-screen timings, with three repetitions and alternating order. Larger ratios mean slower. The added recovery trades additional failure-case work for quality; this is not a universal speedup claim.

## Stylized diagnostics and runtime distribution

On 240 generated derivatives of four QR symbols (versions 4/7, M/H correction),
High and Very High decode 220, versus 212 for their prior versions and 216 for
Medium. ZXing-WASM decodes 188 and native Chrome 218 on the same fixtures.
Every plain, colored, gradient and protected rounded-module case passes.
The strongest artwork overlays, all-module dots and largest center logos account
for failures. This is a controlled diagnostic, not general artistic-QR coverage.

On the main paired screen, already-successful frames use 0.986× / 0.982× total
time in High / Very High; formerly empty frames use 2.787× / 2.724×. Thus ordinary
successful-frame time is essentially unchanged; the quality gain comes with
substantially more work on difficult empty results.

High and Very High close 34 and 38 of Medium's 97 reference-union gaps in the
browser screen. The remaining 63 and 59 known instances show that the reference
readers remain complementary. Selection favors diagnostic cases, so it must not
be interpreted as a representative ranking of general camera workloads.

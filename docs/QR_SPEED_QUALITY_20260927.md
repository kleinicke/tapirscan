# QR speed and quality — 27 September 2026

The validated experiment improves Low and Medium on the frozen development
selection with no lost known baseline detections. The experiment baseline is `ac440e0e5ea8402d950c9580ecb64f9832ec2fa7`.
The experiment record and detailed evidence live in the sibling experiment
workspace under `experiments/qr-speed-quality-20260927` and
`retained/qr-speed-quality-20260927`.

## Final paired results

| Reader          | Known QR / 5,142 | Median ms | p95 ms | Mean ms |
| --------------- | ---------------: | --------: | -----: | ------: |
| Low before      |             4025 |     25.90 |  56.50 |   25.79 |
| Low selected    |             4137 |     21.40 |  52.30 |   22.65 |
| Medium before   |             4146 |     27.20 |  63.90 |   27.95 |
| Medium selected |             4206 |     22.00 |  60.20 |   25.63 |
| ZXing-WASM      |             3611 |      7.40 |  77.70 |   17.84 |
| Chrome native   |             3496 |     25.10 |  70.50 |   32.59 |

Low gains 112 known instances, loses 0, and uses 12.2% less summed per-image median scanner time.
Medium gains 60 known instances, loses 0, and uses 8.3% less summed per-image median scanner time.

In the Barber photo cohort (1,347 known instances), selected Low recovers 1093 and selected Medium 1098, versus ZXing 1092 and Chrome 1121. Low's photo gain is 89; Medium's is four. Most of Medium's additional aggregate quality gain is on synthetic probes.

Low’s Barber photo p95 rises from 55.47 to 59.09 ms despite its lower mean and median. Medium’s inputs with maximum side at most 640 pixels use 16.6% more aggregate time; its discovery retries buy additional recovery there. These changes do not make every frame faster.

## Implementation

Ordinary QR finder search now lives in `core/src/qr_frontend`. The payload/ECC
decoder and physical region ownership remain shared with the pinned multiformat
implementation. Imported history is unchanged. Higher-effort QR and groups with
several enabled matrix formats keep their existing reader. The new frontend is
used when a reader group selects QR alone, including QR grouped alongside linear
formats. Performance comparisons below select QR only.

The alignment search calculates each sampled x coordinate once per horizontal
candidate offset and each y coordinate once per vertical offset. It reuses those
coordinates across the candidate grid, preserving the original floating-point
operation order, floor/cast behavior, template order, scoring, tie order and
selected centers. It does not lower finder limits or decoding-attempt budgets.
An exact six-bit version prefix also rejects impossible single-finder version
headers before sampling their remaining bits.

Low adds a 960-pixel retry after an empty QR-only result on a frame whose maximum
side exceeds 1,920 pixels. This is additive: a successful reduced image never
replaces the full-resolution search. Both Low and Medium can resample existing
QR regions directly from original grayscale pixels using bounded bilinear grids.
That recovery tests at most eight regions, eight selected grids per region and
250,000 bilinear sample points; it retains source-image coordinates and the
shared ECC/physical-instance checks.

Medium additionally tries lower and upper foreground intensity splits, in both
polarities, on empty QR-only frames without a surviving region and with at most
2,097,152 pixels. It runs only those extra thresholds. If still empty, inputs no
larger than 262,144 pixels may use the existing integer sharpening kernel and
another bounded QR search. Binary images skip this retry because sharpening
cannot change their clipped pixels and neither nested histogram has a split.
These are explicit per-frame work limits, not time-dependent early exits.

## Measurement and scope

The frozen development selection contains 4,619 images and 5,142 known QR
instances. Exact payload recovery is matched one-to-one by physical instance
with polygon IoU at least 0.5 where geometry exists (170 instances have payload-only labels). Unknown payloads, unmatched reads and label
disagreements are separate; reference-reader agreement does not change truth.
The 315 variation images derive from only five originals. No model training or
unseen-test claim is involved.

Browser comparisons use identical decoded RGBA pixels, three repetitions and
rotating engine order. Times include input upload and result materialization;
image decoding and engine initialization are excluded. The measured references
are ZXing-WASM 3.1.1 and Chrome's native BarcodeDetector on an Apple M1 Pro Mac.
Chrome uses the macOS Vision backend on this platform, so these results do not
predict Android or Windows performance.

The aggregate is strongly affected by 1,000 damaged-finder synthetic images.
Source-specific photo results and per-case reference wins must accompany it.
The retained report also documents why scaling-first replacements, early
single-symbol completion, explicit SIMD conversion, alternate compiler profiles
and several exact caches were rejected.

## Validation

The exact cache prototype preserved all 9,238 native Low/Medium result objects,
excluding elapsed time. The selected implementation passes the full 4,619-image
native and paired browser replay, plus 100 ordinary/repeated/polarity controls,
432 nested/damaged controls and 224 negative controls, with no lost known reads.

Native/WASM parity passes 900 comparisons over 450 inputs, including all final
Medium gain cases, multiple pixel layouts and mixed selectors. Core tests pass
294 unit tests and three integrations. The public Rust package passes 1,444 unit
tests plus API/doc tests with default and no-default features. Python image tests
run 25 tests with two unavailable GPU-backend skips. All-mode native/WASM builds
and strict Rust checks pass. Final package/import checks and exact equality
between measured development WASMs and immutable release assets are retained in
the experiment's promotion evidence.

Package and demo publication are separate operations.

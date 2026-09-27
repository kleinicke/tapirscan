# Bounded low-resolution EAN-13 recovery

This change promotes the useful subset of the real-photo multirow experiments.
It improves automatic full-image EAN-13 decoding without requiring supplied
regions or a clean scanline. It does not add a reference decoder, a learned model,
a payload dictionary, or checksum-guided correction.

## Method and effort policy

Existing stripe proposals and transition evidence nominate unresolved small
regions. Guard/quiet-zone projection calibrates the active edges within each
proposal. Original pixels from several heights contribute to a subpixel profile;
small shear and nonlinear width adjustments account for misalignment and mild
warping. The strongest visual text must pass checksum, runner-up separation,
quiet-zone, disjoint-pixel agreement and per-digit spatial-support checks.
Repeated contradictory evidence rejects the read. Geometry stabilization keeps
the winning visual payload fixed and cannot repair a checksum.

| Mode      | Admission and search                                                                             | Recovery bank per candidate-scanner transaction |
| --------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------------: |
| Low       | Unchanged                                                                                        |                                               0 |
| Medium    | Existing guard evidence; skip repeated invalid visual reads; one calibrated region, small search |                                               2 |
| High      | Stripe evidence; bounded angle and region alternatives; small search                             |                                               4 |
| Very High | High admission, with a wider fallback                                                            |                                               6 |

These banks also consume the existing frame and per-candidate retry limits and
shared source-association budget. A complete scan may contain multiple existing
candidate-scanner transactions; the bank is not a promise of six searches for an
entire facade scan. Candidate dimensions are bounded to 45–190 pixels along the
bars' varying axis and 12–110 pixels across their height, with at most 16,000
square pixels before region calibration. Exhaustion retains pending coverage.

Medium's extra admission reuses evidence already collected by ordinary scanning;
it does not add an image-wide search. Barcode-like negatives can still resemble
real codes, so the latency guarantee is measured on specified controls rather
than an assertion that every barcode-free image costs exactly the same.

Recovered reads enter normal source-continuity association and frame identity
checks. Their temporary geometric anchors represent distributed evidence, not
independently decoded scanlines. Existing cached observations and search coverage
remain intact. Normal physical ownership and precise display-footprint handling
remain shared across Medium, High and Very High.

## Automatic full-image development results

Baseline: `a52f0068f73f68e9f339bc2124a38386c07b2ad6`. Before final validation,
the candidate also incorporates main's separate QR improvements at `7be6edc`.
EAN-only comparisons use the captured baseline libraries. These are development
selections and related derivatives, not independent unseen camera captures.

| Cohort                                  | Cases | Medium before → after | High before → after | Very High before → after |
| --------------------------------------- | ----: | --------------------: | ------------------: | -----------------------: |
| Downscaled photos, 25 source groups     | 1,920 |             676 → 687 |           777 → 838 |                773 → 841 |
| Mild distortion/blur derivatives        |   320 |               96 → 97 |           127 → 140 |                126 → 140 |
| Original source images, 22 known values |    21 |               20 → 21 |             20 → 21 |                  21 → 22 |
| Existing original-photo development set |   500 |             451 → 451 |           453 → 453 |                453 → 454 |

Counts are distinct correct values per image. The paired cohorts have no lost
baseline correct values or newly accepted wrong values. Existing wrong reads
remain and are not concealed by these counts: the 500-photo set has 7/7/6 wrong
values in Medium/High/Very High, unchanged. The downscaled set retains one wrong
value per mode. Twenty atlas negatives return no accepted values; 720 mixed and
invalid controls add no wrong values over baseline. A broader research-only
Medium gate recovered 738/1,920, but failed the negative latency gate and was not
selected for Medium.

On `IMG_6084.PNG`, Medium now reads green `4104420031326`. High and Very High read
both green and blue `8076809586177`. Previously Medium read neither and High/Very
High read only blue. The green improvement comes from active-edge calibration
and multirow evidence; no expected payload or hand-drawn region reaches the scan.

The real-photo derivatives locate the useful range more precisely. Very High
improves 2 → 20 of 360 at 0.65 nominal pixels/module and 26 → 69 of 360 at 0.8;
High reaches 16 and 67. All modes still read 0/240 at 0.5 in this automatic
full-image cohort. These are source-resampled derivatives, not sensor-resolution
certification. Most extremely small symbols remain unresolved.

The distortion cohort includes 80 matched controls. Very High's separate outcomes
are control 34 → 41, 20% keystone 28 → 28, one-module bow 30 → 31, and 0.35-pixel
blur 34 → 40. Thus the automatic perspective gain is zero in this check; supplied
region research results must not be substituted for end-to-end performance.

## Timing, parity and scope

The experiment retains paired native timings over 81 fixed images: 20 atlas
negative controls, 20 visually inspected background crops, and 41 barcode images.
Pixels are preloaded, scanners reused, two warmups precede nine alternating
paired trials. Native API conversion/materialization is included. On the selected
Medium gate, negative medians are 6.98 → 6.93 ms and 1.89 → 1.87 ms; negative p95
is 19.57 → 19.54 ms and 7.15 → 7.07 ms. Barcode-image median is 17.11 → 17.49 ms.
This is one desktop host, not a mobile latency claim.

Higher efforts intentionally spend more on unresolved candidates. Their prior
quiet native comparison has barcode-image median 56.78 → 57.55 ms for High and
62.37 → 99.62 ms for Very High. The median per-image ratios are 1.15 and 1.19;
cohort medians and paired ratios summarize different distributions. Difficult
barcode-like negatives can also cost more in these modes.

Chromium on the same 81 fixed inputs uses two warmups and seven interleaved
trials. Medium's median paired ratio is 1.00 in both negative cohorts. On the
41 barcode images, cohort median is 20.60 → 20.10 ms in Medium, 70.20 → 71.40 ms
in High and 85.70 → 121.50 ms in Very High. Their p95 values are 42.80 → 42.50,
205.60 → 205.40 and 275.30 → 301.30 ms. Median per-image ratios are 1.00/1.12/1.17.
There are no lost correct values, new wrong values or unstable repeated outputs
in that browser screen. Very High's atlas-negative p95 grows 78.05 → 171.00 ms;
higher-effort recovery is not free on plausible but invalid barcode patterns.

For the real `IMG_6084.PNG`, browser EAN-13 timings are:

| Reader               |   Before |    After | Candidate reads |
| -------------------- | -------: | -------: | --------------- |
| Medium               |  27.8 ms |  39.7 ms | Green           |
| High                 | 111.7 ms | 118.9 ms | Green and blue  |
| Very High            | 156.3 ms | 243.5 ms | Green and blue  |
| ZXing-WASM reference |        — |  16.0 ms | Neither         |

High offers the better tradeoff on this particular image. The reference reader
remains complementary: this deliberately difficult selection does not establish
a general scanner ranking. The website's `ts-med-next`, `ts-high-next` and
`ts-vhigh-next` cards select the new frozen preview; stable defaults and older
version identities remain unchanged. Local registration does not publish a site.

Final browser measurements, exact source/library identities, full comparison
observations and validation logs are retained with experiment
`lowres-integration-20260927` in the experiment and dataset workspaces. The parity
audit checks identical gray/RGB/RGBA pixels, padded strides, default/extended
budgets, repeated EAN-13 labels and a mixed QR scene. Eight pre-existing High-mode
native/WASM diagnostic differences on one image are separately preserved; decoded
values and decoded polygons agree. Do not describe this as bitwise equality of
all diagnostics.

## Remaining limits and future direction

These results justify shipping bounded recovery, not a universal minimum
resolution claim. Fractional pixels per module can be recoverable when rotation
provides complementary samples across height. Axis-aligned aliasing, severe blur,
missing quiet zones, tiny localization failures and strong deformation can remove
that evidence. Upscaling cannot reconstruct absent information.

Very High remains the intended place to tackle difficult low-resolution
localization and distorted multirow recovery as development continues. It should
eventually handle more of these cases automatically; this is a development goal,
not a promise that the present mode solves every low-resolution barcode. Further
broad research is deferred. Medium should only inherit future techniques when
candidate admission and paired negative timing protect ordinary scanning.

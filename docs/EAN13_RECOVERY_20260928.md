# Bounded EAN13 reference recovery

Experiment `ean13-reference-recovery-20260928` continues the accepted Medium V54
work from `medium-ean13-native-20260928`, on main baseline `73b2461`. It also
integrates the selected Low Aztec budget described below. This is development
evidence, not an untouched holdout or a package publication.

## Selected policy

Medium extends bounded short-profile soft evidence and independent-row agreement.
An exact two-entry profile cache avoids repeated identical sampling without
changing values or thresholds. High and Very High share the exact cache,
including their longer profiles. Source luminance uses the existing exact integer
conversion.

After the original scan, unresolved proposals with guard or strong localization
and blur evidence receive bounded original-pixel retries. Medium considers two
proposals; High and Very High consider six. A contrast-profile retry is bounded
at 768 samples in Medium. Up to two eligible proposals can also use RGB-maximum
profiles when the source actually has sufficient chromatic contrast, requiring
four independent confirming rows. This does not replace frame grayscale or alter
already established physical reads.

Optional hypotheses protect original reconciled ownership. Newly admitted
same-value bands must also have separated measured extents before being treated
as separate physical symbols. Existing frame reads retain their ordinary strict
ownership rules. Spatially separate same-value labels remain supported.

A further source-region retry uses two unresolved guard-bearing proposals, a
20% plus four-pixel margin, five-tap Gaussian contrast restoration and a shared
pixel cap of 131,072 in Medium or 262,144 in High/Very High. It cannot recurse
into another restoration retry. Admission requires at least five supporting
rows, agreement with the original proposal, no established overlapping owner,
and no repeated contradictory visual evidence in the original source image.

**Default Medium Retail/Common scanning skips this region-restoration pass.**
It remains available for EAN-only scanning and explicit candidate completion.
The cheaper original-profile improvements still apply to combined modes.

High and Very High additionally run an isolated original-region threshold retry,
with 16/32 candidate/frame paths in High and 64/128 in Very High. The retry flag
is reset on both success and error. Keeping this pass separate protects reads
that earlier inline threshold experiments lost. It requires four supporting rows
and the same source contradiction check.

All additions use observed pixels and explicit work bounds. No reference decoder,
model, payload lookup, annotation geometry or dataset-specific routing is used.
Normal pending-region, geometry, multi-symbol and ranking contracts are retained.
Work caps are not wall-clock latency guarantees.

## Evidence and limitations

The full EAN13 corpus contains 6,980 registered complete images, 6,975 readable
images and 6,420 valid known EAN13 targets. It combines real photographs,
registered variations and fixtures; source groups and derivatives are correlated.
Malformed annotations, unknown values and partial coverage are retained in the
audit. Scoring uses exact payloads with one-to-one instance matching; strict
polygon scoring is reported separately. No annotations were silently corrected.

Controls include 760 invalid/negative frames, 3,928 non-EAN frames, 1,035
other-Retail frames and a 443-frame combined-format panel. Quality diagnostics
run separately from quiet installed-Chrome performance tests. The latter use
identical decoded ImageData, warmups, alternating method order and three
repetitions; mean and median are calculated from per-image median scan time.
Input upload and result materialization are included; file decoding is excluded.

Native full-corpus Medium improves 5,416 to 5,571 known targets with no lost
targets. This includes 95 from accepted V54 and 60 from this continuation.
Aggregate duplicate flags remain 48 and wrong-value flags 10. Some individual
flags change: the annotation audit distinguishes a verified label error, a
valid second pen, incomplete multi-symbol labels, and a genuine glare-split
duplicate inherited from V54. High improves 5,553 to 5,643 with no lost targets.
Its added wrong flags include a verified label error and an unverified partially
visible book code; they remain in the frozen score.

Higher total recall does not establish per-image superiority over native Chrome
or ZXing. Remaining reference-only targets and failed variants are retained with
exact patches, artifact identities, observations and revisit conditions in the
experiment workspace. Wider retries that introduced genuine wrong reads or
lost original targets were rejected.

## Low Aztec selection

Low now uses the maintained Aztec frontend with 10,000 refinement samples,
20,000 ordinary gray-grid samples, no bow or adaptive-grid retries, and 8,192
Rune confirmation samples per reader invocation. Medium/High/Very High retain
the previously merged full Aztec budgets.

Low known hits improve 301 to 404 on the initial 864 images and 1,354 to 1,689
on the additional 5,126 native cases, without baseline target losses. The initial
and additional quiet Chrome panels are respectively 38% and 12% faster than
Medium, but 36% and 47% slower than old Low. This is an accuracy/latency tradeoff.
All 4,320 non-Aztec controls remain empty. Common excludes Aztec, so its timing
alone does not measure Aztec cost.

Full tables, paired intervals, controls and integration identities live in
`tapirscan-experiments/retained/ean13-reference-recovery-20260928/REPORT.md` and
`tapirscan-experiments/retained/aztec-low-budget-20260928/REPORT.md`. Raw journals
remain under the corresponding `tapirscan-datasets/datasets/metadata/` folders.

## Final Chrome confirmation

Installed Chrome 153.0.8010.54 confirms Medium 5,416 to 5,571 and High
5,552 to 5,643 known targets, without baseline target losses. Native Chrome
finds 5,456 and ZXing 4,565. Medium retains 48 duplicate and 10 wrong-value
flags in aggregate; High has 82 and 19, including the audited extra pen and
annotation/partial-symbol issues. Medium still misses 188 native and 114 ZXing
targets; High misses 174 native and 98 ZXing targets. The sets overlap.

Quiet paired Chrome timings, three repetitions, mean / median milliseconds:

| Mode / active formats |             Main |        Candidate | Mean change |
| --------------------- | ---------------: | ---------------: | ----------: |
| Medium / EAN13        |   28.518 / 24.20 |   30.728 / 26.20 |      +7.75% |
| Medium / Retail       |   33.073 / 28.00 |   34.137 / 29.00 |      +3.22% |
| Medium / Common1D     |   38.471 / 32.30 |   39.710 / 33.40 |      +3.22% |
| Medium / Common       |   68.543 / 49.90 |   69.817 / 50.80 |      +1.86% |
| High / EAN13          |  108.930 / 97.05 |  115.624 / 99.25 |      +6.15% |
| High / Common         | 167.959 / 153.80 | 179.532 / 156.70 |      +6.89% |
| Very High / EAN13     | 141.880 / 122.80 | 152.961 / 128.15 |      +7.81% |
| Low / Common          |   38.049 / 22.70 |   38.717 / 23.90 |      +1.75% |

EAN/Retail use 496 original photos; Common panels use 443 mixed-format frames;
Very High EAN uses a deterministic 256-photo subset. The final Low panel uses
the cleaned-up artifact. Non-Low V69 executable sections equal measured V68
exactly; their only data changes are verified Rust panic source-line locations.
New whole-binary hashes are recorded rather than reusing old identities.
Download and initialization costs were not measured.

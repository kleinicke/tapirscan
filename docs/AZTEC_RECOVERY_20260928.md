# Bounded Aztec recovery

Candidate from baseline `343cb96671f3b4b96e60ddf0527164974ca14331`.
Experiment: `aztec-common-budget-20260928`. This is development evidence,
not an untouched accuracy holdout or a package publication.

## Selected algorithm

Medium, High and Very High use the maintained Aztec reader. Matrix-only reader
invocations share the existing QR/DataMatrix threshold images and row runs;
the ordering and policies of the other readers are preserved. Common currently
excludes Aztec. Explicit Common + Aztec exercises this additional reader.

Recovery uses nested foreground thresholds, source-gray bullseye fitting,
bilinear module samples, small sampling offsets, and bounded compression/bow
hypotheses. A final local-mean module threshold handles uneven illumination.
New sampled-grid recovery preserves the independently read compact/full mode,
layer count, data-word count and reader-initialization flag. This prevents a
plausible but incompatible configuration from admitting a wrong payload.

Corrected Rune interpretations require finder agreement plus confirmation from
a nearby binary sample or a source-gray sample with fewer error corrections.
Unconfirmed regions remain pending. The immutable error-correction field tables
are reused; the underlying arithmetic, payload parser and error correction are
unchanged. Low retains its original reader.

Sampling limits per reader invocation are 100, 000 bullseye refinement samples,
150, 000 ordinary gray-grid samples, 50, 000 bow samples, 50, 000 local-threshold
samples and 8, 192 Rune confirmation samples. Search preserves multiple symbols,
source coordinates and explicit unfinished status. The facade can invoke the
reader again within its own bounded recovery stage; these are not frame-global
sample limits or latency guarantees.

## Evidence and limitations

Inputs are complete source images: the initial 864-image development collection,
194 additional distinct real photos, and 4, 932 additional uncropped cylindrical
renders. Derivatives and repeated payload/model groups are correlated. The real
photo extension has only four payload groups and the renders six model groups.
They do not establish generalization to new captures or all Aztec configurations.

Separate controls cover 4, 320 non-Aztec frames and 4, 638 reviewed mixed-format
frames. The preserved journals include target identities, wrong/unmatched reads,
duplicates, geometry, pending regions and runtime. Binary payload annotation
representation issues are reported separately rather than silently corrected.

Paired installed-Chrome measurements use identical decoded ImageData, alternating
method order and warmups. Scan time includes the public API, upload and result
materialization; it excludes image decoding and scanner construction. Quiet
timing runs are separate from quality screens performed during compilation.
Aztec-only recovery has a larger cost than the combined-format average; both
must be considered when choosing an effort policy.

Final quality, runtime and validation tables are recorded with the experiment.
Promotion requires fresh runtime/WASM provenance and matching artifacts from
independent checkout paths. No external decoder is used by production scanning.

## Measured candidate

All measurements use the candidate from experiment `aztec-common-budget-20260928`.
On the initial 864-image collection, Medium improves 358→468 known targets;
High 359→468 and Very High 360→468. No mode loses a previously correct target or
adds a wrong-value/duplicate case; Low is unchanged. Excluding Rune fixtures,
Medium ordinary-Aztec hits improve 250→334, compared with native Chrome 260.
The 315 all-format Aztec variations improve 91→137, versus native 105.

The 5126-image extension improves Medium 1406→2074, versus native Chrome 1626 and
ZXing WASM995. Wrong values fall 53→0 with zero duplicates. Its194 real photos
improve 57→72, versus native 73: the rendered-corpus lead does not establish
real-photo superiority. Photos and renders have only four and six payload/model
groups respectively. Source groups and derivatives must stay out of training
for any unseen-evaluation claim.

Paired quiet Chrome 153 on Apple M1 Pro, public Medium scan API:

| Selection / panel      | Mean before → after (ms) | Median before → after (ms) | p95 before → after (ms) |
| ---------------------- | -----------------------: | -------------------------: | ----------------------: |
| Common /443            |            69.03 → 68.87 |              49.80 → 49.70 |         189.28 → 187.91 |
| Common + Aztec /443    |            71.78 → 73.65 |              50.60 → 52.90 |         204.48 → 213.57 |
| All /443               |            96.64 → 98.20 |              71.80 → 72.80 |         271.77 → 284.02 |
| Aztec / initial 864    |              4.61 → 6.42 |                2.10 → 2.30 |           20.38 → 31.67 |
| Aztec / additional 594 |            14.07 → 21.81 |               9.55 → 17.35 |           37.47 → 54.61 |

Common panels use five repetitions per image; All and Aztec-only panels use
three. Values summarize per-image median scan times. Common excludes Aztec;
the explicit Common + Aztec row measures the additional reader. Aztec-only
recovery has a substantial cost despite the small combined-mode increase.
The 443-frame panel represents existing source/augmentation groups, not a new
independent holdout or a mobile/video latency benchmark.

Native controls preserve every non-Aztec decoded record across 4638 reviewed
mixed-format atlas frames and eliminate eight false Rune reads across 4320
non-Aztec controls. Targeted native/WASM parity covers352 scans across all modes
and both extended-budget settings. Exact commands, source/input/engine hashes,
failed variants, per-target outcomes and raw Chrome repetitions are retained in
the experiment and dataset workspaces.

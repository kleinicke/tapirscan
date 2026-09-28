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

Sampling limits per reader invocation are 100,000 bullseye refinement samples,
150,000 ordinary gray-grid samples, 50,000 bow samples, 50,000 local-threshold
samples and 8,192 Rune confirmation samples. Search preserves multiple symbols,
source coordinates and explicit unfinished status. The facade can invoke the
reader again within its own bounded recovery stage; these are not frame-global
sample limits or latency guarantees.

## Evidence and limitations

Inputs are complete source images: the initial 864-image development collection,
194 additional distinct real photos, and 4,932 additional uncropped cylindrical
renders. Derivatives and repeated payload/model groups are correlated. The real
photo extension has only four payload groups and the renders six model groups.
They do not establish generalization to new captures or all Aztec configurations.

Separate controls cover 4,320 non-Aztec frames and 4,638 reviewed mixed-format
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

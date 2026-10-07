# Production scanner core

Edit `src/` directly. `Cargo.toml` exposes four mutually exclusive effort modes:
`mode-low`, `mode-medium` (default), `mode-high`, and `mode-very-high`.

```sh
cargo test --manifest-path core/Cargo.toml
python3 scripts/build.py low
python3 scripts/build_native.py
```

The public Rust package compiles private mode instances from this same source
through `scripts/prepare_rust.py`. Those generated modules are build output.
They are necessary to offer several compile-time policies in one Rust process;
there is only one maintained algorithm tree.

## Stage boundaries

- `aztec_frontend` owns Medium/High/Very High Aztec finder recovery, bounded
  source-gray refinement and sampling, and Rune confirmation. Its payload
  decoder reuses immutable error-correction fields. The facade's
  `matrix_frontend` shares the original threshold images and row runs with
  other matrix readers.

- `qr_frontend` owns ordinary QR finder search and bounded foreground-threshold
  recovery. Its separable alignment-coordinate tables preserve exact search
  order and budgets; QR payload/ECC and region ownership stay shared with the
  pinned multiformat reader. `qr_grid` resamples unresolved regions from original
  grayscale pixels. The Rust facade selects QR-only reader groups and enforces
  the mode-specific frame retry limits.

- `candidate_scanner.rs` owns candidate evidence and reusable scratch buffers and runs
  the initial candidate pass. `candidate_scanner/sampling.rs` samples and normalizes
  profiles; `candidate_scanner/decoding.rs` invokes decoders and records accepted or
  conflicting observations; `candidate_scanner/association.rs` assembles observations
  with source-continuity proofs and shared budgets.
- `multi_scan/plan.rs` constructs retry paths. `multi_scan.rs` executes them in
  their defined order, enforces budgets and refreshes evidence-dependent effort.
  `verified_coverage.rs` proves which intervals can safely reuse prior work.
- `lowres.rs` and `lowres/photo.rs` recover unresolved small EAN-13 regions by
  fusing original pixels across height, calibrating active edges, and requiring
  distributed visual agreement before checksum acceptance. Medium requires prior
  guard evidence and rejects repeatedly invalid visual reads; higher modes use
  larger bounded searches.
- `frame/identity.rs` owns physical overlap and pending-coverage geometry.
  `frame/conflict.rs` proves identity and resolves competing values from pixels.
  `frame.rs` reconciles candidates and assembles the final frame.
- `retail_pipeline.rs` returns typed retail detections. Optional diagnostics are
  serialized only when requested by the public scanner.

Keep allocations reusable where the scanner already owns scratch. A new sampling
method should return observations through the existing acceptance stage. A new
retry strategy should produce `Segment` plans without changing acceptance or
frame reconciliation.

## Mode differences

Public Low uses the Turbo fast path in the Rust facade; the core Low policy below
provides its recovery and the Low Classic development build.

Low uses module-axis discovery, bounded retries and sparse stripe work. Medium
adds evidence-dependent retry caps and shared short-retail work. High adds
native-soft decoding and more retries. Very High adds stronger identity/conflict
checks and additional source/grid refinement. The image facade's fit limits are
0/1/4/1, and source-detail recovery uses the Low core. Mode-specific arithmetic
is intentional: replacing `hypot` with an algebraically equivalent norm can
change borderline source decisions.

Mode differences use `mode-*` conditions; passive timing and tracing are optional
compile-time features, not configuration options.

## Source and provenance

The package version and git commit identify a build; WASM files carry a source
digest in `build.json`. Turbo presets and Low Classic have their own build
settings, separate from the four public effort modes. See [repository boundaries](../docs/RESEARCH_BOUNDARY.md).

## Localization and retry execution

`stripes::Detector` owns reusable raster, gradient and tile storage. Its stages
are `stripes/raster.rs` (sampling, contrast conditioning and gradients),
`groups.rs` (components, merges and growth), `proposals.rs` (projection and dense
bands), and `refinement.rs` (source-pixel refinement and bounded assembly).
The binding keeps one detector per scanner and a separate reusable grayscale
buffer for additional readers. Returned proposals own their data. The free
localization functions remain useful for one-off calls; scanner sessions reuse
storage across frames, including the secondary raster pass.

`multi_scan/execution.rs` coordinates fixed scanning, round-robin discovery,
effort selection, retries, confirmation and final assembly. Its `Execution`
state owns the shared association/reuse budgets and consumed path count.
`multi_scan/plan.rs` constructs paths; execution preserves candidate order and
pending-work accounting. New retry strategies should change planning without
changing acceptance or reconciliation. Mode-specific numerical evaluation remains
explicit where replacing square roots with `hypot` would alter exact results.

The candidate-scanning state is `CandidateScanner` in `candidate_scanner.rs`. Shared `Policy` defaults have
one initializer, with mode-only fields selected at compile time. Host refinement
limits and recovery directions are listed in `bindings/rust/src/effort.rs`.
Frame reconciliation names its remaining-budget, observation-ordering and
source-alias decisions explicitly while retaining shared budget ownership.

## Warped linear observations

Sparse envelope fits require strongly sloped, consistent boundaries. The regular
per-mode fit limits stay unchanged. Medium and above can also try two full-frame
fits on a small image with no accepted proposal, provided the localizer's latest
raster already contains coherent stripe evidence. Blank backgrounds avoid this
extra source sampling.

The binding completes the existing duplicate proof first. Remaining matching
linear interpretations can use distributed strong ink/paper continuity or bounded
bar tracing with light borders. These original proofs share a 32,768-pixel budget.
Tracing tolerates approximate box endpoints on folded labels only after checking
the continuous path; it does not treat overlapping boxes or matching text alone
as evidence of one physical barcode. Curved-band consolidation keeps a supported
observed polygon instead of extrapolating an unverified full-symbol envelope.

Bar tracing checks ink at half-pixel steps and looks for a light border at three
bounded radii as projected bar widths change.
Medium detail recovery also consults completed short-retail reads: a supported
symbol with at least three source pixels per module can cover a seed through its
observed polygon or the existing continuous-profile proof. Small and weak reads
retain the enlarged rescan to refine their geometry. This skips a recovery seed,
not every candidate intersecting the enclosing crop.

Crossing decoded bands can prove shared ownership by following distributed source
bars to the other decoding line. This avoids assuming equal fractional positions
in two warped polygons refer to the same physical bar. At least six of eight
sampled bars spanning 60% of the reading width must connect, with light borders
along their paths. An endpoint-directed trace is the fallback for strong
projective shear. The consolidation pass has a 32,768-pixel evidence limit;
a separate ownership pass gets at most 32,768
additional samples, only for remaining ambiguous decoded results.

A weak ITF interpretation (support at most two) can be discarded when a
checksum-validated retail read has support at least three, the decoded polygons
substantially overlap, and distributed source bars establish shared ownership.
Overlap of search proposals alone never triggers this rule. Separate labels,
stronger ITF reads and ITF reads without a competing retail read are unaffected.

After reconciliation, a bounded physical-footprint pass samples
12 distributed bars and follows them in both directions to visible endpoints.
Local ink/paper contrast adapts slowly to illumination; interpolated half-step
checks stop at light gaps. At least nine reliable tracks must span 80% of the
reading width; uncertain individual tracks are omitted. Persistent sharp
lighting changes cannot certify physical endpoints. Neighboring tracks must have coherent extents, so a
seed band crossing two adjacent labels cannot silently enclose both as one.
Unstable fits and exhausted work budgets keep the decoded polygon.

A successful footprint supplies a four-corner approximation of the observed
full bar area. Its tracked ink paths, never overlap of the enclosing quad, prove
physical ownership. Lower-ranked or tied supported linear interpretations using
the same distributed source bars are suppressed, including different payloads
and formats; distinct supplement/reader metadata remains separate. Stable
support order breaks ties. The whole additional pass is capped at 262,144 source
samples, and each direction is capped at 1,536 cross-sections. Returned quads
approximate curved boundaries; they are not pixel-exact masks or certificates
that unseen/occluded bars were recovered.

This pass runs after decoding. It does not skip pending search candidates;
that requires separately verified coverage in the search scheduler.

Symbols at least 512 source pixels wide use adaptive cross-section spacing.
Width/contrast measurements advance by up to
six pixels while intervening ink is checked with interpolated half-pixel samples.
Both outside tracks must finish before accepting a large-symbol envelope.
Endpoint lookahead and illumination adaptation scale with bar width and physical
advance. Smaller symbols use half-pixel tracing throughout. The shared
262,144-sample budget applies to both.

## Very High subpixel recovery

Very High includes bounded EAN-13 recovery from original pixel areas and legal
digit sequences inside small affine candidates. Only unresolved candidates enter
the recovery bank. Independent disjoint bands must agree; visual conflicts,
source continuity, association and frame reconciliation still apply. Low, Medium
and High do not compile this path. It does not provide subpixel localization or a
minimum-resolution guarantee, and does not apply to QR or Data Matrix.

## EAN13 and Low Aztec recovery

The runtime uses bounded source-profile evidence and exact profile reuse
for EAN13. Medium default combined modes use the guarded source-region exception described
below; High and Very High retain bounded region/threshold retries. Low uses a
smaller Aztec gray-sampling budget. All retries preserve physical ownership checks.

## Bounded Retail recovery

Medium, High and Very High try additional short-code contrast and polarity
hypotheses only when the frame has no established read. Each probe examines at
most four existing proposals and requires agreement across source rows. Optional
positive-contrast UPC-E recovery requires seven-module quiet runs, except at an
actual image boundary. A proposal boundary inside the image is not an image edge.
The quiet-space
threshold is an empirical recovery safeguard, not a printing-conformance test.

Medium can reuse up to four rejected narrow stripe groups from the original
localizer after ordinary recovery fails. This avoids a second full localization
pass when the primary proposals are available. At the full-image source-profile stage, its wider EAN13 contrast retry
requires a single supported hypothesis, agreement with the input luminance on
at least two separated source rows, and no contradictory source reading. When
that decoder is reused inside a restored crop, the profile agreement concerns
the restored crop pixels; final crop admission separately requires support of
at least five and contradiction/ownership checks against the original image.
These checks corroborate a decoded payload; they do not repair digits, invoke a
reference scanner or prove that every alternative payload is impossible.

In default Medium combined scans, an empty primary Retail result with sufficient
existing guard evidence can trigger one source-region restoration, limited to
262,144 source pixels. EAN-only and extended-budget scans restore up to two
regions of at most 131,072 pixels. These are different bounded search
policies; increasing effort does not guarantee that every individual result is
retained. A matrix code already found in a Common frame can prevent the later
optional empty-frame short-code probes. Ordinary multi-symbol scanning continues.

Medium additionally averages three parallel source lines for bounded EAN
recovery and revisits at most two deferred proposals. A seven-row preflight
can stop an unpromising band retry. Probe observations are discarded; a plausible
value triggers the full row sequence and independent source proof.
The probe restores threshold history and confirmation state before that pass. If the original-image
pass still has no Retail result after its established recovery, it can try one
alternate EAN crop, capped at 131,072 source pixels, and requires original-source
agreement before admitting a result.

For EAN8/UPC-E, Medium can relocalize at most two unresolved source regions
when the image exceeds the primary 768-pixel working dimension,
using an overlap filter to avoid repeatedly selecting the same area. Each region is capped at 1,048,576 source pixels and reduced to
at most 32,768 localization pixels; the combined localization budget is 65,536.
Source-width ordering and an equal per-region allowance prevent one region from
using the entire budget. Only localization uses reduced pixels. Decoding and
confirmation use the original image, require multiple supported rows, and
preserve existing physical owners. This pass can recover an additional label
in a frame that already contains a read. Extra-crop UPC-E reads must span at
least 60% of the source proposal width, rejecting narrow internal fragments of
a wider label. This is a conservative admission heuristic, not a proof that
every narrow symbol inside a wide proposal is invalid.

Medium reuses up to 64 exact source profiles and their thresholds inside one
immutable-image transaction. Image layout, coordinates, density and sample limit
identify entries. Transformations invalidate threshold reuse, a new transaction
clears the active entries, and overflow uses ordinary uncached sampling.

Low adds one bounded Retail contrast pass when its normal fast path has no read,
using at most two existing proposals and a bounded continuity-work budget. EAN13/UPC-A claims also
need independent original-source agreement and no contradiction. Medium and Low
can consult visual EAN8 evidence on optional source profiles, with observed quiet
space and existing-owner checks. High, Very High and the Turbo presets do not
use these passes.

Medium also tries the existing distributed-bar ownership proof in the reverse
direction before retaining a duplicate interpretation. High and Very High can
reject an overlapping UPC-E fragment in favor of an EAN13/UPC-A read with support
at least three and no weaker than the fragment. Both rules still require shared
source-bar continuity; equal text or overlapping bounding boxes are insufficient.
All modes skip source sampling when no requested linear formats remain active.

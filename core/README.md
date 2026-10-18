# Production scanner core

Edit `src/` directly. `Cargo.toml` exposes four mutually exclusive effort modes:
`mode-low`, `mode-medium` (default), `mode-high`, and `mode-very-high`.

```sh
cargo test --manifest-path core/Cargo.toml
python3 scripts/build.py low
python3 scripts/build_native.py
```

The public Rust package compiles private mode instances from this same source
through `scripts/prepare_rust.py`. Those generated modules are build output;
there is only one maintained algorithm tree. For how the stages fit into the
public scanner, see [the architecture overview](../docs/ARCHITECTURE.md).

## Stage boundaries

- Aztec has one reader, `multiformat/src/aztec_detect.rs` (bullseye localization,
  bounded source-gray refinement and sampling, Rune confirmation and the enlarged
  retry) with the decoder `multiformat/src/aztec.rs`. The facade's
  `matrix_frontend` runs it under `Policy::for_mode` (Low has smaller budgets and
  no bowed or adaptive sampling), sharing the original threshold images and row
  runs with Data Matrix and PDF417; QR and MaxiCode are separate reader groups.
  The multiformat crate's own scan uses the same per-mode policies.

- `qr_frontend` runs the QR-only reader: the multiformat QR detector
  (`multiformat/src/qr_detect.rs`) under this mode's `Policy`, then bounded
  block-threshold, foreground-threshold and sharpened retries. The multi-format
  reader groups run the same detector under the same `Policy::for_mode`; there is one
  detector and one threshold cache. `qr_grid` resamples unresolved regions from original
  grayscale pixels. The Rust facade selects QR-only reader groups and enforces
  the mode-specific frame retry limits. EAN add-on selections run the same 2D reader
  groups and recoveries as selections without add-ons.

- `stripes::Detector` localizes candidates: `stripes/raster.rs` (sampling, contrast
  conditioning, gradients), `groups.rs`, `proposals.rs` and `refinement.rs` (source-pixel
  refinement). A scanner session reuses one detector's storage across frames.
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

When changing one stage, preserve what the next relies on: source coordinates and
work-limited signals out of localization, sampling order and precision, acceptance
thresholds, independent support, retry path order and budgets, and the separation of
equal labels from conflicting values. Keep allocations reusable where the scanner already owns scratch. A new sampling
method should return observations through the existing acceptance stage. A new
retry strategy should produce `Segment` plans without changing acceptance or
frame reconciliation.

## Mode differences

Public Low uses the Turbo fast path in the Rust facade; the core Low policy
provides its recovery.

Low uses module-axis discovery, bounded retries and sparse stripe work. Medium
adds evidence-dependent retry caps and shared short-retail work. High adds
native-soft decoding and more retries, and runs Medium's retail profile and crop
recoveries on frames it leaves unread. Very High adds stronger identity/conflict
checks and additional source/grid refinement, including bounded EAN-13 recovery
from original pixel areas. The Turbo presets are separate build settings. Mode
differences use `mode-*` conditions; passive timing and tracing are optional
compile-time features, not configuration options.

## Numerical rules

Mode-specific arithmetic is intentional: replacing `hypot` with an algebraically
equivalent norm can change borderline source decisions. Recovery crops use exact
integer interpolation; preserve its byte-equivalence tests. Tuning constants and
their reasons are documented next to the constants in `src/` and
`bindings/rust/src/effort.rs`.

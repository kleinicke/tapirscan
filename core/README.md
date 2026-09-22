# Production scanner core

Edit `src/` directly. The build does not apply patches or select a historical
feature combination. `Cargo.toml` exposes four mutually exclusive effort modes:
`mode-low`, `mode-medium` (default), `mode-high`, and `mode-very-high`.

```sh
cargo test --manifest-path core/Cargo.toml
python3 scripts/build.py low
python3 scripts/build_native.py low medium high very-high
```

The public Rust package compiles private mode instances from this same source
through `scripts/prepare_rust.py`. Those generated modules are build output.
They are necessary to offer several compile-time policies in one Rust process;
there is only one maintained algorithm tree.

## Stage boundaries

- `experiment.rs` owns candidate evidence and reusable scratch buffers and runs
  the initial candidate pass. `experiment/sampling.rs` samples and normalizes
  profiles; `experiment/decoding.rs` invokes decoders and records accepted or
  conflicting observations; `experiment/association.rs` assembles observations
  with source-continuity proofs and shared budgets.
- `multi_scan/plan.rs` constructs retry paths. `multi_scan.rs` executes them in
  their defined order, enforces budgets and refreshes evidence-dependent effort.
  `verified_coverage.rs` proves which intervals can safely reuse prior work.
- `frame/identity.rs` owns physical overlap and pending-coverage geometry.
  `frame/conflict.rs` proves identity and resolves competing values from pixels.
  `frame.rs` reconciles candidates and assembles the final frame.
- `retail_pipeline.rs` returns typed retail detections. Its legacy JSON adapter
  is only for diagnostics and the historical region ABI.

Keep allocations reusable where the scanner already owns scratch. A new sampling
method should return observations through the existing acceptance stage. A new
retry strategy should produce `Segment` plans without changing acceptance or
frame reconciliation. No plugin interface is needed for either experiment.

## Mode differences

Low keeps its module-axis discovery shortcut, bounded retries and sparse stripe
work. Medium retains evidence-dependent retry caps and shared short-retail work.
High retains native-soft decoding and its existing retry policy. Very High also
retains stronger identity/conflict checks and additional source/grid refinement.
The image facade's fit limits remain 0/1/4/1, and source-detail recovery still
uses the Low core. Mode-specific arithmetic is intentional: replacing `hypot`
with an algebraically equivalent norm can change borderline source decisions.

Common historical features are now ordinary code. Genuine differences use
`mode-*` conditions; passive timing/tracing remain optional compile-time features.
The previous feature graph and rejected research alternatives remain reproducible
under `historical/core`, rather than complicating production configuration.

## History and provenance

`provenance/historical-core-20260922.json` pins the archived baseline.
`python3 scripts/build.py medium --historical --prepare-only` reconstructs its
exact selected sources in `build/history/medium`. Omit `--prepare-only` to run
the original selected-mode tests and verified legacy WASM build; use `--resume`
to complete an already prepared historical build.

Development builds verify frozen imported/history files and record the current
production inputs in the generated package. `scripts/verify_import.py` without
arguments also enforces the selected release source snapshot. After validating
an intentional production change, record a new runtime provenance revision and
new WASM identities; never rewrite an old historical hash to hide a change.

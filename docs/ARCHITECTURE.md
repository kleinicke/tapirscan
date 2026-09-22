# How Tapirscan works

Tapirscan's main EAN-13 path combines oriented localization with a supplied-region
scanner. It uses classical image processing and decoding; neither neural weights
nor another barcode library are required at runtime.

## From pixels to results

1. **Find candidate regions.** Stripe structure proposes barcode-like areas and
   orientations. Selected proposals receive geometric refinement.
2. **Sample original pixels.** Oriented profiles follow each candidate's geometry;
   a full-frame search is also retained.
3. **Attempt all primary candidates.** Cheap attempts precede bounded retries.
   Source evidence decides where the new modes permit additional retry work.
4. **Recover small details.** Medium, High and Very high inspect up to two source
   texture seeds, enlarge selected 256-pixel crops threefold, and scan them with
   a separately pinned Low decoder.
5. **Reconcile and return.** Decoded values have source-coordinate polygons and
   support scores. Optional evidence retains undecoded regions, work limits,
   primary candidates, and recovery frames with explicit crop transforms.

The effort modes change both compiled implementations and host budgets. They are
not merely aliases for one function with different timeouts. Higher effort does
not guarantee a strict superset of a lower mode's reads. Bounded recovery remains
marked unfinished, even when it successfully decodes a symbol.

## One scanner, native and WebAssembly

The public Rust `Scanner` in `bindings/rust/api` owns the complete pipeline.
Native Rust callers use it directly. C, C++, Python and Java reach it through
the C adapter. JavaScript loads one mode-specific WebAssembly module and calls
it through `bindings/wasm`; `rust-session.ts` manages its memory and lifetime.
Scanning is synchronous after JavaScript initialization.

Recovery, interpolation, reader ordering, budgets and duplicate reconciliation
are implemented once in Rust. The JavaScript layer validates inputs, transfers
pixels and exposes immutable results. Historical imported JavaScript remains
available for provenance but is not shipped as an active scanner.

Grayscale, RGB and RGBA inputs support explicit strides; alpha is ignored.
Positions refer to the pixels supplied by the caller, not an earlier image before
resizing. Crop-local candidate indices are never presented as primary indices.
See [the native contract](NATIVE_BINDINGS.md) and [Python input rules](API_DESIGN.md).

## Scanner pipeline boundaries

`bindings/rust/api` defines public types and entry points; `bindings/rust/src`
contains the private per-mode engine.
`pipeline.rs` orders image preparation, localization, primary scanning, recovery
and consolidation. Its stage helpers preserve proposal order, budgets and
source-coordinate bookkeeping so experiments can change one stage at a time.
`serialization.rs` assembles the optional diagnostic JSON after scanning.
`detail.rs` owns source-detail recovery; `formats.rs` coordinates additional
readers and supplement policies; `linear_duplicates.rs` reconciles physical reads.

Keep scanner decisions in the pipeline and result formatting in serialization.
A stage extraction still needs paired scans across all four modes, including
padded gray/RGB/RGBA inputs and compact versus detailed results. The experiment
workspace retains those cases and outcomes; the library retains API unit tests.

## Additional formats

`multiformat/` contains the supported EAN8/UPCE readers and experimental readers
for formats outside the retail group. Readers run only when selected; each
mode-specific WASM includes all readers. EAN-13 and UPC-A retain the selected
primary effort mode. Common1D uses effort 0/1/2/2 and QR Code uses 0/1/2/3
for Low/Medium/High/Very High; other matrix readers use effort 1.
When mixed with EAN13/UPCA, confirmed Common1D coverage can defer deep EAN
retries only for wholly contained proposals. Initial discovery and full-frame
search remain enabled. Source-detail recovery skips seeds inside that coverage.

These readers have different maturity and incomplete work-limit propagation in
some paths. Consult [format coverage](FORMATS.md); do not infer QR reliability
from an EAN-13 result.

## Repository map

| Directory                                 | Purpose                                              |
| ----------------------------------------- | ---------------------------------------------------- |
| `core/`                                   | Frozen base sources and exact experiment patches     |
| `provenance/`                             | Selected mode recipes and source/binary hashes       |
| `bindings/javascript/`                    | Browser/Node API and WASM session adapter            |
| `bindings/rust/`                          | Public Scanner API and shared private pipeline       |
| `bindings/wasm/`                          | Thin WebAssembly adapter over the Rust API           |
| `bindings/c/`, `cpp/`, `python/`, `java/` | Native language interfaces                           |
| `multiformat/`                            | Pinned EAN8/UPCE and experimental nonretail readers  |
| `demo/`                                   | Camera/photo app with independent comparison workers |
| `scripts/`                                | Reproduction, packaging and regression checks        |

Builds apply recipes in generated directories. Two documented native adaptations
expose an existing helper and rename the recovery package to prevent Cargo feature
unification. Neither rewrites the frozen algorithm inputs.
[The current promotion](PROMOTION_DETAIL_20260914.md) records exact selections and
integration evidence. Historical promotion records remain available for audit.

The demo's ZXing and ZBar workers are comparison tools. They never supply fallback
results to Tapirscan, and they are not dependencies of the distributed library.

## Developing algorithms and experiments

The frozen `core/` base and selected recipes remain the reproducibility boundary.
Run `python3 scripts/build.py MODE --prepare-only` in a fresh experiment worktree
to inspect the complete selected source under `build/MODE/temporarysource`.
That directory is generated: promote changes through source, recipe and provenance
review rather than relying on an edited build directory.

Use these stage boundaries when changing a scanner:

| Stage                           | Main implementation                                     | Preserve when testing another stage                                |
| ------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------ |
| Candidate discovery             | `core/src/stripes.rs`, `localize.rs`, `shear.rs`        | Source coordinates and omitted/work-limited signals                |
| Profile sampling and decoding   | `core/src/sampling.rs`, `experiment.rs`, format readers | Sampling order, numerical precision and acceptance thresholds      |
| Evidence and reconciliation     | `core/src/frame.rs`, `verified_coverage.rs`             | Independent support, separate equal labels and conflict handling   |
| Recovery scheduling             | `bindings/rust/src/detail.rs`                           | Effort policy, candidate namespaces and unfinished work            |
| Release duplicate consolidation | `bindings/rust/src/linear_duplicates.rs`                | Geometry, supplement identity, ranking and the shared pixel budget |

Rust release consolidation uses typed reads and geometry. Reader JSON is decoded
at its boundary and retained as an opaque payload, preserving additional metadata;
the primary EAN path does not serialize its results just to reconcile them.

An experiment regression suite belongs with its frozen manifest in the experiment
workspace. Reference shared images by path and hash, put decoded pixels and full
outputs in disposable storage, and retain compact comparison results. Exact output
parity establishes a behavior-preserving refactor on those cases; it does not
establish accuracy or latency improvements. Production API and ownership tests
remain alongside the library.

# How Tapirscan works

Tapirscan combines oriented localization with a supplied-region
scanner. It uses classical image processing and decoding; neither neural weights
nor another barcode library are required at runtime.

## From pixels to results

1. **Find candidate regions.** Stripe structure proposes barcode-like areas and
   orientations. Selected proposals receive geometric refinement.
2. **Sample original pixels.** Oriented profiles follow each candidate's geometry.
   Medium adds a full-frame search only when its candidates read nothing but show
   barcode evidence; other modes always search the full frame. Inspection reports
   a search window only when it was searched.
3. **Attempt all primary candidates.** Cheap attempts precede bounded retries.
   Source evidence decides where the higher modes permit additional retry work.
4. **Recover small details.** Medium, High and Very High inspect up to two source
   texture seeds, enlarge selected 256-pixel crops threefold, and scan them with
   the Low instance of the maintained core.
5. **Reconcile and return.** Decoded values have source-coordinate polygons and
   support scores. Optional evidence retains undecoded regions, work limits,
   primary candidates, and recovery frames with explicit crop transforms.

The effort modes change both compiled implementations and host budgets. They are
not merely aliases for one function with different timeouts.

## One scanner, native and WebAssembly

The public Rust `Scanner` in `bindings/rust/api` owns the complete pipeline.
Native Rust callers use it directly. C, C++, Python and Java reach it through
the C adapter. JavaScript loads one WebAssembly module per mode and calls
it through `bindings/wasm`; `rust-session.ts` manages its memory and lifetime.
Scanning is synchronous after JavaScript initialization.

Recovery, interpolation, reader ordering, budgets and duplicate reconciliation
are implemented once in Rust. The JavaScript layer validates inputs, transfers
pixels and exposes immutable results.

Grayscale, RGB and RGBA inputs support explicit strides; alpha is ignored.
Positions refer to the pixels supplied by the caller, not an image before
resizing. Crop-local candidate indices are never presented as primary indices.
See [the native contract](NATIVE_BINDINGS.md) and [Python input rules](../bindings/python/README.md#array-and-tensor-inputs).

## Scanner pipeline boundaries

`bindings/rust/api` defines public types and entry points; `bindings/rust/src`
contains the private per-mode engine.
`pipeline.rs` orders image preparation, localization, primary scanning, recovery
and consolidation. Its stage helpers preserve proposal order, budgets and
source-coordinate bookkeeping so one stage can change at a time.
`pipeline/restoration.rs` holds restored-contrast and threshold crop retries;
`pipeline/source_evidence.rs` checks recovered reads against original pixels.
`fast_linear.rs` is the bounded Low linear path; `fast_linear/recovery.rs` reuses
its source profiles for Medium, High and Very High recovery.
`result.rs` assembles optional diagnostic JSON after scanning.
`detail.rs` owns source-detail recovery; `formats.rs` coordinates additional
readers and supplement policies; `linear_duplicates.rs` reconciles physical reads.

`read.rs` carries typed detections and regions across all readers. Supplement
attachment, deduplication and ranking operate on those types; public results are
constructed directly. Reader-specific diagnostic fields live in `ReaderPayload`.
JSON is used only for requested diagnostics and output boundaries.

Keep scanner decisions in the pipeline and result formatting at the boundary.
A stage extraction needs paired scans across all four modes, including
padded gray/RGB/RGBA inputs and compact versus detailed results.

## Additional formats

`multiformat/` contains the EAN8/UPCE readers and additional supported linear and
2D readers. Readers run only when selected; every build includes all readers. EAN-13 and UPC-A retain the selected
primary effort mode. Common1D uses effort 0/1/2/2 and QR Code uses 0/1/2/3
for Low/Medium/High/Very High; other matrix readers use effort 1.
When mixed with EAN13/UPCA, confirmed Common1D coverage can defer deep EAN
retries only for wholly contained proposals. Initial discovery and full-frame
search remain enabled. Source-detail recovery skips seeds inside that coverage.

Reliability differs by format, so do not infer QR reliability from an EAN-13
result; see [format coverage](FORMATS.md).

## Repository map

| Directory                                 | Purpose                                                    |
| ----------------------------------------- | ---------------------------------------------------------- |
| `core/`                                   | Maintained production algorithms and explicit effort modes |
| `config/`                                 | Effort-mode settings and the format registry               |
| `bindings/javascript/`                    | Browser/Node API and WASM session adapter                  |
| `bindings/rust/`                          | Public Scanner API and shared private pipeline             |
| `bindings/wasm/`                          | Thin WebAssembly adapter over the Rust API                 |
| `bindings/c/`, `cpp/`, `python/`, `java/` | Native language interfaces                                 |
| `multiformat/`                            | EAN8/UPCE and additional linear/2D readers                 |
| `../tapirscan-web/demo/`                  | Camera/photo app with independent comparison workers       |
| `scripts/`                                | Reproduction, packaging and regression checks              |

Production builds compile `core/src` directly. The public Rust package relocates
this shared tree into private per-mode namespaces to support several modes in one
process. Generated namespaces are build artifacts, not separate maintained copies.

The demo's ZXing and ZBar workers are comparison tools. They never supply fallback
results to Tapirscan, and they are not dependencies of the distributed library.

## Changing the algorithms

Edit `core/src`. Plain Cargo selects Medium;
`python3 scripts/build.py MODE` runs a selected production core's tests.
The [core guide](../core/README.md) lists the stage files, mode features and what
each stage must preserve. Exact output parity establishes a behavior-preserving
refactor; it does not establish accuracy or latency improvements. Decoder changes
in `multiformat/` are tested as described in [its README](../multiformat/README.md);
the shared pipeline, WASM builds and commands are in [development](DEVELOPMENT.md).

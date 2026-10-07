# Release notes

## 1.3.0 — unreleased

**Breaking API changes in every binding.** `scan` returns a barcode list;
`inspect` returns the detailed report. Native consumers must rebuild for ABI 6.
See [migration](docs/API_MIGRATION.md) for replacements. Version 1.3.0 is an explicit early-library exception to the
[compatibility policy](CONTRIBUTING.md#api-stability); migrate before upgrading.

### Application API

- JavaScript adds `tapirscan/browser`: scan files, images, video frames, canvases
  and bitmaps in a bundled worker. Construction is synchronous, scans return
  promises, and server rendering is supported. Next.js and Vite production builds
  bundle the worker and WASM; Vite 6/7 development servers need the configuration
  in the [JavaScript guide](bindings/javascript/README.md).
- `scan` returns decoded instances with text, format, source-image geometry and
  payload metadata. `inspect` adds unread regions, work status, timing and
  diagnostics, replacing the debug option. `best(barcodes)` selects by reader
  support in JavaScript, Python, Rust, C++ and Java; support is not confidence.
- C, C++ and Java share the same scanner/options/result model as the other
  bindings. One native library contains all four effort modes. ABI 6 provides
  caller-owned error details, typed access and lazy JSON serialization. C scanner
  options have an explicit initializer; C++ rejects temporary image buffers.
- Java accepts native `MemorySegment` pixels without copying and compares
  payload bytes by content. Python wheels bundle the single native library.
  Rust barcodes implement serde `Serialize`. JavaScript and Python expose typed
  decoded format identifiers.
- JavaScript adds experimental Turbo presets `2`, `4`, `8` and `16` for 1D
  scanning, including Retail. Numbers are identifiers, not speed guarantees;
  presets do not provide corresponding 2D speedups. Their APIs may change in
  minor releases. See [Turbo usage](bindings/javascript/README.md#experimental-turbo-presets).

### Scanner behavior

- Segment-voting localization and source-bar duplicate consolidation cover all
  stable effort modes and linear formats, retaining distinct same-value labels.
- Retail recovery adds bounded contrast, motion-ghost and defocus retries,
  original-pixel evidence for small labels, and guarded reduced-resolution
  retries. High and Very High share the retail EAN-8/UPC-E path; Very High adds
  subpixel EAN-13 recovery where precise affine candidate geometry is available.
- QR and Aztec recovery gain bounded sampling, contrast and geometry retries.
  Localized linear and matrix recovery preserve source coordinates and work
  limits. Turbo recovery adds evidence confirmation and guarded empty-scan
  retries, including Turbo2 contrast/high-pass retries on its strongest retail box.
- Reads, geometry, ordering and latency can differ from 1.2.2. Higher effort does
  not guarantee more reads on every image. See the [core guide](core/README.md)
  for selected changes, retained measurements, limitations and known failures.

### Packaging and documentation

- Documentation-only provenance snapshots no longer invalidate WASM package
  verification. Compiled sources, selected engine identities and binary hashes
  remain checked. Python validation accepts empty search-window evidence when
  no full-frame search was performed.

- Implemented linear and 2D formats are documented as supported, with explicit
  [variant limitations](docs/FORMATS.md); Turbo presets remain experimental.
- Packages include the complete MIT and Apache 2.0 license texts, correcting
  the license-file mismatch in the published 1.2.2 artifacts.
- Research archives and obsolete prototypes move outside the release source
  tree. Production builds require no research checkout; retained Turbo variants
  and active recovery paths remain available.

## 1.2.2 — 2026-09-25

- Public `low` now uses the original Turbo policy across all bindings; higher
  effort modes remain available. Low prioritizes speed and can miss difficult
  symbols; Medium remains the default API mode. The previous Low is
  TS-Low Classic in the demo. Experimental Turbo tiers remain demo-only.
- Demo cards pause without disappearing; More scanners controls visibility.
  Basic ZXing and ZXing-JS readers are separate optional comparisons.

- Speed up Medium, High and Very High recovery crops with pixel-identical integer interpolation and cached rows; retain existing APIs and scan behavior.

- Recover fuller barcode display outlines with bounded profile agreement when strict bar tracing fails, including high-resolution photos.

- Measure high-resolution linear barcode extents with adaptive cross-section spacing and outer-edge priority under the existing sample budget.

- Measure full visible linear-bar extents and reconcile competing interpretations through connected source-bar footprints.

- Reconcile crossing linear reads through distributed source-bar ownership while preserving earlier duplicate proofs.
- Suppress weak ITF interpretations when supported retail reads demonstrably occupy the same bars.

- Follow changing bar widths when consolidating folded linear observations.
- Reuse well-resolved EAN8/UPC-E coverage before enlarged detail rescans, while
  retaining recovery for small or weakly supported symbols.

- Recover additional strongly sheared retail labels with bounded sparse envelope fits.
- Consolidate more warped linear observations using continuous ink and paper evidence,
  while preserving separate equal-value labels and existing work limits.
- Specialize exact RGB/RGBA luminance conversion and borrow packed grayscale inputs
  for additional format readers.

- Preserve unchanged generated package and adapter inputs during refresh; serialize
  supported package builds and provide experimental WASM assets outside release identities.
- Clarify candidate scanning, mode budgets and frame reconciliation; reuse retail
  conversion storage and remove fixed sampling-list heap allocations.
- Support Chromium comparisons and retained public outputs for labeled experiments.

- Make release WASM identities independent of checkout paths.
- Maintain production algorithms directly in `core/src` with explicit effort-mode
  features; originally preserve recipes under `historical/` (subsequently moved
  to the experiment archive).
- Separate profile sampling, decoding, observation association, retry planning and
  physical identity/conflict resolution into concrete stages.
- Assemble native results on typed data, including supplements, ordering and unread
  regions. JSON serialization is limited to outputs and optional diagnostics.

## 1.2.1 — 2026-09-19

- Enable retail formats (EAN13, UPCA, EAN8 and UPCE) by default across all public
  bindings. Explicit EAN13 selection retains its existing reader path.
- Keep Medium effort and ignored supplements as defaults; native ABI 4 is unchanged.

## 1.2.0 — 2026-09-17

**Breaking changes:** this early-library minor release intentionally changes the
application APIs. Existing callers should follow the migration guide below.

- Mark all retail formats (EAN13, UPCA, EAN8 and UPCE) as supported. Formats
  outside the retail group remain experimental.
- Unify JavaScript, Python and standalone Rust around scan results with decoded
  instances, undecoded proposal geometry and truthful work-limit status.
- Replace finish-candidates booleans with a format-independent extended-budget flag
  while retaining the concise `best` selection helper. Align Rust metadata absence and bounds.
- Medium shares primary EAN/UPC profile evidence when EAN8 or UPCE is enabled,
  with bounded short-code recovery. EAN8-only uses the same recovery as retail
  selection. EAN13-only and opt-in supplement policies retain their reader paths.
- See [migration](docs/API_MIGRATION.md) for intentional breaking changes. Native
  ABI 4 is unchanged; shared retail recipes have new immutable tags.
- Rust supports `scan(image)` with defaults and `scan_with_options(image, options)`
  for explicit settings. `best` remains the selection helper across bindings.

## 1.1.0 — 2026-09-17

- Standalone Rust crate with one-shot scanning, reusable scanners, all four runtime
  modes, borrowed raw/image buffers and owned typed results. Packaging reproduces
  pinned engines with no local path dependencies; default EAN-13 results avoid a
  JSON round trip. Cross-language parity covers metadata and undecoded evidence.

- Optional per-scan `finish_candidates` / `finishCandidates` lets selected
  EAN13/UPC-A candidates continue beyond shared frame budgets. Default scanning
  stays bounded; per-candidate effort and other limits still apply.
- Retail decoding shares grayscale/template work and coverage calculations.
  Source-pixel evidence consolidates duplicate linear observations while preserving
  separate products and different supplements.
- The demo supports candidate continuation alongside camera, comparison and
  ground-truth label controls.

- One-shot `scan()` and reusable scanners return all decoded instances with
  immutable results, source-image geometry, support, timing and work-limit status.
- Python accepts Pillow images, NumPy arrays, PyTorch tensors and explicit pixel
  buffers. Float ranges and layouts are explicit; optional BGR input supports OpenCV.
- Format identifiers and presets select EAN/UPC, linear and matrix readers.
  Additional readers are experimental; effort modes control EAN13/UPCA, Common1D
  and QR Code search.
- QR High adds threshold/sharpen recovery; Very High adds bounded curved-grid
  recovery. Common1D improves low-contrast and scanline decoding; strong mixed-format
  reads can avoid redundant deep EAN retries. QR-only packed RGBA uses direct WASM upload.
- Strict Clippy checks cover the base core, selected mode recipes, additional readers
  and native bindings in routine validation.
- Optional `Ignore`, `Read` and `Require` policies control EAN/UPC supplements.
  Supplement text is separate; polygons describe the main barcode.
- Results expose available payload bytes, GS1 and structured-append metadata.
  `.best` selects the highest numeric support, not cross-format confidence.
- Python `as_dict()` provides JSON-compatible public results. Optional diagnostics
  expose typed undecoded regions and reader-specific search evidence.
- JavaScript supports configurable WASM loading, immutable scanner configuration,
  synchronous reusable scans, and browser worker/camera examples.
- Native consumers use ABI 4, with version checks and descriptive errors.
- CI validates supplement association and geometry with a pinned test encoder,
  alongside cross-language and installation checks.

See the [Python](bindings/python/README.md), [JavaScript](bindings/javascript/README.md)
and [native binding](docs/NATIVE_BINDINGS.md) guides for the current API. Publication
steps are in [the release checklist](docs/RELEASING.md).

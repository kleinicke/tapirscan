# Release notes

## 1.3.0

**Breaking API changes in every binding.** `scan` returns decoded values and
their locations; `inspect` returns the detailed report. Native consumers must
rebuild for ABI 6. See the [migration guide](docs/API_MIGRATION.md).

### Application API

- `scan` returns `values`, `barcodes` (text, format, polygon and rectangle) and
  `best`. `inspect` adds timing, unread regions and diagnostics, replacing the
  `debug` option. The `unfinished` result flag is removed; it was set on nearly
  every scan and gave no useful signal.
- The extended-budget scan option is removed: it did not change results in
  Medium, High or Very High.
- Supplement policies are lowercase like the modes: `"ignore"`, `"read"` and
  `"require"`.
- Per-call formats override the scanner's formats in every binding. Rectangles
  are integers in every binding. Python and JavaScript errors share the same
  `code` names.
- The free `best()` function and direct iteration, indexing and `len` on Python
  and Rust results are removed; use `result.best` and `result.barcodes`.
- JavaScript adds `tapirscan/browser`: scan files, images, video frames, canvases
  and bitmaps in a bundled worker. Construction is synchronous, scans return
  promises, and server rendering is supported. Next.js and Vite production builds
  bundle the worker and WASM; Vite 6/7 development servers need one
  [configuration line](bindings/javascript/README.md#browser-apps-react-and-svelte).
- C, C++ and Java share the scanner, options and result model of the other
  bindings. One native library contains all four effort modes. ABI 6 provides
  caller-owned error details, typed access and on-demand JSON. C scanner options
  have an explicit initializer; C++ rejects temporary image buffers.
- Rust mode features only add modes: all four are default features, and
  smaller builds disable the defaults and list their modes.
- Data Matrix, PDF417, Aztec and MaxiCode report `reader_initialization` as
  false when the symbol has no reader-initialization flag; other formats leave it
  unavailable.
- Java accepts `MemorySegment` pixels without copying. Python wheels bundle the
  native library. Rust barcodes implement serde `Serialize`. JavaScript and
  Python expose typed format identifiers.
- JavaScript adds experimental Turbo presets `2`, `4`, `8` and `16` for faster
  1D scanning, including Retail, in both the core and the browser entry. They may change in minor releases; see
  [Turbo presets](bindings/javascript/README.md#experimental-turbo-presets).

### Scanner behavior

- More linear barcodes are found and boxed in all effort modes, with fewer
  duplicate reads of the same label; separate labels with equal values stay
  separate.
- Retail labels read better under low contrast, camera shake, defocus and small
  print. High and Very High read more EAN-8 and UPC-E labels; Very High adds
  subpixel recovery for small EAN-13 symbols.
- QR Code and Aztec recover more difficult symbols.
- Reads, geometry, ordering and latency can differ from 1.2.2. Higher effort
  does not guarantee more reads on every image.

### Known issues

- A linear barcode decoded along a slightly diagonal scanline can get an outline
  tilted by a few degrees, extending a few pixels past the bars at two corners.
  Decoded values are unaffected.

### Packaging

- All implemented linear and 2D formats are supported, with documented
  [variant limitations](docs/FORMATS.md).
- Packages include the complete MIT and Apache 2.0 license texts, correcting the
  license files in the 1.2.2 artifacts.

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

# Release notes

## Unreleased

- Rust: `Image::try_from(&dynamic_image)` borrows an 8-bit gray, RGB or RGBA
  `image::DynamicImage`, such as a decoded JPEG, without copying pixels.
- Rust: `Format` implements `Display` with the format names used by every
  binding (`EAN13`, `QRCode`), so `println!("{}", barcode.format)` matches them.
- Images up to 256 megapixels and 65,535 pixels per side are accepted (was 32
  megapixels). Images above 32 megapixels are scanned as an area-averaged
  grayscale copy, with results in the supplied image's coordinates.
- Fixed an integer overflow in the QR retry for images wider than about 4.5
  million pixels in WebAssembly.
- Native builds (C, C++, Java, Python, Rust) run the enabled 2D readers of one scan
  on separate threads, with identical results: with all formats, Low scans are
  1.6-1.75× and Medium scans 1.25-1.35× faster (AMD Ryzen 7 5800X, Apple M1 Pro). Set `TAPIRSCAN_THREADS=1` to
  keep each scan on one thread, for example on servers running one scanner per core.
  WebAssembly scans stay single-threaded.
- Data Matrix candidates come from a streaming component labeller that keeps only
  eligible components, so large or textured images no longer fall back to a slow
  flood fill. Identical results; Medium with all formats is 23% faster on a
  12-megapixel image.
- Swift (iOS 15+, macOS 12+): a Swift package over an XCFramework built by
  `scripts/build_apple.py`, with `BarcodeScanner` for pixel buffers, `CGImage`s and
  camera `CVPixelBuffer`s (bi-planar YUV frames are scanned in place). Slices:
  macOS arm64 and x86_64, iOS arm64, and the iOS simulator on arm64 and x86_64.
- Android (API 24+, arm64-v8a and x86_64): an AAR with a JNI bridge built by
  `scripts/build_android.py`, with `BarcodeScanner` for direct buffers such as
  CameraX planes and byte arrays. The AAR ships its R8 keep rules, and the x86_64
  library uses the ABI's SSE4.2 and POPCNT baseline.
- Python: `color_order="BGR"` reorders channels with one strided copy per channel
  instead of fancy indexing; a 12-megapixel OpenCV frame scans in 32 ms instead of
  69 ms on an M1 (converting to RGBA with `cv2.cvtColor` remains fastest).
- Python: `tapirscan.scan_many(images, ...)` scans a batch on worker threads, one
  scanner per thread, and returns results in input order.
- Docs: a [fast end-to-end scanning](docs/PERFORMANCE.md) guide with measured costs
  and the fastest input path per environment; every binding guide now has a short
  fast-usage section, and the C example reuses its scanner.
- Faster Low and Medium scans with several formats, with identical results:
  Low mode's QR alignment search uses the precomputed sampling tables Medium
  already had, PDF417 localization precomputes its rotated-row products, and Data
  Matrix run labeling searches whole row slices. With all formats on the 5,315
  960×960 variation images, median scan time drops 7.5% in Low, 5% in Medium and
  2-4% in High and Very High on x86-64; Low is 9% faster on an Apple M1. The
  1D localizer's downsampling is specialized per channel count, making Low
  retail scans, as used for camera frames, 6-7% faster (M1 5.1 → 4.8 ms, x86-64
  5.5 → 5.1 ms per 960×960 frame).
- Browser: `ImageData` and canvas pixels are copied as bytes instead of converted
  element by element (12-megapixel `ImageData`: 27 ms instead of 42 ms on an M1).
- x86-64 native libraries (C, C++, Java, Python) target x86-64-v2 (SSE4.2,
  POPCNT): scans are 20-30% faster with identical results. They no longer load on
  x86 CPUs older than Intel Nehalem (2008) or AMD Bulldozer (2011).
- Python: non-uint8 inputs convert much faster with identical bytes. CUDA tensors
  are validated and rounded on the GPU and transferred once as 8-bit pixels; CPU
  tensors and NumPy arrays are validated and rounded in one blocked pass, on up to
  eight threads for NumPy arrays of 4 Mi samples or more. Immutable `bytes`
  (Pillow conversions, raw `PixelImage` bytes) are scanned in place. On
  x86-64, a 12-megapixel float32 RGB scan (Medium, retail) takes 36 ms instead of
  840 ms from CUDA, 51 instead of 754 ms from a CPU tensor and 54 instead of
  161 ms from NumPy.
- Python, Java: a scanner copies caller pixels into one native buffer it reuses
  until `close()`, instead of fresh memory per scan. This saves about 20 ms per
  12-megapixel RGBA scan. Python Pillow images already in L or RGB mode skip a
  redundant conversion copy.
- Browser: Blob and canvas inputs up to 16 megapixels are decoded on one canvas
  reused by the worker; larger ones are decoded in bands, with identical results,
  so no full-size canvas or pixel copy is made. `scan(pixels, { transfer: true })`
  hands a whole pixel buffer to the worker instead of copying it (12 MP RGBA on
  x86-64: 23 ms instead of 52 ms); the caller's buffer is detached.
- WebAssembly: images above 32 megapixels are copied in a few rows at a time and
  reduced as they arrive, with identical results. A 48-megapixel RGBA photo leaves
  211 MB of scanner memory instead of 362 MB and scans 24% faster. Scanners
  created from already loaded WASM reuse its compiled module (0.1 ms instead of
  2.6 ms per scanner).
- Docs: the JavaScript guide decodes Node images with sharp, and the Python and
  Rust examples print the format and rectangle instead of the polygon.

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

- Linear barcodes are localized by bar-segment voting, and areas grown along the
  bars merge duplicate reads of one label; separate labels with equal values stay
  separate.
- New recovery passes target low contrast, camera shake, defocus and small print
  on retail labels. High and Very High read EAN-8 and UPC-E in the retail scan;
  Very High adds subpixel recovery for small EAN-13 symbols.
- QR Code and Aztec add recovery passes for difficult symbols.
- On the maintainer's development image sets these changes found more barcodes
  with fewer duplicates; they are not a published benchmark.
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

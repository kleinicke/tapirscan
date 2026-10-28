# Release notes

## Unreleased

- Large QR codes (version 7 and up) on paper that is not quite flat: a symbol
  whose header reads but whose grid does not decode is sampled again cell by
  cell between its alignment patterns, with the version taken from its version
  information. On BoofCV's high-version QR images Medium reads 28 of 37 codes
  instead of 8, High 34 instead of 10 and Low 24 instead of 6.
- QR codes whose finder patterns are found but whose modules the usual
  thresholds split, mostly synthetic codes with cropped quiet zones: a frame
  that read nothing but holds finder patterns is tried with one more global
  threshold, in High and Very High and in Medium's retry of unread QR frames. High reads 19 more codes of a 4,619-image QR set
  and Medium 3.
- More QR codes beside a read symbol: the windows around unread finder pairs
  reach up to 1,920 pixels, scanned reduced to at most 640 pixels, and may then
  cover most of the frame. High and Very High search up to 12 such windows and
  keep windows whose read symbol lies outside their core.
- Large QR codes whose grid is found but whose data does not decode get a
  second try from Medium up: one per frame in a larger crop, and one too large
  for any crop on a copy reduced to at most 640 pixels. With the windows above,
  Medium reads 9 more of the 1,258 codes in BoofCV's QR images and High 8 more.
- MaxiCode and Micro QR symbols that are found but not read get a crop retry,
  as QR, Data Matrix and Aztec symbols already did (MaxiCode: High and Very
  High; Micro QR: Medium and above).
- Windows on ARM64: a `win_arm64` Python wheel and a `windows-arm64` native
  library on each GitHub release, which the Dart build hook downloads.
- Faster scans of selections with several 2D formats: PDF417 localization skips
  angles without guard evidence, and the finer reduced QR copy runs only where a
  QR candidate remains. Reads are unchanged; with every format selected, Medium
  takes about 9% less time, with the 2D formats alone about 15% less.
- Sheets of many small linear symbols: when a frame above 1536 pixels already
  read at least six distinct values, Medium, High and Very High scan it again in
  overlapping tiles with the selected non-retail linear formats. On crowded
  label-sheet photos Medium reads about a quarter more symbols and takes about
  half as long again there; other frames are unchanged.
- Low reads more: Data Matrix and QR retry evidence they would have dropped,
  and blurred, oversampled or faint EAN/UPC rows are decoded again (about 7%
  more EAN-8 on development images). Medium also retries Micro QR evidence,
  loses fewer linear reads when more formats are selected, and reads more of the
  UPC-E symbols Low reads.
- Faster Low and Medium: QR and Data Matrix stop searching further thresholds
  once reads explain the frame (QR-only scans of QR images about 20% faster),
  Medium spends less retry effort on large retail photos (about 9% less) and
  skips its EAN-13/UPC-A recoveries when neither is selected. Overall, Medium
  takes about 2% less time on retail and 4% less on common and all-format
  selections, and Low about 4% less on common and all formats; Low retail scans
  take as long as before.
- Browser scanners recover instead of failing for good: a WASM engine failure
  (out of memory, stack overflow) fails that one scan and the next scan starts
  a fresh engine, also in the core `Scanner`; a worker that reports an error
  after loading is replaced on the next scan. `CameraScanner` skips a failed frame and stops
  only when the scanner is disposed or after three failures in a row.
- Faster first frames and video hand-over in the browser: an idle scanner warms
  up once loaded in Low and Medium, so its first camera frame takes 30% (WebKit)
  to 50% (Chrome) less time; `<video>` and `VideoFrame` inputs reach the worker
  as `VideoFrame`s, 2-4 ms faster per 720p frame. Results are unchanged.
- `CameraScanner` pauses scanning while the video is paused in Safari too, and a
  custom worker file that imports the worker module with a top-level `await`
  no longer leaves the scanner waiting forever.

## 1.5.0

Native consumers need no rebuild: ABI 7 is unchanged. `1D` and `all` also read
GS1 DataBar Limited, and `2D` and `all` also read Micro QR. Swift calls in `async` code need
`await`, Rust code that builds or fully destructures result structs needs a
small change, Java record patterns of `Barcode` need the new component, and
exhaustive switches over formats and content kinds need the new members. Apps
that switch on `content.kind` can see new kinds. Reads,
geometry and timing can differ from 1.4.0. See the
[migration notes](docs/API_MIGRATION.md#upgrading-from-140).

### New

- Dart and Flutter package (preview) on pub.dev, over the C library through
  `dart:ffi`. Its build hook downloads a prebuilt library for 64-bit Android,
  iOS, macOS, Linux and Windows, or builds from source in a checkout; it is tested on macOS arm64, Linux x86_64 and
  an Android arm64 emulator. `scan` runs on a background isolate. See its
  [README](bindings/flutter/README.md).
- Prebuilt native libraries on each GitHub release: a zip per platform with the
  library, the C and C++ headers and a CMake package, the Android AAR and the
  Swift XCFramework, so C, C++, Java, Swift and Android apps need no Rust.
- GS1 DataBar Limited (`DataBarLimited`, in `1D` and `all`) and Micro QR M1–M4
  (`MicroQRCode`, in `2D` and `all`). Selecting `QRCode` alone reads no Micro
  QR.
- Opt-in formats, read only when selected by name: Extended (full-ASCII) Code 39
  (`Code39Extended`), IATA 2 of 5 (`IATA25`), Matrix 2 of 5 (`Matrix25`),
  Industrial (Standard) 2 of 5 (`Industrial25`) and Laetus Pharmacode
  (`Pharmacode`). No preset, `all` included, selects them.
- Codabar barcodes report their start and stop characters, such as `"AB"`, in the
  new optional result field `codabarStartStop` (`codabar_start_stop` in Python,
  Rust, C and C++); `text` still omits them. C reads it with the new
  `TAPIRSCAN_FIELD_CODABAR_START_STOP`; header, bindings and library must come
  from the same release.
- Structured content: new kind `ISBN` for 978 and 979 EAN-13 digits (not
  979-0 ISMN) and written forms such as `ISBN 978-…`, `ISBN-10: …` and
  `urn:isbn:`, with the fields `isbn13` and `isbn10`. More payloads have
  content: `www.` addresses and
  `URLTO:`/`MEBKM:` bookmarks (`URL`, with a `title`), bare e-mail addresses and
  `SMTP:` (`Email`), `mms:` links (`SMS`, which now keeps a `subject`) and
  `BIZCARD:` contacts. Leading whitespace no longer hides a prefix. `http(s)`
  URLs need a host and no whitespace, `SMTP:` needs a valid address, an empty
  `TEL:` has no content, and an ISBN label that names the length needs that
  many digits.
- Swift: `scan` and `inspect` for pixel arrays and `CGImage` have `async`
  overloads that run on a detached task. In async code Swift selects them, so
  existing calls there need `await`.
- Rust: the result structs `Barcode`, `ScanResult`, `InspectionResult`,
  `StructuredAppend`, `UndecodedRegion`, `Diagnostics`, `Content` and `Field`
  are `#[non_exhaustive]`, so later fields are not breaking changes. Get them by
  scanning, deserializing or `parse_content`.
- JavaScript: `tapirscan/browser/low`, `/medium`, `/high` and `/very-high` emit
  one mode's WASM into application builds instead of all eight files.
- JavaScript: `tapirscan/support` exports `browserSupported()` and
  `wasmSupported()`, which report whether a runtime can scan; the module imports
  nothing else. Scanners created where WebAssembly SIMD (or a browser's module
  workers, `OffscreenCanvas` or `createImageBitmap`) is missing download nothing and reject with the
  `ScannerError` code `"unsupported"`.
- JavaScript: the browser `worker` option starts the scanner's worker from
  application code, with the new `tapirscan/browser/<mode>/worker` modules, so
  Angular production builds can bundle it.
- JavaScript: `tapirscan/camera` (preview) adds `CameraScanner`, an optional
  camera loop for any browser scanner, with `scanInterval`, a scan `region`, the
  torch and camera choice. `onDetect` fires once per appearance of a barcode,
  after `confirmations` consecutive reads. Pausing the `<video>` pauses
  scanning. Without `onError`, failures go to `reportError`. `start()` waits for
  a camera request of an earlier call that is still pending.

### Scanner behavior

Read comparisons below describe the development evaluation sets. They do not
guarantee that every symbol of these kinds is read on an arbitrary image. See the
[evaluation protocol](docs/BENCHMARKS.md).

- EAN-2 and EAN-5 supplements (`eanAddOnPolicy` `read` and `require`): other
  formats read exactly as with `ignore` (before, `read` lost Code 128, Code 93,
  Code 39, Codabar and ITF reads and took longer). Supplements are read next to
  more blurred, low-resolution and perspective-distorted EAN/UPC symbols, and
  reported only when two independent reads agree.
- Read small, densely packed QR codes, such as label sheets with dozens of
  symbols, in every mode; High and Very High read more QR codes in shaded or
  blurred photos.
- Medium, High and Very High read a second QR code beside one already read
  whose small finder pattern the first pass missed, as in 4K video frames.
- Read small QR codes over a wider range of sizes and rotations, also in frames
  resampled to another aspect (a 4:3 camera image stretched to 16:9) or seen at
  an angle.
- Read sheets with more than two dozen Data Matrix, Aztec or linear symbols
  (EAN/UPC, Code 128, Code 39, Code 93, ITF, Codabar, DataBar) per frame, in
  every mode. Frames without such sheets scan as fast as before.
- Read single small Aztec symbols at more rotations, and large full-range Aztec
  symbols that are slightly turned or warped. High and Very High read Data
  Matrix symbols squashed to 0.6 of their width, and more EAN-13 symbols on
  label sheets.
- Fewer wrong Code 39 characters in blurred symbols at about one pixel per
  module.
- ITF reads need one narrow/wide model across the symbol, and yield to Code 39
  where the same bars form better-fitting Code 39 characters, so Code 39, Code 32
  and PZN symbols cut by a crop or the image edge are no longer read as ITF.
- Read Code 39 and Codabar symbols with one or two data characters (they need
  full ten-module quiet zones), Code 93 and Code 128 symbols with ink spread,
  and Code 93 with three-module quiet zones or perspective-scaled stop
  characters.

### Fixed

- JavaScript: creating a browser `Scanner` without `wasmBaseUrl` no longer
  throws in Deno without `--location`.

### Performance

- Low and Medium scan as fast as 1.4.0, and frames with QR codes up to about 4%
  slower. High and Very High take about 6–11% longer when QR Code or Data Matrix
  is selected and are unchanged with only linear formats. These are WebAssembly timings in
  Chrome on the development evaluation sets; other images, devices and format
  selections can differ.
- Micro QR in `2D` and `all` makes Low and Medium about 3–5% slower on QR
  images with those selections.

### Packaging

- Python: wheel builds, including from a `TAPIRSCAN_NATIVE_DIR`, need the
  `native-build.json` that `scripts/build_native.py` writes beside the library,
  and fail if the library was built from other sources.
- Concurrent native and WASM builds of one checkout wait for each other.

### Docs

- Browser deployment, camera recipes, and migration guides from ZXing, ZBar,
  Quagga, html5-qrcode and flutter_zxing.

## 1.4.0

Native consumers must rebuild for ABI 7; existing C, C++, Java, Python and
JavaScript calls keep working. Rust code that builds `ScannerOptions` with every
field or matches `Format` exhaustively needs a small change. Reads,
geometry and timing can differ from 1.3.0. See the
[migration notes](docs/API_MIGRATION.md#upgrading-from-130).

### New

- Swift package (iOS 15+, macOS 12+) and Android AAR (API 24+, arm64-v8a and
  x86_64), built from source with `scripts/build_apple.py` and
  `scripts/build_android.py`. Both scan camera frames in place: bi-planar YUV
  `CVPixelBuffer`s on Apple platforms, CameraX Y planes on Android.
- Structured content in every binding: GS1 element strings and Digital Link,
  URLs, email, phone, SMS, geo, Wi-Fi, contacts, calendar events, Swiss
  QR-bills, EPC payments, HIBC, VINs and AAMVA driver's licenses. Every binding
  except JavaScript adds `parse_content`; JavaScript, Python, Rust and Swift
  barcodes also carry `content`. Every JSON form of a barcode (Rust serde, the C scan JSON, Python
  `as_dict()`, JavaScript) includes `content`. Swiss
  QR-bill, EPC, HIBC, VIN and AAMVA parsing is experimental. See
  [structured content](docs/CONTENT.md).
- Opt-in formats `Code32` and `PZN` (pharmacy numbers in Code 39) and
  `MSIPlessey` (with a mod-10 check digit). No preset, `all` included, selects
  them; C adds `TAPIRSCAN_FORMATS_OPT_IN`. See
  [format coverage](docs/FORMATS.md#pharmacy-numbers-in-code-39-code32-pzn).
- PDF417 reads Macro PDF417 control blocks (reported as structured append),
  ECI 925 and 926.
- Images up to 256 megapixels and 65,535 pixels per side (was 32 megapixels).
  Images above 32 megapixels are scanned as an area-averaged grayscale copy, with
  results in the supplied image's coordinates.
- Python: `tapirscan.scan_many(images, ...)` scans a batch on worker threads and
  returns results in input order.
- Browser: `scan(pixels, { transfer: true })` hands the pixel buffer to the worker
  instead of copying it; the buffer is detached.
- Rust: `Format` and `ContentKind` are `#[non_exhaustive]`, and `Format::ALL` is
  a slice, so later formats and kinds are not breaking changes.
- Rust: `Image::try_from(&dynamic_image)` borrows an 8-bit `image::DynamicImage`
  without copying, and `Format` implements `Display` with the names used by every
  binding (`EAN13`, `QRCode`).

### Scanner behavior

Read comparisons below describe the development evaluation sets. They do not
guarantee that a higher effort mode reads every symbol a lower mode can read on
an arbitrary image. See the [evaluation protocol](docs/BENCHMARKS.md).

- **Linear formats** read nearly the same whatever else is selected, follow rotated
  and warped bars from Medium up, and find small Code 128 fields on large label
  photos and sheets far more often. ITF tolerates ink spread. Codabar and
  Code 93 selections without retail formats read at least what Low reads from
  Medium up. High and Very High read at least what Medium reads on Code 128,
  Code 39 and ITF, including small fields on large label sheets. From Medium up,
  boxes where a symbol was seen but not read get the extra work Low spends there,
  and selections with retail and other linear formats read rotated UPC-E that
  only Low read before.
- **Retail:** High and Very High read at least what Medium reads on retail
  photos. Medium reads more labels on large photos with many EAN-13 or UPC-A
  symbols. Low no longer reports a UPC-E or EAN-8 decoded from part of an EAN-13
  or UPC-A it also reads.
- **QR Code** reads more distorted, blurred, shadowed, tiny and large-frame
  symbols. In mixed selections QR runs as its own reader group and reads
  almost like a QR-only scan. Once Low has read two QR symbols, it searches as
  long as Medium for more. Reading EAN add-ons does not change 2D reads; it
  only adds supplement decoding time.
- **Data Matrix** reads more phone photos, industrial marks and foreshortened,
  tiny, blurred and large close-up symbols.
- **PDF417** reads more perspective, blurred and border-clipped symbols. When two
  different payloads decode at the same place, the one needing more corrections
  is dropped.
- **Aztec** reads small symbols from Medium up.
- **MaxiCode:** High and Very High retry large unread frames on reduced copies,
  as Medium does.
- **Textures:** the tested striped, checkered, ringed and noisy backgrounds no
  longer produce false linear reads, and the QR, Aztec and MaxiCode finders bound their
  work on such textures instead of taking seconds per frame. A frame whose finder
  budget binds is reported as unfinished.

### Performance

- Native builds (C, C++, Java, Python, Rust, Swift, Android) run a scan's 1D
  formats and enabled 2D readers on separate threads. Threaded and sequential
  results matched in the development comparisons. See the
  [input and threading guidance](docs/PERFORMANCE.md).
  The scanner option `parallel` turns this off per scanner, and
  `TAPIRSCAN_THREADS=1` for the process. In C, the new
  `tapirscan_scanner_options.parallel` field is zero
  (`TAPIRSCAN_PARALLEL_DEFAULT`) when an initializer leaves it out. Rust
  `ScannerOptions` literals need `..ScannerOptions::default()`. Python's `scan_many` defaults to
  `parallel=False`, since its worker threads already keep the cores busy.
- WebAssembly stays single-threaded.
- x86-64 native libraries target x86-64-v2 (SSE4.2, POPCNT).
  CPUs without x86-64-v2 support can now fail with an
  illegal instruction; see [migration notes](docs/API_MIGRATION.md#upgrading-from-130).
- Faster QR, Aztec, Data Matrix and PDF417 search and retail localization, with
  unchanged results in the paired development comparisons for those optimizations.
- High and Very High run more recoveries and can take noticeably longer on frames
  they leave unread, especially with `all` formats.
- When QR shares the scan with other formats, High and Very High run the
  foreground-threshold and equalized/inverted QR recoveries on frames above
  262,144 pixels only where a QR candidate stayed unread. This saves recovery
  work but can miss symbols that leave no candidate; see the
  [development comparison](https://github.com/kleinicke/tapirscan/commit/a20fae3).
- QR-only scans in High and Very High were slower than in 1.3.0 on the
  [tested retail photos without QR codes](https://github.com/kleinicke/tapirscan/commit/107c3b8).
- Cheaper inputs: Python converts BGR, float and tensor inputs faster (CUDA
  tensors on the GPU) and scans immutable `bytes` in place; Python and Java reuse
  one pixel buffer per scanner; browsers copy `ImageData` as bytes and decode
  large `Blob`s in bands; WebAssembly streams images above 32 megapixels into a
  reduced copy, and scanners created from loaded WASM reuse its compiled module.

### Fixed

- An integer overflow in the WebAssembly QR retry on very large images.

### Docs

- [Fast end-to-end scanning](docs/PERFORMANCE.md): input paths, copy costs and
  threading guidance, linked from every binding guide.

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

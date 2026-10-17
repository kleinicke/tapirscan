# Release notes

## Unreleased

- Very High reads at least what High reads on large frames: it localizes linear symbols
  again at half scale, as High does, and then on overlapping 1536-pixel windows for what
  that missed. The windows alone lost Code 128 fields of label sheets that High read
  (45 on the development images). With all formats, Very High reads 3,160 symbols of the
  1,578 development images instead of 3,129 (High 3,115), with unchanged wrong reads, and
  costs about 2 to 4% more on retail photos in Chrome.
- Medium and High read many more Code 128 symbols on large label photos and sheets, with
  any format selection. Code 128 and Code 39 are swept over the whole image also when
  retail formats are selected (Medium on frames longer than 1536 pixels), and High and
  Very High sweep large frames with twice as many lines. Once a frame whose shorter side
  exceeds 1536 pixels reads one Code 128, Medium sweeps it again with twice the lines and
  diagonal angles, and Medium and above also decode its stripe boxes, which keep the thin
  fields that the 768-pixel localization raster loses. On the 1,578 development images of
  the 15 October 2026 labels with all formats, Code 128 reads rise from 365 to 1,236 in
  Medium and from 366 to 1,243 in High (Low 786, zxing-cpp 704), with no new wrong reads.
  Medium costs about 4% more on retail photos with all 1D formats; retail-only scans and
  `common` are unchanged.
- Linear formats read the same whatever else is selected. With retail selected, Code 39,
  Code 128, Code 93, Codabar and ITF are also decoded from the central band of up to four
  unread retail boxes: these span the full bar height, and rows near their edges could
  leave a symbol without enough row support (for example a noisy Code 39 patch code on a
  scanned page). A read of a 2D format no longer stops the stripe-box fallback for thin 1D
  symbols: on a 20-megapixel sheet of eleven Code 128 labels next to PDF417 symbols, Medium
  read ten with Code 128 alone and none with PDF417, retail, `common` or `all`, and now
  reads ten with each. On the development images, Medium and High each lose one read
  between a format selected alone and all formats.
- Medium, High and Very High localize linear selections without retail formats (Code 128,
  Code 39, ITF, Codabar, Code 93, DataBar, DataBar Expanded) by segment voting, as Low does,
  which follows rotated and warped bars, and decode the central band of unread boxes;
  frames without a 1D read fall back to the stripe search (sparse in Medium, full from High
  up). ITF and Code 128 keep a whole-image sweep, and ITF start and stop patterns tolerate
  ink spread (such reads need at least six digits). Medium is 25 to 45% faster on these
  selections in Chrome and High up to 18%. On 340 variation images per format selected
  alone, Medium (High) reads against 1.3.0: ITF 203 (205) instead of 166, Code 93 311 (311)
  instead of 291, Code 128 75 (77) instead of 56, Code 39 48 (53) instead of 43, DataBar
  198 (202) instead of 158 (177) and DataBar Expanded 322 (322) instead of 292 (279), with
  no wrong reads. Nine of these images that 1.3.0 Medium read are read in no mode (Code 128
  four, Code 93 three, ITF two).
- High and Very High read retail photos at least as well as Medium: frames their own
  retries leave unread also get Medium's retail recoveries (contrast, high-pass, wide and
  band profiles; deferred proposals; late crops), and unread clusters try segment voting's
  alternative boxes. On the 929 retail photos of the user-project evaluation, High reads
  854 photos correctly instead of 841, like Medium, and photos with a wrong value drop
  from 10 to 8. High no longer runs the stripe localizer on retail scans of frames that
  are not tiny (only Medium uses its short-fragment hints) and tries one direction per seed
  in its upscaled retail crops, as Medium does. In Chrome, High costs about 3% more on
  retail photos than before these changes and 6% less on frames without a barcode.
- Low no longer reports a UPC-E or EAN-8 decoded from part of an EAN-13 or UPC-A it also
  reads. Selections beyond `common1D` (Codabar, Code 93, DataBar or any 2D format) produced
  such fragments on many retail photos. On 929 real development photos with `all`, photos
  with a wrong retail value drop from 16 to 6 (retail selection: 7 to 5), with identical
  correct reads and unchanged EAN-8/UPC-E reads on 751 images of those formats. Medium,
  High and Very High are unchanged.
- Data Matrix reads more phone photos, industrial marks, foreshortened, tiny, blurred and
  crowded-frame symbols. Candidates whose clock-track sides are flat (solid blocks, rules,
  blank areas) are rejected before the size search. Candidates that pass the L-border gate
  but do not decode are retried, best border first: with a wider side-ratio window (strong
  perspective shortens one axis), by tuning the grid corners on gray values and
  thresholding modules against the border levels (bloomed or blurred ink merges the L in
  the binary image), and on an area-averaged crop of a large candidate or a bilinear
  enlargement of a tiny one. Low and Medium try three grid refinements and one crop, High
  six and two, and High also retries boxes of 97 to 287 pixels on a crop at source scale
  and refines each data region of a large symbol (32 x 32 and up) on its own L and clock
  tracks, which follows curvature and lens distortion that one grid cannot.
  Medium keeps its four best-ranked failing candidates (busy frames filled the list with
  bare L borders of the first thresholds) and, once a frame leaves a Data Matrix candidate
  region, takes High's averaged scales. On the 1,578 development images, Data Matrix reads
  rise from 544 to 644 (Low), 604 to 671 (Medium) and 661 to 716 (High), with no new wrong
  reads; on 315 Data Matrix variation images Low reads 163 instead of 129, Medium 180
  instead of 168 and High 179 instead of 168. In Chrome with `common`, Low costs about 1%
  more on retail photos, Medium and High about 2%.
- PDF417 reads more symbols. Macro PDF417 control blocks (structured append with segment
  index, count and file id), ECI codewords 925 and 926 and byte runs of five codewords are
  decoded. Guard ends are followed along their bar and guards are also matched after undoing
  print gain, so perspective and blurred photos rectify; a start guard without a matched
  stop guard also proposes the full-symbol width, a second walk along the guard bar proposes
  taller crops, and the proposal cap rises from 24 to 96 (sheets of 12 or more labels).
  Decodes with erasures must leave two check codewords unused. A symbol cut off at the image
  border is re-read for the column count its row indicators give, with the one or two
  clipped cells of each row as erasures (in Low and Medium for up to two proposals at the
  border). In High and Very High, a
  localized symbol the strict row reader cannot finish is re-read for the column count its
  row indicators give, with nearest-codeword guesses (guessed groups keep 8 check codewords
  unused), and then matched cell by cell against blurred codeword templates, so a symbol
  with error-correction level 0 to 2 must decode without corrections. On zxing-cpp's
  60-file PDF417 sample set, 44 become 55 in every mode (zxing-cpp reads 60). On the 1,578
  development images, PDF417 reads rise from 383 to 471 in Low and Medium and to 507 in
  High, with no new wrong reads. In Chrome with all formats, frames with PDF417 cost about
  7% more in Low, 4% in Medium and 9% in High; other frames cost the same.
- QR costs less in Low and Medium with identical Medium reads. Medium tries at most 25
  alignment-pattern searches per threshold pass (was 150), which false finder triples on
  textured photos used up; Low searches only where an affine fit from the three finders
  reads intact format information (both copies of one orientation within three bit errors
  of the same code, or one copy within one). In Chrome, Medium with `common` is about 5%
  faster on retail photos and frames without a barcode, and EAN-13 with QR about 8%; Low
  is about 3% faster with `common` and 6% with EAN-13 and QR. On the 4,619-image QR
  development selection Medium reads 4,242 known codes (4,233 with EAN-13) as before and
  Low 4,169 instead of 4,165.
- Aztec reads small symbols (about 1.5 to 2 pixels per module) in Medium, High and Very
  High: a bullseye candidate with a well-fitting ring that no threshold pass turned into a
  read is retried on an enlarged window around it, and a read must be centered on the
  candidate (short Rune-like numbers are not accepted from enlarged windows). Low is
  unchanged. On all 315 Aztec variation images Medium reads 156 instead of 137 and High
  159, with no new wrong reads and unchanged reads of every format on the 1,578
  development images. In Chrome, Medium costs within 1% more, High about 4% on retail
  photos with Aztec selected.
- Content parsing in every binding: GS1 element strings, Wi-Fi, contacts,
  payment slips, HIBC, VINs, driver's licenses and more. JavaScript barcodes
  carry `content`; the other bindings add `parse_content`. See
  [structured content](docs/CONTENT.md).
- Native ABI 7 adds the content functions; existing functions are unchanged.
  Swift and Android barcodes add the reader's `gs1` flag.
- New opt-in format `MSIPlessey` (bit 1048576), read when it has a mod-10 check
  digit. See [format coverage](docs/FORMATS.md#msi-plessey).
- New opt-in formats `Code32` and `PZN` (bits 262144 and 524288): Code 39
  pharmacy codes reported as their number. C adds `TAPIRSCAN_FORMATS_OPT_IN`.
  See [format coverage](docs/FORMATS.md#pharmacy-numbers-in-code-39-code32-pzn).
- Data Matrix: large, blurred close-up symbols (tens of pixels per module) are
  read in High and Very High. A frame without a Data Matrix read is retried on
  area-averaged copies (400 and 256 pixels long side in High; 400, 1008, 800 and
  256 in Very High, for frames at least 1.5 times larger), and those copies also
  tighten the corners of a fair-but-unclean candidate grid by a bounded search.
  On the 960 development images of the 15 October 2026 labels, Data Matrix reads
  rise from 537 to 588 (High) and from 538 to 599 (Very High) with no false reads;
  of 46 symbols that zxing-cpp read and Tapirscan missed, High now reads 21.
  Medium is unchanged. On large retail photos High with `common` costs about 5%
  more, with identical reads; other formats' reads are unchanged.
- Linear formats other than retail (Code 128, Code 39, ITF, Codabar, Code 93,
  DataBar) are also localized at finer scale in High (one half-scale raster) and
  Very High (overlapping 1536-pixel windows) on frames whose long side exceeds
  1536 pixels. The 768-pixel localization raster made small symbols in large
  photos, such as short Code 128 fields on part labels, too small to localize.
  On 469 large images of the 15 October 2026 development labels, High reads 444
  instead of 249 Code 128 symbols and Very High 525 instead of 274. Medium is
  unchanged; on large retail photos High with `common1D` costs about 15% more
  (`common` 7%), Very High about 35% (14%), without changing their reads.
- PDF417: when two different payloads are decoded at the same place, the one that
  needed more error corrections is dropped. Low error-correction levels let a
  damaged row group correct into another valid codeword. On 198 labelled PDF417
  images all 388 correct Medium reads remain and wrong payloads fall from 6 to 1.
- QR: finder triples are tried in order of geometric plausibility, and once a
  symbol is read, each threshold pass stops after 100 decode attempts without a
  further read (50 in Low, for live frames). On a 4,619-image QR development
  selection (5,142 known codes),
  Medium reads 4,208 instead of 4,206 and its median scan time falls from 20 to
  11 ms; Low reads 4,138 instead of 4,137, median 19 to 9 ms (Apple M1 Pro,
  native). Images without a read keep the full search.
- QR in mixed selections reads almost like a QR-only scan: QR selected with
  other 2D formats (`common`, `2D`, `all`) is scanned as its own reader group with
  the maintained QR frontend, and QR recoveries are gated on the QR group instead
  of the whole selection; likewise the DataBar and MaxiCode retries apply to a
  DataBar-only or MaxiCode-only group of a mixed selection. The fixed-cost
  foreground-threshold pass runs for mixed selections in High and Very High, and
  in Medium on frames up to 262,144 pixels. Frame-level QR recoveries depend on
  whether a QR symbol was read, so a read of another format next to an unread QR
  no longer suppresses them. On the QR selection, Medium with EAN-13 and QR reads
  4,233 instead of 4,059 known codes (QR-only: 4,242); Low with `common` 4,138
  instead of 4,025. Results of other formats are unchanged. Retail frames with
  EAN-13 and QR cost the same in Medium and High; High and Very High with `all`
  cost about 40-50% more, as their QR recoveries also run on frames where only
  other formats were read.
- QR: unread frames get a block-threshold pass in the style of ZXing's hybrid
  binarizer, which follows sharp shadow edges and large modules: in High and Very
  High on every unread frame (High reads 4,391 instead of 4,343 of the QR
  selection's codes), in Medium where a QR candidate stayed unread (4,217
  instead of 4,208). Each pass has a bounded budget.
- QR: when a sampled grid fails to decode at every sampling offset, modules on
  which the offsets disagree become Reed-Solomon erasures, which cost one parity
  symbol instead of two; two parity symbols stay spare to reject miscorrections.
  A pass that patience stops early now reports `unfinished`.
- QR in Medium: unread images above 1920 pixels also retry at 1280 pixels when
  the 960-pixel retry reads nothing (Barber photos: 1,116 instead of 1,105 known
  codes, about 2% more mean time on the QR selection).
- QR in Medium, High and Very High: when a sampled grid fails, its modules are
  resampled in gray and thresholded against their 7x7-module neighborhood, which
  follows illumination gradients across the symbol; High and Very High then also
  retry with the weakest twelfth of the modules as erasures. On the same QR
  development selection, after the changes above: Medium 4,238 instead of 4,230
  known codes (about 2% more mean time), High 4,402 instead of 4,392.
- QR: unread images up to 300 pixels (thumbnails, cropped symbols), whose modules
  may span two pixels, retry on a 3x bilinear enlargement in every mode; camera
  frames of 640x480 and up never take this pass. On the QR development selection:
  Low 4,167 instead of 4,138 known codes, Medium 4,243 instead of 4,238, High
  4,404 instead of 4,402.
- QR in High and Very High: unread frames above 1280 pixels also retry at 1280
  pixels after 1200 (High on the QR development selection: 4,427 instead of 4,404
  known codes, about 16% more mean time).
- QR in Low and Medium: each detection pass runs at most 150 alignment-pattern
  searches, the costly last stage that plausible but false triples reach on
  textured frames. A textured 640x480 frame without QR scans in 10.6 ms in Low
  (was 14.2 ms before these QR changes) and 27.7 ms in Medium (was 30.4 ms); the
  QR selection loses 2 known codes in Low and 1 in Medium, and its mean time
  falls by 7-8%. High and Very High keep searching.
- MaxiCode selected with other 2D formats is a reader group of its own, and High
  and Very High get Medium's reduced-copy retry for large unread frames; in a mixed
  2D selection it runs where a MaxiCode candidate was found. Four of the five
  full-size MaxiCode label photos of the variation set now read in every mode and
  selection (was one, except Medium with MaxiCode alone); a small sample, so a
  trend rather than a measurement. Single-threaded, all formats cost about 5% more
  for the separate group.
- QR finder triples rank mainly by shape (right angle, equal modules); the size
  term no longer pushes large symbols behind small accidental triples.
- Aztec and Data Matrix detection are faster with identical results: Aztec
  candidate merging uses a spatial grid (36-46% faster at 1-12 megapixels), and
  Data Matrix labels runs from word-wise row transitions and reuses span buffers
  (17% faster at 12 megapixels).
- Rust: `Image::try_from(&dynamic_image)` borrows an 8-bit gray, RGB or RGBA
  `image::DynamicImage`, such as a decoded JPEG, without copying pixels.
- Rust: `Format` implements `Display` with the format names used by every
  binding (`EAN13`, `QRCode`), so `println!("{}", barcode.format)` matches them.
- Images up to 256 megapixels and 65,535 pixels per side are accepted (was 32
  megapixels). Images above 32 megapixels are scanned as an area-averaged
  grayscale copy, with results in the supplied image's coordinates.
- Fixed an integer overflow in the QR retry for images wider than about 4.5
  million pixels in WebAssembly.
- Native builds (C, C++, Java, Python, Rust) run one scan's 1D formats, its 2D
  thresholds and its enabled 2D readers on separate threads, with identical
  results. With all formats, Low scans are 1.7-1.9× and Medium scans 1.35-1.55×
  faster on 960×960 photo crops, and up to 2.9× (Apple M1 Pro) and 2.2× (AMD Ryzen 7 5800X)
  faster on 12-megapixel images. Set `TAPIRSCAN_THREADS=1` to
  keep each scan on one thread, for example on servers running one scanner per core.
  WebAssembly scans stay single-threaded.
- Data Matrix candidates come from a streaming component labeller that keeps only
  eligible components, so large or textured images no longer fall back to a slow
  flood fill. Identical results; Medium with all formats is 23% faster on a
  12-megapixel image.
- Medium retail localization reads image rows as slices, so retail scans of
  images up to 768 pixels on the long side (such as VGA) are 6-13% faster; the QR alignment search skips bounds checks for positions inside
  the image (QR scans 2-5% faster). Identical results.
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

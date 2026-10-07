# Tapirscan demo

[Open the public demo](https://tapirscan.f-kleinicke.de)

A browser interface for trying Tapirscan on photos and camera frames. It uses
the `tapirscan` package, four EAN13 effort modes, and separately loaded
ZXing-WASM 3.1.1 and ZBar-WASM 0.11.0 comparisons. The Detect selector offers
EAN-13, Retail (default: EAN-13, UPC-A, EAN-8, UPC-E), Common (Retail plus
Code 128, Code 39, ITF, QR Code, and Data Matrix), and All (all Tapirscan formats). Every selected scanner receives
the same transformed pixels. ZBar stays selected in Common and All, scanning its
supported subset. It skips Data Matrix, PDF417, Aztec and MaxiCode; the UI notes
these coverage differences. Initialization is
excluded from displayed scan times, and engines differ in search strategies and
completion reporting. The separate Image benchmark panel records a reproducible image suite; timings from different devices are not directly comparable. Camera and image processing are local to the browser.

## Local development

Build all four engines and the additional readers using the
[development guide](../docs/DEVELOPMENT.md), then run from the repository root:

```sh
npm run build --prefix bindings/javascript
cd demo
pnpm install --frozen-lockfile
pnpm check
pnpm build
pnpm test
pnpm dev
```

Deploy `demo/dist/` to a static host when ready. Camera access requires HTTPS
(or localhost). Nothing is deployed by the local build.

The preparation step copies only the selected engine assets and generates a
small synthetic EAN13 example. Four metadata-stripped photographs are tracked
in `public/images`; the [release checklist](../docs/RELEASING.md#demo-deployment)
records their authorization for the demo. `public/engines`, the generated example and `dist` are ignored by
Git. Other research photos, labels, models, result histories and reference
scanners other than the two demo comparison dependencies are excluded. npm and Python packages are built from their
binding directories and exclude this entire app.

Comparison dependencies are pinned in the demo lockfile. Their WASM files are
copied from node_modules at build time into ignored `public/engines`; generated
site output is ignored too. License texts and source/build links are included in
`public/THIRD_PARTY_NOTICES.txt` and `public/licenses`.

## Camera and photo modes

- One Resolution selector applies to video and loaded photos: Full HD (default),
  4K, or Original. Video requests the selected size, or a high native-resolution
  preference for Original. Delivered and scan dimensions are shown; the browser
  may supply a lower resolution.
- Repeated snapshots draw the delivered video frame to a canvas. All selected scanners analyze those same
  pixels before another frame is captured. This path never invokes ImageCapture
  and does not claim higher quality than the video stream.
- Take photo opens the device camera/file picker with `capture=environment`.
  The exact interface and still-photo resolution depend on the device and browser.
- Save image downloads a lossless PNG of the scanner input at the selected resolution,
  including crop, zoom and rotation, without overlays. Live capture saves the most
  recent analyzed frame. The live preview uses a separate canvas to avoid mobile
  video-layer sizing issues; its display size does not affect scanner/export pixels.
- Pause / freeze retains the current photo or a video frame for comparison.
- Changing Resolution immediately reanalyzes the loaded photo from its retained
  original pixels. Full HD/4K cap the long edge at 1920/3840 pixels, preserving
  aspect ratio without upscaling. Original is capped at the scanner’s 32 MP limit.
- Pesto is the default at 1.2× zoom and 0° rotation, with TS-Med, TS-Low, ZXing and ZBar enabled.
  Sauce loads at 2.21× zoom and 34° rotation.
  Pills loads at 2.33× zoom and 45° rotation. Other images start at 1× zoom and 0° rotation. Demo-image buttons include the synthetic example, Pesto, Pills, Sauce, and Sunscreen.
  Camera access begins only after Use camera is pressed.
- Compact scanner buttons select the readers and show scan time. Their success
  shades distinguish 1 (green), 2 (teal), and 3+ (blue-teal) distinct decoded values;
  repeated copies of the same value count once. The exact count remains visible. Result cards immediately below the
  image controls show each scanner’s decoded values and runtime.
- Take photo, camera settings, repeated capture, image adjustments and region
  overlays are under More options.

Camera selection, continuous autofocus, zoom and torch use reported capabilities.
Optional control failures leave the stream usable. Repeated captures pause when
this page is hidden. Camera-mode changes and late capture results use cancellation
checks so stopped sessions cannot overwrite a newer image.

Automated browser checks exercise simulated cameras and capture APIs. Actual
full-resolution output, focus behavior and device-camera behavior still need
verification on physical iPhones, Android devices and webcams.

## Timing and result caching

Each newly loaded decoder runs one warm-up scan before its displayed scan. The
warm-up uses the same image and is excluded from scanner timing. Still-image
results are cached for the current pixels: toggling scanners preserves completed
results and only runs missing methods. Changing the image, resolution or transform
invalidates those results; changing Detect also clears old results and restarts
workers so each new configuration is warmed up. Live camera frames continue to scan normally. Frozen
camera frames do not select any demo-image button.

## Static hosting

After building and testing, deploy `demo/dist/` to a static host with HTTPS.
Only that directory is needed; hosting the demo does not publish the repository
or language packages. The HTTPS site can be opened on phones for camera testing.

For your own Netlify site, run from `demo/` after authenticating the CLI:

```sh
netlify deploy --prod --dir dist --site YOUR_SITE_ID
```

Use your own site identifier. Netlify CLI authentication is local and `.netlify/`
is ignored. Maintainer-specific deployment details may be kept in the optional,
ignored root `MAINTAINER.local.md`; they are not needed to develop or host a fork.

## Image interaction and overlays

The radial controls use distance from the view center for zoom and angle for
rotation. Camera frames fill the viewer without the photo manipulation surround,
including frozen camera frames.

Double-click or double-tap a point in a photo or frozen frame to make it the view center without
changing zoom or rotation. Touch taps allow small finger movements; drags,
pinches and canceled gestures do not count as taps. Subsequent zoom and rotation use that center; Reset image
restores the original center. New images and camera sessions start centered.
A small contrasting crosshair marks the center while dragging, briefly after
recentering, and while scrolling to zoom or rotate. It fades away without
intercepting gestures or changing the analyzed pixels.

The image stage suppresses text selection, long-press callouts and native drag/
context-menu behavior; controls and result text outside it retain normal behavior.
While rotating or zooming a still image, scans continue in batches using identical
pixels for all selected methods. Existing outlines follow the image transform
until replaced by results from a newer scan. An updating label distinguishes
previous outlines from detections on the current pixels. Changing the source or
resolution clears incompatible overlays.

SVG outlines use non-scaling strokes so their visible thickness stays consistent
at Full HD, 4K and Original resolution, including while zooming.

## Fullscreen viewer and result labels

Fullscreen expands the existing photo or live-camera viewer, keeping the same
scan pixels, transforms and overlays. Fullscreen shows only the analyzed frame,
without the surrounding preview margin. Back or Escape returns to the page and
restores keyboard focus. Browsers without native fullscreen use an expanded
viewport instead. On entry, the demo requests a lock to the current screen
orientation; unsupported or denied locks are ignored silently. A viewport resize fits the image without changing its scan rotation.
Actual camera orientation and lock support still require physical-device testing.

Decoded results use the public barcode list for every selected format, including
EAN-8 and QR; EAN-specific diagnostic regions are never used as the decoded list.
Undecoded geometry remains separately available through More options.

Labels group equal values only when their reported locations overlap. Separate
physical instances keep separate labels. Each group shows the barcode once, with
compact color-matched scanner names on one line below. Name slots keep their
positions as later results arrive. Tapirscan modes use TS-Low, TS-Med, TS-High and TS-VHigh consistently
in buttons, overlays and results. A note next to the scanner selection explains
that TS means Tapirscan and the suffixes indicate scan effort.

The normal view shows runtimes in the scanner buttons above the image. Fullscreen
shows scanner names and runtimes in a translucent overlay beside Back, without
shrinking or moving the image. Its order and runtime widths stay fixed while scanners finish.

ZXing and ZXing default are separate comparisons. ZXing enables tryHarder,
tryRotate and tryDownscale; ZXing default explicitly disables those three flags
(the former unchecked behavior). All other library settings remain unchanged.

Label placement is recalculated next to the current barcode bounds on every update,
with a bounded preference for nearby positions and small sideways corrections. The
preference follows the barcode; candidate positions are always rebuilt from its current
bounds, so offsets cannot accumulate. There is no movement delay. Crowded views omit labels with a
count; all values remain in Results. Long payloads are shortened on the image,
with the full value in the title and Results.

Adjust toggles two narrow vertical movement sliders stacked on the right of the image, including
fullscreen. Both rest at zero: hold up/down to rotate or zoom continuously,
with faster movement farther from zero. Release or losing focus stops movement
and returns the handle to center. Readouts show the current angle and zoom.
Labels follow the current barcode geometry during rotation and zoom.

## Additional comparisons and progressive results

Every scanner checked under More scanners gets a card in the main scanner row. More scanners opens a
compact checkbox selector for all scanners, without runtime or result counters. It includes all Tapirscan effort levels, jsQR 1.4.0 and Native.
jsQR supports QR only and returns at most one code per scan; select Common or All.
Native uses the browser's BarcodeDetector, scans the intersection of the selected
and device-supported formats, and reports unavailability without substituting a
fallback. These are optional demo comparisons, never Tapirscan fallbacks.

Each scanner retains its previous result while the next result is pending for the
same source. Cards mark previous view results as updating. Results from a different
source or incompatible resolution are discarded. Overlay input order stays fixed
as asynchronous results arrive; label placement still follows current geometry.

Quagga2 1.12.1 (MIT) uses its raw ImageWrapper initialization path in a dedicated
worker, with multiple linear codes, large locator patches and halfSample disabled.
RGBA-to-grayscale conversion and decoding are timed; there is no PNG encoding or
image loading. The UI remains free to update while Quagga2 scans. Its built-in
worker pool remains disabled; the demo owns the worker and cancellation lifecycle.

ZXing-JS enables TRY_HARDER and quarter-turn rotations, with at most four
full-resolution passes. ZXing-JS default omits TRY_HARDER and uses one pass.
Neither uses extra downscale or inversion passes. Both are independently
selectable under More scanners; there are no separate settings checkboxes.

The Quagga2 worker build applies two guarded compatibility fixes to the pinned
1.12.1 browser bundle: the start check only requires a framegrabber for UI input,
and the raw update path decodes its supplied ImageWrapper without grabbing an image.
Both substitutions fail the build if their upstream text changes. A worker-local
`window` alias accommodates its UMD wrapper; no DOM shim or PNG conversion is used.
Production-worker tests exercise repeated initialization and format switching.

## Analytics

The demo loads the self-hosted Plausible script at
`https://analytics.re4vive.com/js/script.js` only on
`tapirscan.f-kleinicke.de`. It uses the existing legacy analytics dashboard identifier
identifier to preserve its history. No build environment variable is required.
Localhost, deploy previews and forks do not load the tracker. The server also
restricts ingestion to those two production hostnames; adding another hostname
requires updating both lists. The integration sends pageviews, not scanner images
or decoded barcode values. Do not inject synthetic production analytics events
as a deployment check.

## PDF upload

Load image / PDF accepts local PDFs and scans one complete page at a time.
Previous/Next page switches pages and replaces the results. PDFs retain their
stored page orientation; all image zoom, rotation and recenter controls are
unavailable. Scanner format and input-resolution settings still apply.
PDF.js loads only when opening a PDF. Pages rasterize at up to 300 dpi, limited
to a 4096-pixel long edge and 16 MP before the scanner's resolution limit.
Only the current page is retained as an image. Password-protected files must be
unlocked before loading. Parsing uses PDF.js's worker, and its worker, fonts,
character maps and WASM helpers are served from the demo's own origin.

## Image benchmark

Open **Image benchmark** below the examples. The panel compares the actual browser
BarcodeDetector (no fallback), ZXing-WASM, ZBar-WASM, Tapirscan Low and Tapirscan Medium using
Retail formats. All image processing happens locally; uploaded files are never
sent to a server.

- **Batch:** choose any number of image files, then Run uploaded images. Files
  decode and scan sequentially, with no application file-count cap. Original
  resolution is optional; the default limits the longest side to 1920 pixels.
  Results and thumbnails consume memory in proportion to the file count.
- **Stress test:** choose one file and Generate variations, or use Stress-test
  current image. Non-zoom variants preserve the complete image, including all
  barcodes and background. Ordinary rotation uses padding to avoid clipping. The baseline keeps the chosen resolution; only
  small variants deliberately downsample. The suite includes rotation, scaling,
  all four edges and corners, blur, contrast, compression and perspective.
  Destructive crop controls are excluded. Nine zoom variants enlarge the original
  photo within a fixed viewport, centered on the first detected barcode (Medium
  first; image-center fallback). Targets are 35%, 60% and 85% of the limiting frame
  dimension, capped at 8×, plus rotated and near-edge cases. These are tagged
  zoomed / viewport crop: other barcodes and background may leave the frame.
  Zooming interpolates pixels; it does not add image detail. Current-image tests ignore the main preview's
  zoom/rotation and have their own resolution setting.
- **Saved examples:** the five existing public examples include precomputed
  reports. Their browser/version/timestamp and engine identities are shown.
  Run locally to measure the visitor's device; saved timings are not presented
  as local measurements.

The table and gallery share method/tag/read/repeated-value/error filters. Each
method includes scan time, decoded values and numbered returned geometry.
ZBar outlines are hulls of returned sample points. Repeated values can represent
separate physical symbols, so these are possible repeats, not confirmed duplicate
scans. No upload has ground-truth labels: detection counts are not accuracy.
Stop retains a partial report, including its cancelled status. JSON downloads
include all scenes, not only the current filter or page.

Each engine receives one untimed blank warm-up. Reported milliseconds cover a
single scanner call per scene, excluding image preparation, loading, worker
transfer and ZBar state creation. Native includes detector creation and format
query; Low and Medium include their debug-enabled synchronous scans. Both use
their normal mode budgets and report unfinished work. No claim of statistically significant
speed ranking is made from these samples.

### Regenerate and verify

Browser automation uses an optional Playwright installation with Chrome:

```sh
# After changes to the local binding, rebuild it and refresh the file dependency first.
# From demo/: npm run build --prefix ../bindings/javascript && pnpm install --force --offline --frozen-lockfile
# PLAYWRIGHT_MODULE may name an installed Playwright module by absolute path.
npm run benchmark:precompute
npm run build
npm run check
npm test
node scripts/benchmark-browser.test.mjs
```

The generator starts an isolated local Vite server and runs the same transformation
and worker code as the UI. Commit `public/benchmarks/*.json` after regeneration.
The build regenerates the identity manifest from the recipe, example pixels,
worker adapters, JavaScript binding and WASM bytes. The UI rejects stale saved
reports when identities differ. Tests cover a real 100-image five-engine batch,
invalid file isolation, single-image variants, cancellation, source changes,
overlays, filters and mobile overflow. Browser results depend on platform-native
format support; the browser test expects BarcodeDetector to support EAN-13.

Local builds and benchmark generation do not deploy the website.

The statistics table groups successfully processed images into 0, exactly 1, or
2+ unique decoded values, with matching filters. Multiple returns of the same
value count once for these groups; total detections and repeated-read counts
still preserve all returned detections. Errors are separate from zero detections.

Gallery thumbnails are 720-pixel, high-quality JPEG previews, while decoders use
larger scan images. **Inspect full resolution** recreates a scene from the example
or retained upload and the same transform at its scan dimensions, with fit,
1:1 and 2× views, per-method detection overlays and decoded values. Only the
inspected image is recreated, so batch runs do not retain full-resolution copies
of every scene in memory. A reported detection is not verified correctness.
Pagination reserves square image frames and anchors new pages at the top controls
so lazy image loading cannot collapse the page or shift the reading position.

## Tapirscan versions

The public demo offers fixed current and next scanner entries, without a version
selector. Standard TS entries use the registry default
`1.2.2+release-r2.20260925`; the next entries pin
`1.2.2+retail-runtime.20260929`. This keeps current-versus-next comparisons distinct.

`src/lib/scanner-versions.json` retains historical versions and pins each version's
four WASM assets. Preparation and worker loading verify SHA-256 hashes. Keep old
bytes immutable when adding versions. Saved example benchmarks must match the
requested build identity.

More options → Show analyzed areas also
shows reported proposals, primary EAN13/UPCA candidate counts, candidates without a
primary EAN13/UPCA read and localization-limit omissions. Fast-discarded areas are not exposed by the
current builds and are explicitly marked unavailable, rather than inferred from
failed decodes.

Image benchmark is hidden by default for the current public deployment. To enable
it in a later build, set `VITE_ENABLE_IMAGE_BENCHMARK=true` when building the demo.

## Experimental fast scanners

**TS-Low** is the former TS-Turbo, now using the selected public Low build beside
TS-Med. **TS-Low Classic** is the original Low approach under **More scanners**;
it preserves the improved original Low on a fixed consensus build.

The experimental Turbo tiers are optional checkboxes under **More scanners**.
Their numbers name speed targets relative to the original Turbo (now TS-Low),
not guaranteed speedups for every image or format. Select **Detect → Common1D**
for EAN-13, UPC-A, EAN-8, UPC-E, Code128, Code39 and ITF. Other explicit format
selections and **All** are supported. **Detect → 2D** selects QR Code, Data Matrix,
PDF417, Aztec and MaxiCode directly. Every Turbo tier shares the same guarded,
source-resolution 2D fast path; their numeric speed targets apply to Common1D.
Small code groups on large uniform backgrounds can decode much faster, while
busy photographs retain the full-image readers. MaxiCode remains less reliable
than the other matrix formats. These experiments remain outside the public API.

**All** also applies oriented proposal decoding to Codabar, Code93, DataBar and
DataBar Expanded, with source-based checks to consolidate repeated observations.
The complementary readers remain available for missed and stacked symbols.
This update primarily improves coverage; All-format scan time is approximately
unchanged in the measured development cohort. Common intentionally excludes
these four additional linear formats.

These scanners support rotated codes but deliberately omit recovery work,
always report unfinished, and can miss codes or produce repeated values and
partial outlines. The numbered Turbo experiments use fixed builds independent of
the current/next builds; TS-Low uses the default public release.
Images stay local.

`src/lib/turbo.json` pins each artifact's source commit, source digest, build
environment and WASM hash. These assets are demo-only and excluded from language
packages. Reproduce a variant by checking out its recorded commit in an isolated
worktree and running `python3 scripts/build_wasm.py --development low` with its
recorded environment. Copy the resulting `build/wasm-development/assets/low.wasm`
to `build/demo-experiments/<file>` using the manifest filename. Asset preparation
and worker loading verify the pinned SHA-256. Existing immutable assets remain
available; this does not add public API modes.

### Basic ZXing comparisons

**ZXing default** means the former checkbox-off behavior: tryHarder, tryRotate
and tryDownscale are explicitly false, with other library defaults preserved.
**ZXing-JS default** uses one full-resolution pass without TRY_HARDER or added
rotation, downscale or inversion passes. These basic configurations reduce search
work and may miss difficult codes. The separate **ZXing** and **ZXing-JS** entries
retain the enhanced checkbox-on behavior.

TS-Low is enabled on initial load alongside TS-Med, ZXing and ZBar. All scanners
can be toggled under **More scanners**. Checking a reader adds and activates its runtime/result card above the image.
Clicking the card pauses or resumes scanning while keeping it visible. Unchecking
the reader under More scanners removes the card and deactivates it.

## Public Low mapping

Public `low` now uses the former Turbo approach. TS-Low follows the selected
public release. TS-Low Classic preserves the original Low implementation on its
fixed improved consensus build. See [Low and Low Classic](../docs/LOW_MODES.md).

## Public documentation

`content/docs.json` contains the website documentation. `scripts/build-docs.mjs` generates static HTML, Markdown exports, license downloads, `llms.txt`, and the sitemap during preparation. The format table comes from `config/formats.json`; complete API Markdown comes from the binding guides. Update both version-specific content and examples when preparing a new release.

# Tapirscan demo

[Open the public demo](https://tapirscan.netlify.app)

A browser interface for trying Tapirscan on photos and camera frames. It uses
the `tapirscan` package, four EAN13 effort modes, and separately loaded
ZXing-WASM 3.1.1 and ZBar-WASM 0.11.0 comparisons. The Detect selector offers
EAN-13, Retail (default: EAN-13, UPC-A, EAN-8, UPC-E), Common (Retail plus
Code 128, Code 39, ITF, QR Code, and Data Matrix), and All (all Tapirscan formats). Every selected scanner receives
the same transformed pixels. ZBar stays selected in Common and All, scanning its
supported subset. It skips Data Matrix, PDF417, Aztec and MaxiCode; the UI notes
these coverage differences. Initialization is
excluded from displayed scan times, and engines differ in search strategies and
completion reporting; this interactive comparison is not a benchmark. Camera and image processing are local to the browser.

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
in `public/images`; see [image provenance and usage](public/images/README.md). `public/engines`, the generated example and `dist` are ignored by
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
- Pesto is the default at 1.2× zoom and 0° rotation, with TS-Low, TS-Med, ZXing and ZBar enabled.
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

TS-Med, ZXing and ZBar stay in the main scanner row. More scanners opens a
checkbox dropdown for the other Tapirscan effort levels, jsQR 1.4.0 and Native.
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

The Quagga2 worker build applies two guarded compatibility fixes to the pinned
1.12.1 browser bundle: the start check only requires a framegrabber for UI input,
and the raw update path decodes its supplied ImageWrapper without grabbing an image.
Both substitutions fail the build if their upstream text changes. A worker-local
`window` alias accommodates its UMD wrapper; no DOM shim or PNG conversion is used.
Production-worker tests exercise repeated initialization and format switching.

## Analytics

The demo loads the self-hosted Plausible script at
`https://analytics.re4vive.com/js/script.js` only on `tapirscan.netlify.app` and
`tapirscan.f-kleinicke.de`. Both use the existing `tapirscan.netlify.app` dashboard
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

## Scanner names and selection

See [Low and Low Classic](../docs/LOW_MODES.md) for the public API mapping.
TS-Low is the former Turbo and uses the selected public Low build. TS-Low Classic
is the original Low implementation, pinned to the improved consensus build.
Turbo2/4/8/16 remain optional private comparisons. Detect supports Common1D, 2D
and All in addition to Retail and Common.

TS-Low starts active alongside TS-Med, ZXing and ZBar. More scanners contains all
readers in a compact checkbox selector. Checking adds and activates a card;
clicking a card pauses/resumes it without hiding it. Unchecking removes it.

ZXing default disables tryHarder, tryRotate and tryDownscale and preserves all
other library defaults. ZXing-JS default uses a single pass without TRY_HARDER or
added rotations. Enhanced ZXing and ZXing-JS are separate entries; their previous
settings checkboxes have been removed. The basic modes trade recovery for speed.

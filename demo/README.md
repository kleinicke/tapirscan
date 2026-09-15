# Tapirscan demo

[Open the public demo](https://tapirscan.netlify.app)

A browser interface for trying Tapirscan on photos and camera frames. It uses
the `tapirscan` package, four EAN13 effort modes, and separately loaded
ZXing-WASM 3.1.1 and ZBar-WASM 0.11.0 comparisons. The public demo is fixed to
EAN13; every selected scanner receives the same transformed pixels and EAN13
restriction. Initialization is
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
- Pause / freeze retains the current photo or a video frame for comparison.
- Changing Resolution immediately reanalyzes the loaded photo from its retained
  original pixels. Full HD/4K cap the long edge at 1920/3840 pixels, preserving
  aspect ratio without upscaling. Original is capped at the scanner’s 32 MP limit.
- Pesto is the default at 1.2× zoom and 0° rotation, with Medium, ZXing and ZBar enabled.
  Sauce loads at 2.21× zoom and 34° rotation.
  Pills loads at 2.33× zoom and 45° rotation. Other images start at 1× zoom and 0° rotation. Demo-image buttons include the synthetic example, Pesto, Pills, Sauce, and Sunscreen.
  Camera access begins only after Use camera is pressed.
- Compact scanner buttons include scan time. Result cards immediately below the
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
invalidates those results; live camera frames continue to scan normally. Frozen
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

The image stage suppresses text selection, long-press callouts and native drag/
context-menu behavior; controls and result text outside it retain normal behavior.
While rotating or zooming a still image, scans continue in batches using identical
pixels for all selected methods. Existing outlines follow the image transform
until replaced by results from a newer scan. An updating label distinguishes
previous outlines from detections on the current pixels. Changing the source or
resolution clears incompatible overlays.

SVG outlines use non-scaling strokes so their visible thickness stays consistent
at Full HD, 4K and Original resolution, including while zooming.

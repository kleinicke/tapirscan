# Comparing scanner changes

Build the baseline and candidate before measuring. Keep the baseline checkout and
its built artifacts unchanged until the comparison finishes. The command requires
Python with Pillow; WASM comparisons also require Node 24 and built JavaScript
packages in both checkouts.

```sh
python scripts/compare_scanners.py \
  --baseline /path/to/baseline \
  --candidate /path/to/candidate \
  --dataset-root /path/to/tapirscan-datasets \
  --manifest /path/to/manifest.json \
  --output /path/to/evidence/native
```

Run again with `--backend wasm --output /path/to/evidence/wasm` for WASM. The
defaults select all four modes, EAN13 and retail formats, full
diagnostics and five timing repetitions on 50 evenly spaced images; narrow a run
with `--modes`, `--formats` or `--timing-count`. `--help` lists every flag.

Native and WASM runs check output parity between two builds. Timings that are
recorded or published come from Chrome: `--backend browser --browser-channel chrome`
(see [the runtime rule](../../tapirscan-web/AGENTS.md)).

The manifest is a nonempty JSON array. Each row names an encoded image with
`image`, or a raw pixel file with `file`, `width`, `height`, optional `channels`
(default 1) and `stride` (default width × channels). Paths are relative to
`--dataset-root`; absolute paths also work. Optional `formats` overrides the
command's selections for that row. `eanAddOnPolicy` accepts `Ignore`, `Read`, or
`Require`. Encoded images are converted to RGBA without applying EXIF rotation.

Native builds are found in each checkout's `build/native`. WASM selection comes
from `bindings/javascript/wasm/build.json`, with assets in the same directory;
the `--baseline-*` and `--candidate-*` native, wasm and assets options select
captured artifacts instead. The harness uses its current Python facade for both
native libraries, which must support that ABI. WASM uses each checkout's built
JavaScript facade. This is a same-backend before/after comparison; it does not
assert native/WASM floating-point equivalence.

The command refuses to overwrite an evidence directory. `report.json` records the
source state, artifact hashes, `build.json` source digest, host and runtime,
comparison counts and timing distributions; `inputs.json` identifies the input
files, and `differences.jsonl` keeps the first differences (`--max-differences`).
Any mismatch or load/scan error exits with status 1 and leaves no final report.

Parity preserves ordering, geometry, metadata and work counters, excluding only
known elapsed-time fields. Measurements follow parity, with images preloaded,
scanners reused, warm-up scans, alternating baseline/candidate order, and debug
output disabled. Timings include public binding conversion and exclude compilation,
image decoding and result comparison. `timings.jsonl` retains every paired sample;
summary groups separate modes, formats and supplement policy. Avoid
concurrent builds or other heavy work while measuring.

The browser backend needs `--playwright-module /absolute/path/to/playwright/index.mjs`
and runs the same comparisons in headless Chromium (the default channel) or an
installed Google Chrome. A loopback server exposes only the two built JavaScript
packages and selected WASM files. Pixel transport, compilation, image decoding and
result serialization are outside the browser's scan timer. The report records the
browser version and user agent. Native, Node and browser groups are separate
evidence; desktop Chromium is not a mobile benchmark.

`--save-results` retains public outputs in `results.jsonl` for independent scoring.
`--allow-differences` records algorithm changes without treating unequal outputs
as a failed parity check; it does not ignore load or scan errors. Keep the strict
default for structural refactors. Dataset labels are never passed to the scanner.

## Measuring an integration on a phone

Comparing scanners inside an app on a phone measures the whole path: camera
start, frame hand-over, scheduling and decoding. Small mistakes in the setup
outweigh the differences between decoders:

- **Time from the first camera frame**, not from the tap. Camera permission and
  start-up (often 0.5-1 s) vary between runs and are not the decoder's. Record
  when the `<video>` shows its first frame (`requestVideoFrameCallback`, or
  `readyState >= 2` with `currentTime > 0`) and when the code is read.
- **Skip the first opening** after a page load: it includes downloads,
  compilation and engine warm-up. Report it separately as start-up.
- **One code at a time**, held the same way for every scanner, and several
  repetitions per code. Alternate the scanner order between repetitions.
- **Log which decoder read each code** when an app runs several decoders, in
  turn or as fallbacks, and how many frames each one received. A decoder that
  gets every third frame reads later for reasons that have nothing to do with it.
- **Count readability and speed separately.** First: which codes each scanner
  reads at all within a time limit, and its wrong reads. Then: the time to read
  for the codes both read. A scanner that reads more hard codes looks slower in
  a mean over its reads.
- **Serve it like production:** a production build, with the server's
  compression and caching headers, over HTTPS (a tunnel to a local server works),
  and the service worker if the app has one. A development server serves
  unminified, uncompressed files.
- **Frame loop and formats** decide most of the time per frame: note the
  selected formats, the mode, the frame size and any fixed wait between scans,
  and keep them equal across scanners where the app allows it.
- **Safari's user agent is frozen.** Since Safari 26 it no longer reports the
  current iOS version (as macOS Safari reports 10_15_7), it never names the device
  model, and every iOS browser uses WebKit. Record the iOS version, browser and
  device model by hand.
- **Indicative, not general:** a dozen codes on one phone shows where a scanner
  helps or fails; it is not a recall or speed benchmark. Keep the codes, so the
  same set can be repeated after a change.

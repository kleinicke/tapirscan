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

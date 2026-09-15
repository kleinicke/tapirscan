# Source-detail promotion, 14 September 2026

Unreleased package version **0.1.2** promotes the complete Medium, High and Very
high detail variants from research commit
`a819749edb79d6e91bb28bc803025dd13023054f`. Low and the selected additional-format
reader sources remain unchanged. Existing defaults remain Medium and EAN-13.

## Exact inputs

`provenance/modes.json` selects `medium-detail-20260914`,
`high-detail-20260914` and `very-high-detail-20260914`. The accompanying research
configuration is preserved in `provenance/detail-20260914.json`. All three WASM
binaries reproduce its SHA-256 hashes with Rust 1.91.1 and SIMD enabled.

The update includes primary retry scheduling, source-evidence masks, at most
two texture seeds, 256-pixel crops enlarged threefold, and a separately pinned
`nano-lint-20260913` recovery decoder. Medium uses one geometry refinement and
one recovery direction; High uses four refinements and two directions; Very
high uses one refinement, its secondary grid, and two recovery directions.
Every primary candidate retains its cheap attempt and full-frame search remains.
Recovery is bounded and therefore keeps `unfinished` true.

The frozen Rust base is unchanged. Only the three experiment recipes and the
matching upstream recipe builder were imported. `provenance/import.json` pins
the imported JS runtime; its upstream `direct-recovery.mjs` is retained separately
as `provenance/direct-recovery-upstream.mjs`.

## Platform integration

The release recovery copy changes only canvas creation and ImageData construction
through `detail-canvas.ts`. Browsers retain the research Canvas context settings
and interpolation. Node uses a bounded software bilinear canvas, with rounded
8-bit channels. Browser interpolation is implementation-dependent; this is not
a promise of pixel-identical resampling across every browser and OS.

`detail.ts` preserves gray/RGB/RGBA and padded-stride inputs. Alpha remains ignored,
including during crop enlargement. Raw crop evidence and transforms are exposed
under `recovery.attempts` when regions are requested. Recovered public reads use
empty `candidate_indices` so a crop-local index cannot point to a primary-frame
candidate. The demo uses composed source-coordinate `detailRegions`, including
undecoded recovery geometry. Mixed-format scans retain undecoded recovery regions.

`bindings/rust/src/detail.rs` ports the same bounded pipeline for Rust, C, C++,
Python and Java. The native build prepares a separate Low crate and changes only
its package name to avoid Cargo feature unification with the primary decoder.
The existing native-only visibility adapter is retained. No native ABI change
is required. Native crop interpolation matches the Node software implementation;
raw timing records differ between native and browser hosts.

## Validation

- All three primary WASM SHA-256 hashes match the pinned research config.
- Selected core tests and all four native facade/C ABI tests pass.
- Existing Rust, C++, Python, Java and JavaScript output/ownership tests pass.
- Strict maintained Rust, JavaScript, Python, C/C++ and Java checks pass.
- All four CMake packages load after installation and relocation. Python image
  adapters pass (one unavailable-device case skipped).
- The npm tarball contains the complete runtime and current WASMs: 43 files,
  1,323,813 bytes compressed; demo and research assets are excluded.
- Chrome research/release comparison: 36 generated small/rotated barcode scans,
  identical decoded values, support, polygons, primary candidates and composed
  regions; seven decoded outputs came from recovery.
- Native versus Node/WASM: the same 36 scans and seven recovery additions match
  on decoded values, support and polygons (coordinates rounded to five decimals).
- Demo production workers load all four modes and retain working ZXing/ZBar
  comparisons, including deployment beneath a URL subpath.

These are integration and regression checks, not a new performance benchmark or
physical-device camera test. The research development study reports one Medium
read regression and some slower slices despite aggregate gains. Its 300 images
include correlated derivatives of one photo; it is not a release holdout. Detailed
research evidence remains in the experiment repo at
`benchmark/reports/effort_detail_20260914.md`.

Reproduce after building all modes and native bindings:

```sh
python3 scripts/verify_import.py
python3 scripts/test_detail.py
# Requires Pillow in QUALITY_PYTHON and Chrome/Playwright in the research repo:
QUALITY_PYTHON=python3 node bindings/javascript/test/detail-browser.mjs
```

The generated test images live only in temporary directories. No additional
research images, labels, datasets or model weights enter the release repository.

The validated demo is deployed at https://tapirscan.netlify.app/ (Netlify deploy
`6aa816f4c9c8d33ab335d58a`). All four live WASM hashes match the promotion manifest.
Package registries were not published.

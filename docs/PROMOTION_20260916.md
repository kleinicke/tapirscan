# QR, Common1D and strict Clippy promotion

Source: research revision `85734fe17`; exact revision, recipe and host hashes are
recorded in [promotion provenance](../provenance/promotion-20260916.json).
Package version is **1.1.0**, native ABI **4**. Nothing was published.

## Selected implementation

- Strict-Clippy base core and four selected recipes, with new immutable
  `*-release-20260916` tags and canonical WASM hashes in
  [modes.json](../provenance/modes.json).
- Exact `common1d-conservative-qr-20260916` multiformat source: reusable scanline
  buffers, low-contrast handling, conservative Code 128 and ITF checks, QR
  preparation improvements, and bounded High/Very High QR recovery.
- Common1D effort 0/1/2/2 and QR effort 0/1/2/3 across Low/Medium/High/Very High.
  Other matrix readers retain their fixed search.
- QR-only packed RGBA uploads directly into WASM, preserving integer grayscale
  conversion and ignoring alpha. Other pixel layouts keep their existing path.
- Strong supplemental linear reads can defer deep EAN retries for wholly
  contained proposals and source-detail seeds. Every normal proposal still gets
  initial discovery, and full-frame search remains enabled.

The release's source-detail recovery, fit limits, recovery directions and separate
Low recovery engine are preserved. The research host configuration was not copied
wholesale. Experimental shared-retail and late ITF width compensation are not selected.
New detail host modules adapt research imports to the release's browser/Node canvas
implementation. Recipe registration is the only imported build-helper adaptation.
Historical snapshots and their original provenance remain reproducible from the
previous release commit recorded in the manifest; their recipes require that base.

## API review

Python implementation, C ABI, C++ and Java public interfaces are unchanged.
The generated JavaScript public `index.d.ts` is byte-identical to the previous
release build. Public defaults, image layouts, optional BGR support, immutable
results, payload bytes, supplement policies, support ranking and diagnostic shapes
are preserved. Mode selection now controls the promoted readers' effort as well.

There is **no public continuation option**. Search remains bounded and continues
according to each reader's existing policy; it is not an exhaustive search until
all viable candidates are exhausted. `unfinished` reports known remaining or
limited work. As requested, this promotion adds no continuation mode or new API concept.

## Local validation

All checks below passed on macOS ARM64 with Rust 1.91.1:

| Check                     | Evidence                                                                                                                                              |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Source provenance         | 149 pinned files verified; new mode WASM hashes reproduced                                                                                            |
| Complete quality gate     | Formatting, provenance, JS, Python, native warnings, Rust, demo, tooling tests, imported core                                                         |
| Strict Clippy             | Base fast/quality, all four selected recipes, multiformat, all four Rust/C facades                                                                    |
| Multiformat unit tests    | 88 passed                                                                                                                                             |
| JS API tests              | 19 passed, including coverage-mask boundary checks                                                                                                    |
| Cross-language bindings   | All four end-to-end tests passed                                                                                                                      |
| Supplement parity         | 240 mode/policy cases, debug enabled and disabled                                                                                                     |
| Multiformat parity        | Mixed EAN/Code128/QR, separate identical instances, five formats across languages, four QR pixel layouts in every mode                                |
| Source-detail parity      | Generated scaled/rotated scenes, native/WASM                                                                                                          |
| Paired release regression | 18 generated EAN scenes × four modes: exact values, support, polygons rounded to five decimals, and `unfinished` against the saved prior native build |
| Python image inputs       | 23 tests: 21 passed, two optional-dependency checks skipped                                                                                           |
| Installed Python wheel    | Bundled libraries in all modes; 48 metadata/bytes/work-limit fixture cases                                                                            |
| Installed npm package     | Node 48 fixture cases; Chromium 96 scans with default assets and 96 with relocated assets; eight worker checks                                        |

The four QR layouts cover grayscale, RGB, packed RGBA and padded RGBA, including
colored pixels and nonopaque alpha. These checks are part of
`scripts/test_multiformat.py`, already run by CI. The coverage regression tests
verify that partial overlap does not suppress adjacent proposals and that the
full-frame bit remains enabled.

Routine reproduction:

```sh
python3 scripts/verify_import.py
# Run each mode separately; these commands test and verify its WASM hash.
python3 scripts/build.py low
python3 scripts/build.py medium
python3 scripts/build.py high
python3 scripts/build.py very-high
python3 scripts/build_multiformat.py
python3 scripts/build_native.py
npm run build --prefix bindings/javascript
npm test --prefix bindings/javascript
node tools/quality/all.mjs
python3 scripts/test_bindings.py
python3 scripts/test_multiformat.py
python3 scripts/test_detail.py
python3 scripts/test_supplements.py --library-dir build/native --encoder /path/to/zint
```

C++/Java test tools and Python fixture dependencies must first be prepared as in
[development](DEVELOPMENT.md) and CI. Package installation tests are described in
[releasing](RELEASING.md). Raw local logs and generated fixtures stay under ignored
`build/promotion-20260916/`.

This validation establishes the tested API and parity behavior, not a universal
speed improvement or exhaustive real-world detection guarantee. No new performance
claim is made from these synthetic checks. Higher effort deliberately permits more work.

# Independent multiformat research

This crate implements experimental barcode localization, sampling, error
correction and decoding. It links no barcode decoder. The optional JavaScript
wrapper combines it with the frozen EAN-13 Medium scanner; it does not replace
or modify that scanner. No release or general ZXing superiority is established.

The explicit format IDs are `EAN13`, `UPCA`, `EAN8`, `UPCE`, `Code128`, `Code39`,
`Code93`, `ITF`, `Codabar`, `DataBar`, `DataBarExpanded`, `QRCode`, `DataMatrix`,
`PDF417`, `Aztec`, and `MaxiCode`. The default is EAN13 alone. UPC-A uses the same optical
structure as zero-prefixed EAN-13 in the Medium wrapper. All other formats are
opt-in and load a separate WASM artifact only when first needed.

Implemented families include DataBar Omnidirectional/Stacked/Stacked
Omnidirectional, Expanded/Expanded Stacked, QR Model 2, Data Matrix ECC200/DMRE,
normal/Compact PDF417, compact/full Aztec plus Rune, and MaxiCode modes 2–6.
MaxiCode includes ECI and structured append; image localization currently uses
affine circular-finder geometry. Micro QR, QR Model 1, rMQR,
Micro PDF417 and several optional payload variants remain unsupported.
QR localization includes bounded two-finder recovery after its ordinary
three-finder attempts. This helps some damaged/cropped symbols; it does not
cover arbitrary missing-finder geometry or all single-finder cases.
Non-character/general-purpose ECI values are not interpreted as text; for example,
the upstream MaxiCode mixed-ECI binary fixture remains undecoded despite successful
error correction. A native probe records this limitation separately from the
eight labeled text fixtures. Code39
currently returns standard characters; automatic full-ASCII interpretation is
not yet implemented. PDF417 Macro/structured append remains unsupported. Reader
coverage is experimental and varies substantially.

## JavaScript research API

`js/camera-demo/src/lib/multiformat/scanner.ts` exposes:

```ts
const scanner = await MediumMultiformatScanner.create(mediumWasm, extraWasm);
const frame = scanner.scan(
  { data: rgba, width, height, channels: 4, stride: width * 4 },
  ["EAN13", "Code128", "QRCode"],
  { eanAddOnSymbol: "Ignore", linearStrategy: "scanlines" },
);
scanner.dispose();
```

Images must use a `Uint8Array` with 1, 3 or 4 channels, valid row stride,
minimum dimensions 3×3, and at most 32 megapixels. The research page prepares
images at a maximum side of 1600 pixels. Its image loading/resizing is outside
the scanner clock and is not part of a browser throughput claim.

`frame.barcodes` contains decoded reads. `frame.regions` contains those reads
plus qualified localized-but-undecoded candidates. Polygons use original input
coordinates. A read has `text`, `format`, `polygon`, `support` and experimental
`rank`; optional metadata includes `gs1`, `eanAddOn`, `readerInitialization` and
`structuredAppend`. Structured-append indices are **one-based**, with `count`
and format-specific `id` or `parity`. Payloads are returned per symbol; automatic
cross-symbol assembly is not implemented. Initialization symbols are reported,
never executed as scanner configuration.

EAN/UPC supplements use `Ignore` (default), `Read`, or `Require`. `eanAddOn`
contains the two or five supplemental digits separately from the base text.
Supplement reading uses the independent full-frame retail reader because the
frozen Medium crop can exclude the supplement. Extra work is off by default.

`linearStrategy: "medium-localized"` is an experimental alternative that uses
Medium candidates for additional linear formats. It currently has important
runtime, duplicate and candidate-limit weaknesses. The default `"scanlines"`
strategy preserves full-frame sampling. It retains structurally recognized
EAN/UPC, Code128 and Code93 regions after checksum failure, requiring repeated
scanline evidence and strong pattern agreement. Retail regions also survive a
missing required supplement. General unread localization for Code39, ITF,
Codabar and failed DataBar payloads remains incomplete. Matrix localizers retain
qualified unread regions. `unfinished` signals known
candidate/attempt limits; not every internal cap is yet propagated. Localization
scores and ranking are not calibrated probabilities.

## EAN-8 recovery update

EAN-8 now adds bounded local-contrast and sharpened axial scanline retries at
effort >= 1, plus blur-aware grayscale digit matching for recognized candidates
whose ordinary payload decode failed. The grayscale reader requires unambiguous
digits, checksum validation and repeated scanline evidence; it does not repair
digits by searching for a matching checksum. EAN-8 geometry preserves decoded
scanline endpoints on curved labels. Other format masks and the frozen EAN-13
Medium default retain their existing paths.

The five user-reported cases pass the website's existing matching rule. One
bottle box remains partial, and browser runtime is above the 1.5x ZXing target.
See [the development regression report](../../benchmark/reports/ean8_review_20260913.md).

## Reproduction

From the repository root:

```sh
cargo test --manifest-path rust/multiformat/Cargo.toml --offline
cargo build --manifest-path rust/multiformat/Cargo.toml --release --offline
UV_CACHE_DIR=/tmp/barcode-uv-cache uv run --no-sync python benchmark/scripts/multiformat_20260913/build_wasm.py
```

The WASM build script records compiler flags, source hashes and the frozen
Medium hash, archives the exact sources, and publishes local immutable assets
for the demo's `#formats` page. See the benchmark scripts' README and
`benchmark/reports/multiformat_20260913.md` for measured results and
limitations. Native timings do not establish browser performance.

Reference encoders generate test images and supply attributed standard symbol
tables. Runtime decoding and localization are project-owned. `serde` and
`serde_json` provide serialization; `encoding_rs` provides character conversion.
See `THIRD_PARTY_NOTICES.md` for notices. Dataset payloads and generated runs are
not tracked in Git.

# Format coverage

EAN13 is the default. Additional formats are explicitly selected at scanner
creation in JavaScript and Python. Python also allows per-scan overrides;
JavaScript accepts per-scan subsets of the creation selection.
Rust and the native ABI select formats per scan. Python and JavaScript accept
`"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"` and `"all"` presets for their supported formats, a single identifier,
or explicit lists.

| Preset       | Formats                              |
| ------------ | ------------------------------------ |
| `"retail"`   | EAN13, UPCA, EAN8, UPCE              |
| `"common1D"` | Retail + Code128, Code39, ITF        |
| `"common"`   | common1D + QRCode, DataMatrix        |
| `"1D"`       | All supported linear formats         |
| `"2D"`       | All supported matrix/stacked formats |
| `"all"`      | All supported formats                |

Retail covers the EAN/UPC family, not every format used in retail (for example,
DataBar requires an explicit selection or `"1D"`). Common is a convenience selection,
not a coverage or accuracy guarantee. EAN13 remains the default.

EAN13 and UPCA share the primary scan; selecting UPCA adds output normalization,
not another image scan. Retail additionally runs the extra engine for EAN8/UPCE,
sharing its grayscale image and scanline traversal. It does not invoke matrix
readers. This additional pass costs time depending on image size and content.
JavaScript also loads the extra WASM module at creation; reuse a scanner across
frames to amortize initialization. The selected mode tunes EAN13/UPCA, Common1D and QR Code.

The supported public identifiers and native bits are:

| Identifier      |    Bit | Scope                                                            |
| --------------- | -----: | ---------------------------------------------------------------- |
| EAN13           |      1 | Pinned effort-mode scanner                                       |
| UPCA            |      2 | Zero-prefixed EAN13, returned as 12 digits when UPCA is selected |
| EAN8            |      4 | Experimental retail reader                                       |
| UPCE            |      8 | Experimental retail reader                                       |
| Code128         |     16 | Experimental, including GS1 metadata                             |
| Code39          |     32 | Standard characters; no automatic full-ASCII expansion           |
| ITF             |     64 | Experimental                                                     |
| Codabar         |    128 | Experimental                                                     |
| Code93          |    256 | Experimental                                                     |
| QRCode          |    512 | Model 2; no Micro QR, Model 1 or rMQR                            |
| DataMatrix      |   1024 | ECC200 and DMRE                                                  |
| PDF417          |   2048 | Normal/Compact; no Micro PDF417 or Macro/structured append       |
| Aztec           |   4096 | Compact/full and Rune                                            |
| DataBar         |   8192 | Omnidirectional, Stacked and Stacked Omnidirectional             |
| DataBarExpanded |  16384 | Expanded and Expanded Stacked                                    |
| MaxiCode        | 131072 | Modes 2–6; affine finder localization                            |

Effort levels select the following bounded searches:

| Reader               | Low | Medium | High | Very High |
| -------------------- | --- | ------ | ---- | --------- |
| Common1D             | 0   | 1      | 2    | 2         |
| QR Code              | 0   | 1      | 2    | 3         |
| Other matrix readers | 1   | 1      | 1    | 1         |

These are internal reader effort levels, not comparable work or confidence scores.
QR High adds threshold/sharpen recovery; Very High also tries bounded curved-grid
recovery for Model 2 version 2 and above. Packed RGBA QR-only inputs use direct
WASM upload with the same grayscale conversion and ignored alpha.
`unfinished` reports exhausted limits; no public continuation option is available.
EAN13-only scanning does not invoke them. When EAN13
and UPCA are both enabled, zero-prefixed EAN13 is returned as UPCA.

Polygons are in input-image coordinates. Metadata such as GS1, reader
initialization and structured append is available directly on Python/JavaScript
barcodes and retained in raw JSON. Structured-append
indices are one-based; symbols are not automatically assembled across frames.
Initialization payloads are data and never executed as configuration.
Non-character/general-purpose ECI is not interpreted as text. Source images
with multiple symbols remain multiple results; single-result selection happens
after scanning and ranks by support.

Undecoded localization for Code39, ITF, Codabar and failed DataBar payloads is
incomplete. Candidate, retry and parsing caps are reflected in `unfinished`;
a bounded search may still return valid reads. Fixed sampling strategies and
unsupported format variants are not completeness guarantees. Scores are not calibrated probabilities. These limitations are
inherited from the promoted experiment, not release performance guarantees.

Python and JavaScript optionally expose two- and five-digit EAN/UPC supplements
through the creation policy `Ignore` (default), `Read` or `Require`. Supplement
reading remains experimental; EAN8 supplements are a nonstandard extension.
The experimental localized-linear strategy remains internal; public scanning
uses the full-frame strategy.

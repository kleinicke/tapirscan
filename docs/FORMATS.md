# Format coverage

EAN13 is the default. Additional formats are explicitly selected at scanner
creation in JavaScript and Python; Python also allows per-scan overrides.
Rust and the native ABI select formats per scan. Python and JavaScript accept
`"1D"`, `"2D"` and `"all"` presets for their supported formats, or explicit lists.
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

The extra readers run at their research scanline effort 1, independent of the
four EAN13 effort levels. EAN13-only scanning does not invoke them. When EAN13
and UPCA are both enabled, zero-prefixed EAN13 is returned as UPCA.

Polygons are in input-image coordinates. Metadata such as GS1, reader
initialization and structured append is retained in JSON. Structured-append
indices are one-based; symbols are not automatically assembled across frames.
Initialization payloads are data and never executed as configuration.
Non-character/general-purpose ECI is not interpreted as text. Source images
with multiple symbols remain multiple results; single-result selection happens
after scanning and ranks by support.

Undecoded localization for Code39, ITF, Codabar and failed DataBar payloads is
incomplete. Several additional-reader internal caps are not yet reflected in
`unfinished`. Scores are not calibrated probabilities. These limitations are
inherited from the promoted experiment, not release performance guarantees.

The research module also supports optional EAN supplements and an experimental
localized-linear strategy. The initial main release API exposes format selection
with the default Ignore-supplements/full-frame strategy; it does not silently
select either experimental option.

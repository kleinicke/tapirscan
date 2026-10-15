# Format coverage

Tapirscan supports the linear and 2D formats listed below. The retail group
(EAN13, UPCA, EAN8 and UPCE) is the default selection. A scanner's formats are
chosen at creation, and every binding can override them for a single call
without changing the scanner. Python and JavaScript accept the presets below as
strings, a single identifier, or explicit lists; native bindings use the
constants listed here.

| Preset (Python, JS) | Formats                              |
| ------------------- | ------------------------------------ |
| `"retail"`          | EAN13, UPCA, EAN8, UPCE              |
| `"common1D"`        | Retail + Code128, Code39, ITF        |
| `"common"`          | common1D + QRCode, DataMatrix        |
| `"1D"`              | All linear formats except opt-in     |
| `"2D"`              | All supported matrix/stacked formats |
| `"all"`             | All formats except opt-in            |

The opt-in formats `Code32`, `PZN` and `MSIPlessey` are in no preset, `all`
included; select them by name.

Native names follow each language: Rust `Formats::COMMON_1D`, C
`TAPIRSCAN_FORMATS_COMMON_1D`, C++ `Formats::common_1d()` and Java
`Format.COMMON_1D`, with the same pattern for the other presets.

Retail covers the EAN/UPC family, not every format used in retail (for example,
DataBar needs an explicit selection or `"1D"`). Common is a convenience
selection, not a coverage or accuracy guarantee.

EAN13 and UPCA share the primary scan; selecting UPCA adds output normalization,
not another image scan. Enabling more formats can add reader work. JavaScript
loads one complete WASM scanner for the selected effort mode, not a separate
module per format. Reuse a scanner across frames to amortize initialization.
The selected mode tunes EAN13/UPCA, common1D and QR Code.

The supported public identifiers and native bits are:

| Identifier      |     Bit | Scope                                                            |
| --------------- | ------: | ---------------------------------------------------------------- |
| EAN13           |       1 | Effort-mode scanner                                              |
| UPCA            |       2 | Zero-prefixed EAN13, returned as 12 digits when UPCA is selected |
| EAN8            |       4 | Supported retail reader                                          |
| UPCE            |       8 | Supported retail reader                                          |
| Code128         |      16 | Supported, including GS1 metadata                                |
| Code39          |      32 | Standard characters; no automatic full-ASCII expansion           |
| ITF             |      64 | Supported                                                        |
| Codabar         |     128 | Supported                                                        |
| Code93          |     256 | Supported                                                        |
| QRCode          |     512 | Model 2; no Micro QR, Model 1 or rMQR                            |
| DataMatrix      |    1024 | ECC200 and DMRE                                                  |
| PDF417          |    2048 | Normal/Compact; no Micro PDF417 or Macro/structured append       |
| Aztec           |    4096 | Compact/full and Rune                                            |
| DataBar         |    8192 | Omnidirectional, Stacked and Stacked Omnidirectional             |
| DataBarExpanded |   16384 | Expanded and Expanded Stacked                                    |
| MaxiCode        |  131072 | Modes 2–6; affine finder localization                            |
| Code32          |  262144 | Opt-in Code 39 variant: Italian pharmacy code, 9-digit number    |
| PZN             |  524288 | Opt-in Code 39 variant: German PZN8/PZN7, number without `-`     |
| MSIPlessey      | 1048576 | Opt-in; mod-10 check digit required, returned with the data      |

### Pharmacy numbers in Code 39 (Code32, PZN)

Italian Code 32 and German PZN codes are ordinary Code 39 symbols; `Code39`
reads them as raw content (`0CSSBD`, `-02532876`). Select `Code32` and/or `PZN`
to get the pharmacy number instead: `012345676` (the printed `A` prefix is not
part of it) and `02532876`. Only reads with a valid check digit are converted;
the raw Code 39 text stays in the payload bytes. Other Code 39 reads are returned
only if `Code39` is selected too.

Both formats are opt-in, because an arbitrary six-character Code 39 passes the
Code 32 check about one time in ten: select them only where these codes are
expected.

### MSI Plessey

MSI Plessey has no per-character check, so it is read only when it ends in a
mod-10 (Luhn) check digit, which is returned with the data: `1234566` for
`123456`. Mod-1010 symbols read the same way; symbols with only a mod-11 check
or none are not read. The symbol also needs:

- quiet zones of at least eight modules on both sides, so a symbol cut by the
  image edge is not read;
- at least four digits, including the check digit;
- consistent bar widths across the symbol.

`MSIPlessey` is opt-in.

## Effort levels

Effort levels select the following bounded searches:

| Reader        | Low | Medium | High | Very High |
| ------------- | --- | ------ | ---- | --------- |
| Common-linear | 0   | 1      | 2    | 2         |
| QR Code       | 0   | 1      | 2    | 3         |

Data Matrix and Aztec get an additional grid-recovery pass for localized symbols
from Medium up, and Aztec uses smaller refinement budgets in Low. Data Matrix
frames without a read are also retried on reduced copies in High (two scales) and
Very High (four), which reads large blurred close-ups. PDF417 uses
the same effort in every mode; MaxiCode reads the same way in every mode, and from
Medium up retries large unread frames (over 1920 pixels) on reduced copies.

On frames whose long side exceeds 1536 pixels, High also localizes the non-retail
linear formats on a half-scale raster and Very High on overlapping 1536-pixel
windows, so small symbols in large photos can be found; Medium and Low use the
768-pixel localization raster only. For small codes in large photos with Medium, crop
around them.

These are internal reader effort levels, not comparable work or confidence scores.
QR recovery grows with the mode. Every mode retries tiny unread images (up to 300
pixels) on an enlarged copy. Medium adds, for grids that fail to decode, a gray
resampling against each module's neighborhood, a block-threshold pass where a QR
candidate stayed unread, and retries of large frames at 960 and 1280 pixels. High
adds threshold/sharpen recovery, block thresholds on every unread frame and
Reed-Solomon erasures for uncertain modules; Very High also tries bounded
curved-grid recovery for Model 2 version 2 and above. Packed RGBA QR-only inputs use direct
WASM upload with the same grayscale conversion and ignored alpha.
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
incomplete. Candidate, retry and parsing caps bound the search,
which may still return valid reads. Fixed sampling strategies and
unsupported format variants are not completeness guarantees. Scores are not calibrated probabilities.

Every binding optionally exposes two- and five-digit EAN/UPC supplements
through the creation policy `ignore` (default), `read` or `require` (`Ignore`, `Read`, `Require` in native bindings). EAN8
supplements are a nonstandard extension.
The localized-linear strategy is internal; public scanning
uses the full-frame strategy.

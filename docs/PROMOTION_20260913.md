# Tapirscan promotion — 2026-09-13

This promotion selects the experimental workbench's current 2026-09-13 effort
configuration. `provenance/import.json` records the source revision and every
imported file hash; `provenance/modes.json` records exact recipes, frozen host
selection, fit/proposal limits and expected EAN13 WASM SHA-256 values.

| Public mode      | Exact recipe                       |
| ---------------- | ---------------------------------- |
| low              | nano-lint-20260913                 |
| medium (default) | very-fast-v2-lint-20260913         |
| high             | guarded-quality-lint-20260913      |
| very-high        | high-effort-transfer-lint-20260913 |

All four rebuilt EAN13 WASM assets match the selected research hashes exactly.
The very-high facade preserves the secondary localization grid and 63-proposal
host budget. The other modes preserve their 32-proposal budget; refinement limits
are 0 for low and 8 for the other modes.

The additional readers are imported unchanged from `rust/multiformat`, with
source hashes matching the research artifact manifest. Their relocated WASM
build has SHA-256 `cfa674daac1a324730257e5e4712c08e8418db52aef6c50003d3cbd69fcd7725`,
while the research artifact has
`d4eed7394fb1d73639446f6fbaf3bfed0f08271724aae3c6132807d72a208fbf`.
Byte identity is not claimed for this relocated build. Direct execution of both
modules produced identical complete JSON on five independently encoded fixtures
(EAN8, Code128, QRCode, DataMatrix and Aztec). The release build records its own
hash, compiler and flags in generated `wasm/multiformat.json`.

The safe native facade needs the existing secondary localization helper exposed
across a crate boundary. `prepare_native_source` copies verified prepared source
into `native-core` and changes only `pub(crate) fn detect_secondary` to `pub fn`.
It records before/after hashes in `native-adapter.json`. Imported source and WASM
recipes are not modified by this adapter. Native builds use unwind to contain
panics at the C boundary; algorithms and host budgets are unchanged.

Public names become `tapirscan`, C++ namespace/include `tapirscan`, and Java
`org.tapirscan.Tapirscan`. The experimental workbench and demo branding use Tapir
Scan. Historical experiment identifiers, directories and frozen internal crate
names remain intact so provenance stays traceable. No registry is published.

ABI 3 replaces the fixed EAN-only string with length-delimited UTF-8 access and a
format name, and adds explicit format selection. JSON schema stays 2. Native JSON
parsing enables float_roundtrip to preserve source-coordinate f64 values exactly.
Single-result selection still follows full scanning and stable support ranking;
spatial reconciliation retains distinct physical copies of identical values.

Validation covers four-mode core/facade/native tests, maintained-code formatting,
Clippy, TypeScript, Python lint/types, C/C++/Java checks, synthetic cross-language
EAN13 and extra-format comparisons (including long Unicode payloads), installed
Python wheel/JAR execution, and relocatable CMake consumers. The public demo includes three owner-authorized, sanitized research photographs
copied byte-for-byte, plus a generated synthetic example. These are excluded from
language distributions. The demo has a clean
Svelte/TypeScript check and production build. Its built workers decode a synthetic
EAN13 in all four Tapir modes plus ZXing-WASM 3.1.1 and ZBar-WASM 0.11.0; these
comparison dependencies are demo-only and never used as library fallbacks. Package contents are inspected to
exclude the demo and research data. Local platform: macOS arm64; remote CI remains
pending. This is not a full dataset evaluation or browser camera field test.

Four effort presets apply to EAN13/UPCA. Additional readers use the experiment's
scanline effort 1 in every mode; supplements and localized-linear research options
are not enabled implicitly. See [coverage and remaining limitations](FORMATS.md).

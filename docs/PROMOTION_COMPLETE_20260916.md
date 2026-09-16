# Retail and candidate-continuation promotion — 16 September 2026

The selected 1.1.0 recipes import retail grayscale/template reuse, coverage
arithmetic improvements, linear duplicate consolidation and optional completion
of EAN13/UPC-A candidate work. Source files and experiment recipes are identified
by SHA-256 in [the promotion manifest](../provenance/promotion-complete-20260916.json).
The research checkout included uncommitted changes; its Git revision alone is
not a reproducible identifier.

## Release integration

- All four modes use new immutable tags and reproducible recipe/binary hashes in
  [modes.json](../provenance/modes.json).
- Release source-detail recovery, retry masks, fit limits, direction budgets and
  source-image geometry are retained. The primary and recovery hosts both forward
  continuation; their exact bytes are pinned.
- The public per-scan names are `finish_candidates` in Python/Rust and
  `finishCandidates` in JavaScript. Python and JavaScript research adapters use the
  same names. Default is false. See [the API contract](API_DESIGN.md#candidate-continuation).
- Native ABI 4 advertises the additive feature through `barcode_capabilities()`.
  Native public flag 16 and region-WASM flag 32 belong to separate interfaces.
- Supplemental kernels and crop capability are imported. The release retains its
  scanline scheduling by default; it does not enable the research host's mixed
  full-frame/crop scheduling or the rejected experimental coverage bridge.
- Duplicate consolidation uses source-pixel evidence in both Rust and JS. A
  payload match alone cannot join separated products or different supplements.

## Validation

- Four selected native/WASM builds reproduce the pinned binary hashes; selected
  core tests and additional-reader tests pass.
- Strict Clippy passes for the base core, selected recipes, additional readers,
  and all four prepared Rust/C facades. A subsequent isolated facade gate could
  not prepare another checkout because free space fell below its 10 GiB reserve;
  the same strict Clippy flags were rerun successfully against the verified
  prepared builds. The reserve was not changed.
- Formatting, provenance, JS/TS, Python lint/types, native compiler warnings,
  Svelte diagnostics and quality-tool regression checks pass.
- Python image tests: 25 cases, two environment-dependent skips. JS: 28 tests,
  including a 64-candidate budget test and rotated/padded duplicate fixtures.
  Native duplicate tests verify continuous bands, separators and supplements.
- Cross-language bindings, all four modes, and native/WASM continuation parity
  pass. Small/rotated source-detail fixtures and additional-format tests pass.
- Supplement validation passes 240 mode/policy cases, including distinct
  supplements on identical main payloads, with diagnostics on and off.
- Installed Python wheel and npm Node/browser/Worker checks pass. The wheel smoke
  enables continuation in every mode. The production demo Worker tests cover
  continuation, mode/format switching and comparison readers under a subpath.

These checks establish integration and tested behavior, not a new speed or recall
claim. No private datasets, labels or model weights are included.

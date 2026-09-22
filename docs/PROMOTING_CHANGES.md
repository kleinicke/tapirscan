# Integrating scanner experiments

Start from a committed Tapirscan baseline in an isolated worktree. Keep the
experiment manifest, input identities, commands and compact evidence in the
experiment workspace; keep original images in the dataset workspace.

1. Capture baseline results before editing. Change `core/src` directly; it is
   the production algorithm for all four modes. Use the stage boundaries in
   [the core guide](../core/README.md). Keep behavior changes distinct from
   structural refactors, and preserve all-symbol scanning, work limits,
   undecoded coverage, source geometry and support-based ordering.
2. Run selected core tests, native and WASM builds, the public Rust tests and
   relevant binding/installation checks. Compare the original and candidate on
   the same inputs, formats, effort, supplement policy and budget settings.
   Measure performance in paired, quiet runs after compilation finishes.
3. Review the ordinary source diff. Record a new production source revision in
   provenance, select it through `provenance/modes.json`, and assign new immutable
   WASM filenames when bytes change. `scripts/build_wasm.py --record` records the
   complete four-mode source and binary identity. Do not repoint old tags or
   change historical checksums to conceal drift. Keep package versions aligned
   with the release being prepared and document changes in release notes.
4. Run `scripts/verify_import.py` to check both frozen history and the selected
   release snapshot. Integrate the validated commit once, then rebuild through
   the normal scripts in the canonical checkout. Do not copy generated binaries
   or generated Rust modules manually between checkouts.

Development builds intentionally use `verify_import.py --historical-only` so
ordinary source edits can be compiled before recording a new release snapshot.
This still verifies imported decoders and the entire frozen core archive.

For an older research result, first reproduce its exact recipe from
`historical/core` (or the research archive) and retain its hashes and evidence.
Port the necessary algorithm change into maintained `core/src`; do not add a
production patch or copy a second scanner implementation. Preserve the public
facade and cross-language ownership/error contracts.

Publishing packages, a demo or a website remains a separate operation.

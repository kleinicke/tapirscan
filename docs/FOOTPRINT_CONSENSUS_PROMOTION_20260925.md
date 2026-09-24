# Selected barcode outline version — 2026-09-25

The user selected the validated profile-consensus improvement for production.
Implementation commit `9a093db` on main integrates experiment `c017680`.
The selected local release remains 1.2.2, with immutable assets named
`<mode>-footprint-consensus-20260925.wasm`. The demo default is
`1.2.2+consensus.20260925` (1.2.2 · improved barcode outlines).

The fallback adjusts display geometry after strict physical ownership decisions.
It does not add duplicate suppression or change decoding values. Experimental
validation is documented in [the experiment report](FOOTPRINT_CONSENSUS_EXPERIMENT_20260924.md).

Canonical all-mode native and WASM builds passed. JavaScript tests, demo tests,
Svelte checks, source provenance, release metadata and package input checks passed.
Local desktop/mobile browser checks verified real scans, all six versions,
version-specific asset requests and preservation of independent reader results.

The same browser checks passed on the live website, and all four deployed WASM
SHA-256 hashes match the selected provenance.

Netlify deployment: `6ab5a08511a68f2169ee4068`.
The image benchmark remains disabled and Turbo keeps its independent pinned build.
No Git tag or language-package publication was made. Existing release/demo working
changes are preserved; this note does not claim the complete working tree is clean.

Build and deployment evidence is retained in the sibling experiment workspace at
`retained/footprint-consensus-promotion-20260925/`.

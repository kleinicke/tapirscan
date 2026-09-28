# Integrating selected scanner changes

Research is conducted in isolated worktrees of a committed Tapirscan baseline.
Keep manifests, commands, alternatives, reports and compact evidence in the
separate experiment workspace. Keep dataset identities and observations in the
dataset workspace. See [repository boundaries](RESEARCH_BOUNDARY.md).

1. Capture baseline results before editing. Modify maintained `core/src` and the
   shared Rust facade; preserve all-symbol scanning, explicit work limits,
   source geometry, physical-instance identity and ranking.
2. Validate the candidate against the baseline on identical inputs, formats,
   effort and budget settings. Check affected modes, combined format presets,
   negatives, duplicates and native/WASM parity. Measure timing separately from
   builds. Keep failed alternatives and revisit conditions in the experiment record.
3. Prepare a production diff containing only selected implementation, necessary
   regression tests, current documentation and provenance. Preserve intentional
   Turbo variants. Do not merge unused prototypes, exploratory adapters, dated
   research reports or alternative implementations. Normal builds must not depend
   on an experiment checkout or historical scanner tree.
4. Format and run relevant core, public Rust, native/WASM and binding checks.
   Record a new runtime source snapshot and immutable WASM identities; select
   them in `provenance/modes.json`. Never overwrite an older artifact identity.
   Development builds use `verify_import.py --imports-only`; release checks also
   verify the selected runtime. Frozen decoder hashes remain enforced.
5. Integrate the validated commit once and rebuild in the canonical checkout
   through normal build commands. Retain the experiment-to-production commit link.
   Update release notes to describe supported behavior, not research chronology.

Publishing packages, a demo or a website is a separate operation.

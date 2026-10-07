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
   regression tests and current documentation. Preserve intentional
   Turbo variants. Do not merge unused prototypes, exploratory adapters, dated
   research reports or alternative implementations. Normal builds must not depend
   on an experiment checkout.
4. Format and run relevant core, public Rust, native/WASM and binding checks.
   Build the WASM files from source with `python3 scripts/build_wasm.py`; the
   build is identified by the package version, the git commit and the source
   digest in `bindings/javascript/wasm/build.json`. Time benchmarks on these
   builds (installed Chrome, JavaScript API, paired with the baseline); during
   iteration a small fixed subset of only the changed modes is enough.
5. Integrate the validated commit once and rebuild in the canonical checkout
   through normal build commands. The demo's `-next` readers show the current
   build without further steps. Keep research chronology out of the repository:
   release notes and documentation describe current behavior. A change reaches
   users through the next package version; see [releasing](RELEASING.md).

Publishing packages, a demo or a website is a separate operation.

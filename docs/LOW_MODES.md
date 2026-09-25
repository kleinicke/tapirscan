# Low and Low Classic

Public `low` now uses the original Turbo implementation, displayed as **TS-Low**
in the demo. This is an implementation change to the existing mode, shared by
JavaScript/WASM, Rust and native C/C++/Python/Java bindings. No new public mode
name is added; `medium` remains the default.

**TS-Low Classic** names the previous Low implementation, including its later
improvements. It remains an optional, fixed-build demo comparison. It is not a
public `low-classic` mode. The demo pins the improved consensus build for Classic;
changing the public version selector changes TS-Low, Medium, High and Very High,
not that historical comparison. Optional Turbo2/4/8/16 remain private experiments.

Low emphasizes bounded, rotation-aware decoding with all supported format masks.
It can miss difficult codes that higher effort modes find, and fast linear scans
report `unfinished: true`. Source coordinates, multiple physical symbols and
undecoded regions remain supported. Supplement policies other than Ignore and
extended linear budgets retain the original pipeline. Medium/High/Very High keep
their existing policies and recovery engines.

Rebuilding a demo-only Classic artifact from this source requires
`TAPIRSCAN_LOW_CLASSIC=1 python3 scripts/build_wasm.py --development low`.
Ordinary release builds reject that override. Keep previous version artifacts
immutable; packages already published with the old Low implementation are not
changed by this source update.

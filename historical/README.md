# Frozen scanner history

These are the original core sources, mode patches, manifests, and selected-mode
build scripts at `bad9a7a19fcc2812d0154124bc0e983d389c5406`. They are retained for
reproduction, not production development. Do not format or edit them.

The `multiformat` and `adapters` links resolve the same hash-pinned dependencies
as the original build. Each original recipe still checks its base, patch, target
and external hashes. The maintained wrapper `scripts/build.py --historical`
places outputs under `build/history` and delegates to the original build script.

New experiments edit `core/src` and use the normal public Rust/native/WASM build.

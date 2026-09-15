# Release build portability, 2026-09-15

The first GitHub builds exposed build/validation issues. The scanner
algorithms, imported recipes, and public API are unchanged.

- `multiformat/Cargo.toml` had acquired release license/author fields after its
  import was pinned. Removing exactly those two lines reproduces the recorded
  SHA-256. The repository and distributed packages retain their MIT license.
- Native primary and Low recovery crates exported identical unmangled WASM ABI
  symbols. Linux and Windows linkers rejected them. The native-only recovery
  adapter now leaves those symbols Rust-mangled; recovery is invoked through
  Rust. Pinned source snapshots are unchanged. Windows and both macOS targets
  passed their four-mode installed-wheel tests after this change.
- Linux's auditwheel adds a ZIP directory entry under `_native/`. The installed
  wheel test now counts files rather than treating that entry as a fifth library.

The original WASMs embedded local standard-library paths when `rust-src` was
installed. The same Rust 1.91.1 build on GitHub, without `rust-src`, used canonical
`/rustc/<compiler-commit>/library` paths. Both GitHub hosts produced Low hash
`0a4b80e381ab2f762f6c5b5cb7591584c63646224bd33c17b272f00952a03ff6`, while the
original was `4abf45ddbfa695d6cc1f497e90c3dc3f691a7354d1f6e327d8afdd463ad8f30f`.
WASM inspection found identical function type, function, table, memory, export,
element, name, producer, and target-feature sections. Data paths, addresses and
code references differed. Applying an explicit standard-library path remap on
the local compiler reproduced GitHub's Low binary exactly.

The release builder now applies that remap to each mode and the additional-format
module. It still verifies original base/patch/target hashes, but checks the
resulting release binary against `provenance/modes.json`. The original imported
recipe hashes remain intact. New immutable artifact names are
`low-release-20260915`, `medium-release-20260915`, `high-release-20260915`, and
`very-high-release-20260915`; `originalBinarySha256` records their predecessors.
The provenance manifest's entry for the changed mode metadata is updated with
maintainer authorization. This does not waive source or binary hash verification.

The demo now stages only the selected WASM modes and additional-format module;
it no longer requires unused historical binaries on a clean checkout.

Validation for the completed release must include GitHub source/hash checks,
all native platform wheel scans, cross-language tests, npm tests and demo tests.
The release workflow rejects unsuccessful or different-commit builds and requires
all five supported wheels plus the matching npm tarball before publication.

The Linux Very high core test compared `hypot` and a fast square-root weight
with bitwise equality. It reported 131.73458164050928 versus 131.73458164050925.
The release test adapter keeps the accept/reject decision exact and allows only
`2 * f64::EPSILON * max(abs(expected), 1)` weight rounding. It runs in a separate
`test-core` copy and records before/after hashes. WASM compilation still uses the
unmodified, hash-verified `temporarysource` files. No production math changed.

On the standard macOS GitHub runner, PyTorch advertises MPS but cannot allocate
the test tensor. The MPS test now reports a hardware skip if fixture allocation
fails, before invoking Tapirscan; scanner failures are not caught or skipped.
The MPS test remains enabled on usable local hardware.

# Imported scanner core

Exact source files required by the pinned Fast and Quality manifests are preserved
here. The latest selected artifacts require the patches in `experiments/`.
Use the repository-root `scripts/build.py` rather than building the base aliases
and assuming they match the current demo. See `provenance/import.json` for hashes.

Inactive research modules remain because they are part of the pinned compilable
crate. No neural model or runtime is included, and the selected modes are classical.
Do not prune source or rewrite the package metadata until replacement builds have
been compared against these exact baselines.

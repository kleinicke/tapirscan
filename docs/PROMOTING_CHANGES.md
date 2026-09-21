# Promoting workbench improvements

New experiments start from a committed Tapirscan baseline in an isolated Git
worktree. Keep the experiment manifest, regression harness, dataset identities,
commands and compact results in the experiment workspace; keep original images
in the dataset workspace. The original workbench is a historical reference,
not the destination for new scanner development.

For each promotion, identify the candidate revision and exact mode recipe. Import
only required project-owned source, tests and relevant aggregate evidence. Recompute
`provenance/import.json` and mode manifests; retain the previous release's provenance
in Git. Never import private datasets, labels, model weights or reference binaries.

Use a release-repository branch and review its diff. Run native tests, WASM hash or
behavior checks, binding parity and the agreed benchmark contract. Distinguish a
compiler/toolchain byte change from a behavioral change. Pin new modes explicitly;
do not reuse an immutable version tag for different bytes or policy settings.
Update release notes and package versions together. Once package APIs diverge from
the workbench, port the change rather than overwriting the release tree wholesale.

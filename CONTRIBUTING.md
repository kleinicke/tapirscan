# Contributing to Tapirscan

Useful contributions include reproducible failures, documentation improvements,
platform testing, and focused fixes to the public bindings.

## Report a problem

Include the Tapirscan version, effort mode, selected formats, input dimensions,
and OS/browser/Python version. Describe the expected value and actual result.
For camera problems, also include the camera mode and delivered resolution.

A minimal image or code example is ideal. Share only images you have permission
to publish, and remove personal or confidential information. Keep correctness,
geometry, and timing issues distinct: a narrow ZBar overlay is not necessarily
a failed decode, for example.

## Make a change

1. Follow [the development guide](docs/DEVELOPMENT.md).
2. Keep changes focused and explain the behavior being improved.
3. Run `node tools/quality/install.mjs` once to install the formatting hook, then format
   maintained files with `node tools/quality/cli.mjs format`.
4. Run the tests relevant to the change. Native ABI changes require binding and
   installation tests; algorithm promotions require parity and paired timing checks.
5. Describe validation and remaining limitations in the pull request.

Never add a reference-decoder fallback under the Tapirscan result label. Scanner
changes in `core/` and `multiformat/` follow [Changing the scanner](#changing-the-scanner).

Keep image datasets, generated engines, native binaries and model weights out of
source control. Small procedural fixtures are welcome. Performance claims need
paired measurements under [the benchmark protocol](docs/BENCHMARKS.md).

The public package API should stay small. Please discuss substantial APIs,
format commitments, or distribution changes before building a large patch.

## API stability

The binding guides and [API design](docs/API_DESIGN.md) describe the public API.
See [upgrading from earlier versions](docs/API_MIGRATION.md) when moving between releases.

Apart from the experimental APIs below, documented
functions, defaults, result fields, identifiers and ownership/error contracts
stay compatible within a major version. Minor releases add compatible
capabilities; patch releases fix bugs.

Decoder improvements can change reads, geometry, ordering and runtime on a given
image. Those outputs are not bit-for-bit compatibility promises. Supported format variants and limitations are documented in [format coverage](docs/FORMATS.md).
Experimental APIs, currently the JavaScript `experimentalTurbo` option, may change or be removed in minor releases.

Undocumented internal counters, private modules and generated build paths are not
public interfaces. The C ABI is version 6; Rust binary ABI stability is not
promised across compiler versions.

Keep the public interface small. Validate API changes through consumers, packaging
and cross-language parity tests.

## Changing the scanner

- Experiments, exploratory adapters, failed variants and dated reports live in a
  separate experiment workspace; dataset identities and observations live in the
  dataset workspace. `scripts/check_repository_boundary.py` rejects research
  directories, known unused prototypes and dated reports in this repository.
- Run experiments in isolated worktrees of a committed baseline, editing the real
  `core/src` (or `multiformat/`) rather than a second implementation.
- Integrate a change once, as a diff of only the selected implementation, its
  regression tests and current documentation. Include paired evidence against
  the baseline: identical inputs, formats, modes and budgets, native/WASM parity
  and separately measured timing ([benchmark protocol](docs/BENCHMARKS.md),
  [comparison command](docs/COMPARING_SCANNERS.md)).
- Normal builds must not depend on an experiment checkout. Keep the four
  Turbo presets building and tested.
- Size the Chrome timing to the step: quick (about 100 images per set, one
  repeat, changed modes) while iterating; merge (about 300 images per set, one
  repeat, changed modes) before integrating; full only for package releases.
  Recorded numbers follow the [benchmark runtime requirements](docs/BENCHMARKS.md).

## Contribution license

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Tapirscan by you, as defined in the Apache-2.0 license, shall be
dual licensed as MIT OR Apache-2.0, without any additional terms or conditions.
Third-party components retain their existing licenses and notices.

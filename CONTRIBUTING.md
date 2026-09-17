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
3. Format maintained files with `node tools/quality/cli.mjs format`.
4. Run the tests relevant to the change. Native ABI changes require binding and
   installation tests; algorithm promotions require recipe and parity checks.
5. Describe validation and remaining limitations in the pull request.

`core/`, imported hosts, and provenance are reproducible snapshots. Changes to
these follow [the promotion process](docs/PROMOTING_CHANGES.md), rather than ad hoc
edits. Never add a reference-decoder fallback under the Tapirscan result label.

Keep image datasets, generated engines, native binaries and model weights out of
source control. Small procedural fixtures are welcome. Performance claims need
paired measurements under [the benchmark protocol](docs/BENCHMARKS.md).

The public package API should stay small. Please discuss substantial new APIs,
new format commitments, or distribution changes before building a large patch.

## API stability

The binding guides and [API design](docs/API_DESIGN.md) describe the
1.2.0 API revision prepared for release. See [migration](docs/API_MIGRATION.md).
The early-library 1.2.0 release explicitly makes a one-time exception by including
breaking changes in a minor version. From 1.2.0 onward, documented functions, defaults, result fields, identifiers and
ownership/error contracts stay compatible within a major version. Minor versions
may add compatible capabilities; patch versions fix bugs.

Decoder improvements can change reads, geometry, ordering and runtime on a given
image. Those outputs are not bit-for-bit compatibility promises. Experimental
format coverage describes decoding maturity, not permission to break the API.
Undocumented internal counters, private modules and generated build paths are not
public interfaces. The C ABI is version 4; Rust binary ABI stability is not
promised across compiler versions.

Keep the public interface small. Validate API changes through consumers, packaging
and cross-language parity tests.

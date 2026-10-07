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
   installation tests; algorithm promotions require parity and paired timing checks.
5. Describe validation and remaining limitations in the pull request.

Scanner changes in `core/` and `multiformat/` follow
[the promotion process](docs/PROMOTING_CHANGES.md). Never add a reference-decoder fallback under the Tapirscan result label.

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
The JavaScript `experimentalTurbo` option, its preset values, corresponding
`experimentalTurbo` scanner/result properties and `wasm/experimental-turbo*.wasm`
imports are explicitly exempt from minor-version compatibility: they may change
or be removed in a minor release, with changes recorded in release notes. Patch
releases retain interface compatibility. Pin an exact version when using them.
This exception does not cover stable effort modes or shared result fields.

Undocumented internal counters, private modules and generated build paths are not
public interfaces. The C ABI is version 6; Rust binary ABI stability is not
promised across compiler versions.

Keep the public interface small. Validate API changes through consumers, packaging
and cross-language parity tests.

## Contribution license

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Tapirscan by you, as defined in the Apache-2.0 license, shall be
dual licensed as MIT OR Apache-2.0, without any additional terms or conditions.
Third-party components retain their existing licenses and notices.

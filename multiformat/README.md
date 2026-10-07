# barcode-multiformat

The format readers behind Tapirscan: localization, sampling, error correction
and decoding for the linear and 2D formats beyond the core EAN/UPC scanner. It
links no third-party barcode decoder. The crate is internal (`publish = false`);
applications use it through the Tapirscan bindings.

Supported formats and variant limitations are listed in
[format coverage](../docs/FORMATS.md). Payloads are returned per symbol;
structured-append parts are reported with one-based indices but not assembled.
Reader initialization symbols are reported, never executed.

## Testing

From the repository root:

```sh
cargo test --manifest-path multiformat/Cargo.toml --offline
```

Reference encoders generate test images and supply attributed standard symbol
tables; runtime decoding and localization are project code. `serde` and
`serde_json` provide serialization and `encoding_rs` character conversion. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

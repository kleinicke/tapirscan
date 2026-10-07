# barcode-multiformat

Internal crate (`publish = false`) with the readers for the linear and 2D formats
beyond the core EAN/UPC scanner. Applications use it through the Tapirscan
bindings; supported formats and limits are in [format coverage](../docs/FORMATS.md).

Test it from the repository root:

```sh
cargo test --manifest-path multiformat/Cargo.toml --offline
```

Test-image encoders and attributed tables are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

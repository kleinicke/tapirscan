# Release notes

## 1.1.0 — unreleased

- One-shot `scan()` and reusable scanners return all decoded instances with
  immutable results, source-image geometry, support, timing and work-limit status.
- Python accepts Pillow images, NumPy arrays, PyTorch tensors and explicit pixel
  buffers. Float ranges and layouts are explicit; optional BGR input supports OpenCV.
- Format identifiers and presets select EAN/UPC, linear and matrix readers.
  Additional readers are experimental; effort modes control EAN13/UPCA search.
- Optional `Ignore`, `Read` and `Require` policies control EAN/UPC supplements.
  Supplement text is separate; polygons describe the main barcode.
- Results expose available payload bytes, GS1 and structured-append metadata.
  `.best` selects the highest numeric support, not cross-format confidence.
- Python `as_dict()` provides JSON-compatible public results. Optional diagnostics
  expose typed undecoded regions and reader-specific search evidence.
- JavaScript supports configurable WASM loading, immutable scanner configuration,
  synchronous reusable scans, and browser worker/camera examples.
- Native consumers use ABI 4, with version checks and descriptive errors.
- CI validates supplement association and geometry with a pinned test encoder,
  alongside cross-language and installation checks.

See the [Python](bindings/python/README.md), [JavaScript](bindings/javascript/README.md)
and [native binding](docs/NATIVE_BINDINGS.md) guides for the current API. Publication
steps are in [the release checklist](docs/RELEASING.md).

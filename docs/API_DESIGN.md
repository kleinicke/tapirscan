# Public API design

This page holds the semantic rules shared by every binding. The language guides
link here instead of repeating them. For moving from an earlier version see
[upgrading](API_MIGRATION.md); for field names per language see
[native bindings](NATIVE_BINDINGS.md#result-fields).

## Results

`scan` returns a lightweight `ScanResult`: decoded strings through `values` and
their locations through `barcodes`. Each barcode has a format, a source-image
polygon, `support` and optional payload metadata. Results are not iterable; use
`barcodes`. There is no public `debug` option.

Inspection is explicit and returns an `InspectionResult` with:

- `barcodes`: the same decoded instances as scanning with the same options.
- `undecoded`: localized regions without an accepted decode.
- Image size, selected mode and scan time.
- `diagnostics`: unstable engine evidence for advanced consumers. C and C++ expose it as JSON
  text, Rust as `serde_json::Value`.

In C, both operations return owned result handles. The count, barcode and copy
accessors work for either; summary and undecoded-region accessors need an
inspection result. JSON is serialized lazily: a barcode array for scans, a
schema-2 engine report for inspections.

### Semantic rules

- **Equal payloads stay separate.** Equal values at different locations are
  different barcodes.
- **Support is not confidence.** `support` is uncalibrated, reader-specific
  evidence. `best` returns the barcode with the largest support (first read wins
  ties). It is a convenience, not a most-reliable selection across formats or
  efforts, and it does not change scan work. Select by format, payload or
  position when that matters.
- **Undecoded regions are hints.** They can be false candidates, failed attempts
  or deferred work, and may overlap. An empty list does not guarantee that every
  barcode was found.
- **Geometry.** Polygons are in source-image pixels with a top-left origin, x
  rightward and y downward. `rect` encloses the polygon as integers: floor of the
  minimum to ceil of the maximum. C has no `rect`; compute bounds from the polygon.
- **Optional metadata.** GS1 and reader-initialization flags distinguish
  unavailable from false. Payload bytes are the original decoded bytes when the
  reader reports them, never a re-encoding of the text. Supplements stay separate
  from the main text and polygon.
- **No detection** is a successful empty result, never an error.
- **No deadline.** A call has no timeout and no retry-until-finished loop. Work
  is bounded by the effort mode.

## Images

Inputs are decoded pixels, not filenames, URLs or encoded images: gray8, RGB8 or
RGBA8, with alpha ignored. Limits, enforced by every binding:

- width and height at least 3×3, at most 32 megapixels;
- the addressed layout, `(height - 1) * stride + width * channels` bytes, fits
  in the supplied buffer and in 128 MiB (a larger backing buffer, such as a
  frame around a crop, is accepted);
- stride is at least `width * channels`; 0 means packed rows where a binding
  accepts it.

Decode image files and convert BGR, planar, float or 16-bit pixels before
scanning. Python also accepts Pillow, NumPy and tensor inputs, JavaScript
`ImageData` or byte buffers, and Rust the `image` crate buffers; their guides
list the conversions.

## Configuration

Defaults are Medium effort, retail formats and ignored supplements. Effort and
supplement policy are fixed when a scanner is created. A per-call `formats`
selection overrides the scanner's formats for that call only and never changes
the configuration, in every binding. Input validation and errors follow each
language's conventions.

## Ownership and lifetime

- Python snapshots input and releases scanners with a context manager or `close`.
- Rust borrows pixels synchronously and uses RAII.
- C++ and Java copy results into owned values; Java byte arrays are copied while
  native segments are read in place.
- C returns owned handles that the caller destroys.
- JavaScript initializes asynchronously, scans synchronously and releases WASM
  sessions with `dispose`. `tapirscan/browser` runs the same scanner in a
  bundled worker, accepts browser image sources (files, image, video and canvas
  elements, bitmaps) and returns promises.

Results outlive the scanner that produced them. See
[native bindings](NATIVE_BINDINGS.md) for the native ABI.

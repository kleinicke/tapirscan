# Adding a language binding

A binding wraps the shared C ABI; it does not port the decoder. Work through
this checklist.

1. **C ABI.** Bind every function in
   [`tapirscan.h`](../bindings/c/include/tapirscan.h), the signature source of
   truth, and check `tapirscan_abi_version()` against `TAPIRSCAN_ABI_VERSION`
   when loading the library.
2. **Formats.** Do not hand-write format bits or presets. Add an output target to
   `scripts/generate_formats.py` (it currently writes C, C++ and Java) so the
   constants are generated from `config/formats.json`.
3. **API shape.** Follow [API design](API_DESIGN.md) and the field names in
   [native bindings](NATIVE_BINDINGS.md#result-fields): `scan` and `inspect`,
   scanner options (mode, formats, supplement policy), per-call formats,
   barcodes with source-image polygons, `values`, and undecoded regions for
   inspection.
4. **Ownership and errors.** Copy variable-length fields with explicit lengths
   (embedded NUL bytes are valid), destroy every result and scanner exactly once
   including on error paths, map non-zero statuses to the language's error type
   while keeping the status code and the optional `tapirscan_error` message, and
   keep pixels alive for the duration of each call.
5. **Parity tests.** Add a `scan_raw` harness that prints the same JSON as
   `bindings/cpp/examples/scan_raw.cpp` and register it in
   `scripts/test_bindings.py`, plus checks for blank images, multiple symbols, padded strides, short buffers, invalid
   dimensions, use after close and concurrent calls. See also
   [validation](VALIDATION.md).
6. **Packaging.** Document how the native library is found and test installed
   packages on every advertised OS and architecture.

Keep user documentation in one language guide: quick usage first, reference
afterward.

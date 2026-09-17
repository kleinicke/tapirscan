# Adding a language binding

Anyone can ask an AI coding assistant to scaffold a Tapirscan binding with a
single prompt. The shared C ABI makes that practical: there is no need to port
the decoding algorithm. Generated code still needs compilation, ownership tests
and decoding checks before it is ready for users.

Start with this prompt, replacing the language and target platforms:

> Create a Tapirscan binding for LANGUAGE on TARGET PLATFORMS using
> bindings/c/include/tapirscan.h and docs/NATIVE_BINDINGS.md. Follow the current
> Java and C++ wrappers for ownership and UTF-8 handling. Provide a simple scan
> API following docs/API_DESIGN.md: decoded instances, undecoded proposals,
> source-image polygons and work-limit status, with optional diagnostics and
> format selection. Expose the format-independent extended-budget intent through
> the appropriate native controls; keep engine-specific budget details internal. Preserve native status codes, explicit buffer
> lengths and result cleanup on errors. Use the same native library for every
> operation on a handle. Include build instructions, one minimal example and
> tests against the existing cross-language fixtures. Do not claim it works until
> those tests pass. Do not publish packages automatically.

The [C header](../bindings/c/include/tapirscan.h) is the signature source of truth;
[the ABI contract](NATIVE_BINDINGS.md) explains layout and ownership. Use FFI to a
selected native library for native languages, or the [JavaScript package](../bindings/javascript/README.md)
for a JavaScript runtime. A Rust caller can use the [native Rust API](../bindings/rust/README.md).

Before shipping, check:

- Correct values and input-coordinate polygons against `scripts/test_bindings.py`
  and `scripts/test_multiformat.py`, including blank images and multiple symbols.
- UTF-8 length handling, including embedded NULs; no fixed-size payload truncation.
- Scanner/result cleanup, use after close, repeated close and concurrent calls.
- Invalid dimensions, padded row strides, short buffers and native error propagation.
- Installed-package loading on every advertised OS/architecture.

A wrapper can be short; distributing native libraries and validating platforms
usually takes more work than generating its first version. Keep user documentation
in one language-specific guide: quick usage first, full reference afterward.

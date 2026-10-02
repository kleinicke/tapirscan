# Raw WebAssembly adapter

This crate is generated per effort mode and delegates scanning to the public
`tapirscan::Scanner` API. It has no host imports. JavaScript owns timing and
copies pixels into the adapter's prepared buffer before each synchronous scan.

ABI 2 exports `tapirscan_abi_version`, `tapirscan_mode`, `tapirscan_create`,
`tapirscan_destroy`, `tapirscan_prepare`, `tapirscan_input_ptr`,
`tapirscan_input_len`, `tapirscan_scan`, `tapirscan_output_ptr`, and
`tapirscan_output_len`. Handles are checked registry IDs. A prepare allocates
exactly `(height - 1) * stride + width * channels` bytes; its input view remains
valid until the next prepare or destroy. Output is UTF-8 JSON and remains valid
until the next scan or destroy.

`tapirscan_create(mode, formats, addon)` returns zero for invalid options or a
mode that does not match the loaded artifact. Add-on values are 0 Ignore, 1 Read,
and 2 Require. `tapirscan_scan(handle, flags, formats)` uses flag bit 0 for an
extended budget and bit 1 for raw diagnostics. A zero scan format mask keeps the
creation default; a nonzero mask overrides it for that call. Status values match
the native boundary where applicable: 0 success, 1 invalid argument, 2 invalid
handle, 4 scanner/internal failure, and 5 capacity. Scan failures leave a JSON
error object in the output buffer.

Ordinary success JSON contains only `barcodes`. Barcode entries include their
computed `rect`; unavailable payload metadata is omitted. Inspection (flag bit 1)
adds `bestIndex`, `undecoded`, `image`, `mode`, `elapsedMs`, `unfinished` and the
raw engine value under the transport key `debug`. JavaScript exposes that evidence
as `diagnostics` on the inspection report. Its public `scan` returns the barcode
array directly. The JavaScript host also accepts historical ABI 1 assets, whose
ordinary wire results contain the report envelope.

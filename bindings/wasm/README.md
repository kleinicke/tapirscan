# Raw WebAssembly adapter

Most users should use the [JavaScript package](../javascript/README.md), which
wraps this adapter. This page is a reference for calling the WebAssembly module
directly. Each module is built for one effort mode and has no host imports. The
host copies pixels into the module's input buffer, then runs a synchronous scan.

## Exports

| Export                                                         | Purpose                                                                                       |
| -------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `tapirscan_abi_version()`                                      | Adapter ABI version (2)                                                                       |
| `tapirscan_mode()`                                             | Compiled effort: 0 Low, 1 Medium, 2 High, 3 Very high                                         |
| `tapirscan_experimental_turbo()`                               | Turbo preset (2, 4, 8 or 16) compiled into a Low module, else 0                               |
| `tapirscan_create(mode, formats, addon)`                       | Returns a scanner handle, or 0 for invalid options or a `mode` that differs from the module's |
| `tapirscan_destroy(handle)`                                    | Releases the scanner                                                                          |
| `tapirscan_prepare(handle, width, height, channels, stride)`   | Describes the image and allocates its input buffer                                            |
| `tapirscan_input_ptr(handle)`, `tapirscan_input_len(handle)`   | Location and size of the input buffer                                                         |
| `tapirscan_scan(handle, flags, formats)`                       | Scans the prepared image                                                                      |
| `tapirscan_output_ptr(handle)`, `tapirscan_output_len(handle)` | Location and size of the UTF-8 JSON result                                                    |

`formats` is a format bit mask. `addon` is 0 Ignore, 1 Read or 2 Require. In
`tapirscan_scan`, flag bit 0 enables the extended budget and bit 1 selects
inspection; a nonzero `formats` overrides the creation formats for that call.

`tapirscan_prepare` allocates exactly `(height - 1) * stride + width * channels`
bytes. The input view stays valid until the next prepare or destroy, and the
output until the next scan or destroy. Handles are checked IDs.

## Results

`tapirscan_scan` returns 0 on success, 1 for an invalid argument, 2 for an
invalid handle, 4 for a scanner failure and 5 for exceeded capacity. On failure
the output is a JSON object with an `error` message.

An ordinary scan outputs `{"barcodes": [...]}`. Each entry includes its `rect`;
unavailable metadata is omitted. An inspection also contains `bestIndex`,
`undecoded`, `image`, `mode`, `elapsedMs` and the engine diagnostics under
`debug`.

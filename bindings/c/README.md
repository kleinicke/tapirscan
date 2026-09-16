# C native interface

`python3 scripts/build_native.py low medium high very-high` builds four independent shared libraries
in `build/native/`. All implement ABI version 4 from `include/tapirscan.h`.
The core source and WASM mode recipes remain unchanged. Native builds use unwinding
so Rust panics can be caught at the C boundary; invalid raw pointers and allocation
failure are outside that guarantee.

```c
#include <tapirscan.h>
tapirscan_handle scanner = 0;
barcode_result result = 0;
int status = tapirscan_create(&scanner);
if (status != BARCODE_OK) return status;
status = barcode_scan_with_options(scanner, pixels, length, width, height, 4, stride,
    0, &result);
if (status == BARCODE_OK) {
    barcode_result_metadata metadata;
    status = barcode_result_info(result, &metadata);
    /* Check status before using metadata.
       barcode_result_read gives typed decoded reads.
       barcode_result_copy_json gives all localization and candidate evidence. */
    barcode_result_destroy(result);
}
tapirscan_destroy(scanner);
```

Link the selected mode library; `barcode_mode()` returns 0 (low), 1 (medium), 2 (high), or 3 (very-high).
Use the same library for creation, scanning, result access and destruction. Handles
are checked registry IDs. Null outputs, short buffers, invalid dimensions/strides,
stale handles and exhausted handle capacity return explicit status codes. Valid
pointer ranges and their lengths remain the C caller's responsibility.

Input is borrowed during the synchronous call. Distinct scanners can run concurrently;
one scanner serializes scans. Destruction prevents subsequent operations, while an
already-started call may finish. Every successful result must be destroyed separately;
results remain valid after scanner destruction. JSON is copied into caller memory,
so no borrowed result pointer can escape. Capacity must include its NUL terminator.
The registry allows at most 1024 live scanners and 1024 live results per library.

The supported native target is currently 64-bit. macOS arm64 has been tested; Linux
has CI coverage configured but not run remotely yet. Windows build naming is included
but Windows/MSVC validation remains pending. See [the ABI contract](../../docs/NATIVE_BINDINGS.md) for schema.

`barcode_scan` uses the defaults: multiple results, no exposed region evidence.
`barcode_scan_with_options` adds flags: `BARCODE_SINGLE` returns at most one
highest-support decoded result after full scanning; `BARCODE_INCLUDE_REGIONS`
adds localization/search/candidate evidence to JSON. Combine flags with bitwise OR;
zero uses the defaults and unknown bits return `BARCODE_INVALID_ARGUMENT`.
The typed result count follows the selected mode; no-read results have count zero.
JSON schema 2 keeps completion flags even when region fields are omitted. ABI
version 4 requires supplement-policy support. Rebuild clients and native libraries
together. The wrappers check the required ABI during initialization.

`BARCODE_READ_EAN_ADDON` attempts a supplement; `BARCODE_REQUIRE_EAN_ADDON`
accepts retail reads only with a confirmed supplement. These flags are mutually
exclusive and leave other formats unaffected. Supplement text is in JSON
`eanAddOn`; geometry describes the main barcode.

## Functions and format selection

The example is a function-body fragment with decoded RGBA pixels, their byte
length, dimensions and stride supplied by the caller. It uses default result
options; it does not load an encoded image file.

| Function                                | Purpose                                                                |
| --------------------------------------- | ---------------------------------------------------------------------- |
| `barcode_abi_version`, `barcode_mode`   | Inspect ABI and selected mode.                                         |
| `tapirscan_create`, `tapirscan_destroy` | Allocate/free a scanner handle.                                        |
| `barcode_scan`                          | EAN13, multiple results, no diagnostics.                               |
| `barcode_scan_with_options`             | EAN13 with output flags.                                               |
| `barcode_scan_formats`                  | Output flags plus an explicit nonzero format mask.                     |
| `barcode_result_info`                   | Read count, JSON length and completion flags.                          |
| `barcode_result_read`                   | Read a zero-based barcode's polygon, support, UTF-8 length and format. |
| `barcode_result_copy_text`              | Copy decoded text into caller memory, including terminal NUL.          |
| `barcode_result_copy_json`              | Copy complete JSON into caller memory, including terminal NUL.         |
| `barcode_result_destroy`                | Free a result handle.                                                  |

All signatures, argument types and status constants are in the
[public header](include/tapirscan.h). `barcode_scan_formats` has the same arguments
as `barcode_scan_with_options`, with a format mask before the output result pointer.
For example, `1u | 16u` selects EAN13 and Code128. See [format bits](../../docs/FORMATS.md).
There are no string presets in C. Check every status before consuming its outputs;
text capacity must be `text_length + 1`, using the explicit length for embedded NULs.

For a runnable consumer and compile/link setup, use
[the C smoke example](tests/smoke.c) and the [CMake build](../cpp/README.md).
The mixed `tapirscan_*`/`barcode_*` names are the current ABI, not separate libraries.

## Finishing candidate work

Enable the `BARCODE_FINISH_CANDIDATES` scan flag to remove shared frame retry and association budgets
for EAN13/UPC-A candidates. Default is disabled; selected formats must include
EAN13 or UPCA. Per-candidate effort and other limits remain; unfinished work is
still reported. See [API design](../../docs/API_DESIGN.md) for scope and cost.

# Tapirscan for C

`tapirscan_scan` writes an owned result handle through `out` and returns a
`tapirscan_status` (`int32_t`). Read the barcode count with
`tapirscan_result_count`, then each barcode with `tapirscan_result_barcode` and
`tapirscan_result_copy`. Positions are in `polygon`. Destroy the result with
`tapirscan_result_destroy`.

Use `tapirscan_inspect` to also get unread regions, timing and diagnostics.
`tapirscan_result_info` and `tapirscan_result_undecoded` require an inspection
result and return `TAPIRSCAN_INVALID_ARGUMENT` for ordinary scan results.

Defaults are Medium effort and retail formats (EAN13, UPCA, EAN8 and UPCE). One
shared library contains all four effort modes.

```c
#include <stdio.h>
#include <stdlib.h>
#include <tapirscan.h>

/* Scan one RGBA frame with a scanner the caller created once and reuses. */
int32_t scan_rgba(tapirscan_scanner scanner, const uint8_t *pixels, uint64_t width,
                  uint64_t height) {
    tapirscan_image image = {pixels, width * height * 4, width, height, 4, 0};
    tapirscan_result result;
    int32_t status = tapirscan_scan(scanner, &image, NULL, &result, NULL);
    if (status != TAPIRSCAN_OK) {
        fprintf(stderr, "%s\n", tapirscan_status_message(status));
        return status;
    }
    uint64_t count = 0;
    tapirscan_result_count(result, &count);
    for (uint64_t i = 0; i < count; i++) {
        tapirscan_barcode barcode;
        if (tapirscan_result_barcode(result, i, &barcode) != TAPIRSCAN_OK) continue;
        char *text = malloc(barcode.text_length + 1); /* the copy adds a NUL */
        if (text && tapirscan_result_copy(result, i, TAPIRSCAN_FIELD_TEXT, (uint8_t *)text,
                                          barcode.text_length + 1) == TAPIRSCAN_OK) {
            printf("%s %s at (%.1f, %.1f)\n", tapirscan_format_name(barcode.format), text,
                   barcode.polygon[0].x, barcode.polygon[0].y);
        }
        free(text);
    }
    tapirscan_result_destroy(result);
    return TAPIRSCAN_OK;
}

int scan_frames(const uint8_t *const *frames, size_t count, uint64_t width, uint64_t height) {
    tapirscan_scanner scanner;
    int32_t status = tapirscan_scanner_create(NULL, &scanner, NULL); /* Medium, retail */
    if (status != TAPIRSCAN_OK) return status;
    for (size_t i = 0; i < count && status == TAPIRSCAN_OK; i++) {
        status = scan_rgba(scanner, frames[i], width, height);
    }
    tapirscan_scanner_destroy(scanner);
    return status;
}
```

Operations return a `tapirscan_status`; `tapirscan_status_message` describes it.
If nothing is decoded, the call succeeds and `tapirscan_result_count` returns 0.
Results own their data and outlive their scanner. C has no `best` helper; choose
from the list by format, payload or position. Support, undecoded regions and
duplicate payloads are described in [API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md).

## Reuse and configuration

Initialize scanner configuration with `TAPIRSCAN_SCANNER_OPTIONS_INIT` and set
only the fields you change; a zero-initialized `tapirscan_scanner_options` is
rejected because `formats` 0 is invalid. Pass a caller-owned `tapirscan_error`
as the final create/scan argument for detailed errors. Its message is cleared on
success and contains up to 511 UTF-8 bytes on failure; status codes remain
authoritative. Use a separate error output per call.

Create a scanner once and reuse it across images. Pass `NULL` options for the
defaults, or select a mode, formats and supplement policy:

```c
tapirscan_scanner_options options = TAPIRSCAN_SCANNER_OPTIONS_INIT;
options.mode = TAPIRSCAN_MODE_HIGH;
options.formats = TAPIRSCAN_FORMAT_EAN13 | TAPIRSCAN_FORMAT_QR_CODE;
tapirscan_scanner scanner;
tapirscan_scanner_create(&options, &scanner, NULL);

/* `image` is a tapirscan_image as in the example above. */
tapirscan_scan_options scan = {TAPIRSCAN_FORMAT_QR_CODE}; /* this call only */
tapirscan_result result;
tapirscan_inspect(scanner, &image, &scan, &result, NULL);
```

| Scanner option      | Default                       | Choices                                      |
| ------------------- | ----------------------------- | -------------------------------------------- |
| `mode`              | `TAPIRSCAN_MODE_MEDIUM`       | `_LOW`, `_MEDIUM`, `_HIGH`, `_VERY_HIGH`     |
| `formats`           | `TAPIRSCAN_FORMATS_RETAIL`    | `TAPIRSCAN_FORMAT_*` bits combined with `\|` |
| `ean_add_on_policy` | `TAPIRSCAN_EAN_ADD_ON_IGNORE` | `_IGNORE`, `_READ`, `_REQUIRE`               |
| `parallel`          | `TAPIRSCAN_PARALLEL_DEFAULT`  | `_OFF`, `_ON`, `_DEFAULT`                    |

`parallel` runs one scan's formats on separate threads where the host allows it;
results are identical either way. Use `TAPIRSCAN_PARALLEL_OFF` when you already
run one scanner per core. `_DEFAULT` (also selected by `NULL` options) is on
unless the process environment sets `TAPIRSCAN_THREADS` to `0` or `1`. It is
zero, so an initializer that leaves `parallel` out keeps the default.

Presets: `TAPIRSCAN_FORMATS_RETAIL`, `_COMMON_1D`, `_COMMON`, `_LINEAR`,
`_MATRIX` and `_ALL`, from the generated `tapirscan_formats.h`. See
[format coverage](https://github.com/kleinicke/tapirscan/blob/main/docs/FORMATS.md).

| Per-scan option | Default | Meaning                                                      |
| --------------- | ------- | ------------------------------------------------------------ |
| `formats`       | `0`     | Readers for this call, replacing the scanner's; 0 keeps them |

`NULL` scan options equal a zero-initialized struct.

## Results

`tapirscan_result_count` returns the number of decoded barcodes for either
operation.

`tapirscan_result_barcode` fills a `tapirscan_barcode` with the source-image
`polygon`, `support`, `format` and the lengths of its variable fields. Copy a
field with `tapirscan_result_copy` into a buffer larger than its length; the copy
is NUL-terminated and preserves embedded NUL bytes. Optional fields
(`TAPIRSCAN_FIELD_PAYLOAD_BYTES`, `_EAN_ADD_ON`, `_STRUCTURED_APPEND_ID`) report
`TAPIRSCAN_ABSENT` as their length when unavailable. `gs1` and
`reader_initialization` are -1 when the reader does not report them.

For inspection results, `tapirscan_result_info` fills a `tapirscan_summary`:
`barcode_count`, `undecoded_count`, `width`, `height`, `mode` and `elapsed_ms`.
`tapirscan_result_undecoded` fills a `tapirscan_region` for each localized region
without an accepted decode; `format` is 0 when unknown.

JSON is serialized on first use. Query `tapirscan_result_json_length(result,
&length)`, allocate `length + 1` bytes, then call `tapirscan_result_copy_json`.
For scans it is an array of the decoded barcodes, each with its structured
`content` unless the text is plain (parsed when the JSON is first requested);
for inspections it is a schema-2 engine report whose fields may change and
carries no `content`.

## Structured content

`tapirscan_parse_content` returns a handle to the named fields of GS1, Wi-Fi,
contact, payment and similar payloads, or 0 for plain text. Pass the barcode's
`gs1` flag.

```c
/* `text` is the copied barcode text, a char *, and barcode.text_length its length. */
tapirscan_content content = 0;
tapirscan_parse_content((const uint8_t *)text, barcode.text_length, barcode.gs1, &content, NULL);
if (content) {
    tapirscan_content_summary summary;
    tapirscan_content_info(content, &summary); /* kind, field_count */
    /* read fields with tapirscan_content_field and tapirscan_content_copy */
    tapirscan_content_destroy(content);
}
```

Kinds and fields: [structured content](../../docs/CONTENT.md).

## Images

`tapirscan_image` describes gray8, RGB8 or RGBA8 pixels (`channels` 1, 3 or 4).
`stride` is bytes per row; 0 means `width * channels`. `length` is the readable
buffer size. Pixels are borrowed only during the call. Size limits and
conversion rules are in [API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md#images).

## Threads and limits

Calls are thread-safe. Scans on one scanner serialize; separate scanners run
concurrently, and each scan uses extra threads unless `parallel` is off. Destroy each handle once; stale handles return
`TAPIRSCAN_INVALID_HANDLE`. Status failures never unwind into C; out-of-memory
and invalid raw memory are outside that guarantee, and valid pointers remain the
caller's responsibility. The library targets 64-bit platforms.

## Building

```sh
python3 scripts/build_native.py
```

This builds `build/native/libtapirscan.{so,dylib}` (or `tapirscan.dll`). Compile
with `-Ibindings/c/include` and link the library. The CMake package in
[bindings/cpp](https://github.com/kleinicke/tapirscan/blob/main/bindings/cpp/README.md) installs the library and both headers for C and
C++ consumers. `tapirscan_abi_version()` returns `TAPIRSCAN_ABI_VERSION`; build
the application against the header that matches the library.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

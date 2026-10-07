# Tapirscan for C

`tapirscan_scan` returns an owned result handle. Read the barcode count with
`tapirscan_result_count`, then each barcode with `tapirscan_result_barcode` and
`tapirscan_result_copy`. Positions are in `polygon`. Destroy the result with
`tapirscan_result_destroy`.

Use `tapirscan_inspect` to also get unread regions, timing and diagnostics.
`tapirscan_result_info` and `tapirscan_result_undecoded` require an inspection
result and return `TAPIRSCAN_INVALID_ARGUMENT` for ordinary scan results.

Defaults are Medium effort and retail formats (EAN13, UPCA, EAN8 and UPCE). One
shared library contains all four effort modes.

```c
#include <tapirscan.h>
#include <stdio.h>

int scan_rgba(const uint8_t *pixels, uint64_t width, uint64_t height) {
    tapirscan_scanner scanner;
    int status = tapirscan_scanner_create(NULL, &scanner, NULL); /* Medium, Retail */
    if (status != TAPIRSCAN_OK) return status;
    tapirscan_image image = {pixels, width * height * 4, width, height, 4, 0};
    tapirscan_result result;
    status = tapirscan_scan(scanner, &image, NULL, &result, NULL);
    if (status == TAPIRSCAN_OK) {
        uint64_t count;
        tapirscan_result_count(result, &count);
        for (uint64_t i = 0; i < count; i++) {
            tapirscan_barcode barcode;
            char text[256];
            tapirscan_result_barcode(result, i, &barcode);
            if (barcode.text_length < sizeof text &&
                tapirscan_result_copy(result, i, TAPIRSCAN_FIELD_TEXT, (uint8_t *)text,
                                      sizeof text) == TAPIRSCAN_OK) {
                printf("%s %s at (%.1f, %.1f)\n", tapirscan_format_name(barcode.format),
                       text, barcode.polygon[0].x, barcode.polygon[0].y);
            }
        }
        tapirscan_result_destroy(result);
    } else {
        fprintf(stderr, "%s\n", tapirscan_status_message(status));
    }
    tapirscan_scanner_destroy(scanner);
    return status;
}
```

Operations return a `tapirscan_status`; `tapirscan_status_message` describes
it. If nothing is decoded, the call succeeds and `tapirscan_result_count` returns 0. Results own their data and outlive their scanner. Equal payloads at distinct
locations are reported as separate barcodes. C has no `best` helper; choose from
the list by format, payload or position.

## Reuse and configuration

Initialize mutable scanner configuration with `TAPIRSCAN_SCANNER_OPTIONS_INIT`.
Unlike a zero-initialized per-scan struct, this selects Medium/Retail defaults.
Pass a caller-owned `tapirscan_error` as the final create/scan argument for detailed
errors. Its message is cleared on success and contains up to 511 UTF-8 bytes on
failure; status codes remain authoritative. Use a separate error output per call.

Create a scanner once and reuse it across images. Pass `NULL` options for the
defaults, or select a mode, formats and supplement policy:

```c
tapirscan_scanner_options options = {
    TAPIRSCAN_MODE_HIGH,
    TAPIRSCAN_FORMAT_EAN13 | TAPIRSCAN_FORMAT_QR_CODE,
    TAPIRSCAN_EAN_ADD_ON_IGNORE,
};
tapirscan_scanner scanner;
tapirscan_scanner_create(&options, &scanner, NULL);

/* `image` is a tapirscan_image as in the example above. */
tapirscan_scan_options scan = {0}; /* zero fields keep the defaults */
scan.extended_budget = 1;
tapirscan_result result;
tapirscan_inspect(scanner, &image, &scan, &result, NULL);
```

| Scanner option      | Default                       | Choices                                      |
| ------------------- | ----------------------------- | -------------------------------------------- |
| `mode`              | `TAPIRSCAN_MODE_MEDIUM`       | `_LOW`, `_MEDIUM`, `_HIGH`, `_VERY_HIGH`     |
| `formats`           | `TAPIRSCAN_FORMATS_RETAIL`    | `TAPIRSCAN_FORMAT_*` bits combined with `\|` |
| `ean_add_on_policy` | `TAPIRSCAN_EAN_ADD_ON_IGNORE` | `_IGNORE`, `_READ`, `_REQUIRE`               |

Presets: `TAPIRSCAN_FORMATS_RETAIL`, `_COMMON_1D`, `_COMMON`, `_LINEAR`, `_MATRIX`
and `_ALL`, from the generated `tapirscan_formats.h`. Retail formats are
enabled by default; additional formats are supported when selected. See [format coverage](../../docs/FORMATS.md).

| Per-scan option   | Default | Meaning                                      |
| ----------------- | ------- | -------------------------------------------- |
| `formats`         | `0`     | Override readers for this call; 0 keeps them |
| `extended_budget` | `0`     | 1 allows extra reader work for any format    |

`NULL` scan options equal a zero-initialized struct. `extended_budget` can cost
more time and does not promise exhaustive decoding; see [API design](../../docs/API_DESIGN.md).

## Results

`tapirscan_result_count` returns the number of decoded barcodes for either
operation.

`tapirscan_result_barcode` fills a `tapirscan_barcode` with the source-image
`polygon`, `support`, `format` and the lengths of its variable fields. `support`
is reader-specific evidence, a ranking heuristic and not a confidence. Copy a
field with `tapirscan_result_copy` into a buffer larger than its length; the copy
is NUL-terminated and preserves embedded NUL bytes. Optional fields
(`TAPIRSCAN_FIELD_PAYLOAD_BYTES`, `_EAN_ADD_ON`, `_STRUCTURED_APPEND_ID`) report
`TAPIRSCAN_ABSENT` as their length when unavailable. `gs1` and
`reader_initialization` are -1 when the reader does not report them.

For inspection results, `tapirscan_result_info` fills a `tapirscan_summary`:
`barcode_count`, `undecoded_count`, `width`, `height`, `mode` and `elapsed_ms`.
`tapirscan_result_undecoded` fills a `tapirscan_region` for each localized
region without an accepted decode; `format` is 0 when unknown. These can be
false candidates, and an empty list does not guarantee that every barcode was found.

JSON is serialized on first use. Query `tapirscan_result_json_length(result,
&length)`, allocate `length + 1` bytes, then call `tapirscan_result_copy_json`.
For scans it is an array of the decoded barcodes; for inspections it is a
diagnostics report whose schema is unstable.

## Images

`tapirscan_image` describes gray8, RGB8 or RGBA8 pixels (`channels` 1, 3 or 4).
Alpha is ignored. `stride` is bytes per row; 0 means `width * channels`. Images
are at least 3×3 and at most 32 megapixels. `length` is the readable buffer
size; the addressed layout, `(height - 1) * stride + width * channels` bytes,
must fit in it and in 128 MiB. A larger backing buffer, such as a frame around
a cropped view, is accepted. Pixels are borrowed only during the call. Decode image files and convert BGR,
planar, float or 16-bit pixels before scanning.

## Threads and limits

Calls are thread-safe. Scans on one scanner serialize; separate scanners run
concurrently. Handles are checked IDs: a destroyed or unknown handle returns
`TAPIRSCAN_INVALID_HANDLE`, and IDs are never reused. There is no fixed limit
on live scanners or results; destroy each one. Native panics never unwind into C. Valid pointers remain the
caller's responsibility. The library targets 64-bit platforms.

## Building

```sh
python3 scripts/build_native.py
```

This builds `build/native/libtapirscan.{so,dylib}` (or `tapirscan.dll`). Compile
with `-Ibindings/c/include` and link the library. The CMake package in
[bindings/cpp](../cpp/README.md) installs the library and both headers for C and
C++ consumers. `tapirscan_abi_version()` returns `TAPIRSCAN_ABI_VERSION`; build
the application against the header that matches the library.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

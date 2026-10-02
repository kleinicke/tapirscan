# Tapirscan for C

Scan decoded pixels and receive every accepted barcode, source-image geometry,
undecoded proposals and reported work limits. Defaults are Medium effort and
retail formats (EAN13, UPCA, EAN8 and UPCE). One shared library contains all
four effort modes.

```c
#include <tapirscan.h>
#include <stdio.h>

int scan_rgba(const uint8_t *pixels, uint64_t width, uint64_t height) {
    tapirscan_scanner scanner;
    int status = tapirscan_scanner_create(NULL, &scanner); /* Medium, Retail */
    if (status != TAPIRSCAN_OK) return status;
    tapirscan_image image = {pixels, width * height * 4, width, height, 4, 0};
    tapirscan_result result;
    status = tapirscan_scan(scanner, &image, NULL, &result);
    if (status == TAPIRSCAN_OK) {
        tapirscan_summary summary;
        tapirscan_result_info(result, &summary);
        for (uint64_t i = 0; i < summary.barcode_count; i++) {
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

Every function returns a `tapirscan_status`; `tapirscan_status_message` describes
it. No detection is a successful result with `barcode_count` 0. Results own their
data and outlive their scanner. Equal payloads at distinct locations remain
separate physical instances.

## Reuse and configuration

Create a scanner once and reuse it across images. Pass `NULL` options for the
defaults, or select a mode, formats and supplement policy:

```c
tapirscan_scanner_options options = {
    TAPIRSCAN_MODE_HIGH,
    TAPIRSCAN_FORMAT_EAN13 | TAPIRSCAN_FORMAT_QR_CODE,
    TAPIRSCAN_EAN_ADD_ON_IGNORE,
};
tapirscan_scanner scanner;
tapirscan_scanner_create(&options, &scanner);

tapirscan_scan_options scan = {0}; /* zero fields keep the defaults */
scan.extended_budget = 1;
tapirscan_scan(scanner, &image, &scan, &result);
```

| Scanner option      | Default                       | Choices                                      |
| ------------------- | ----------------------------- | -------------------------------------------- |
| `mode`              | `TAPIRSCAN_MODE_MEDIUM`       | `_LOW`, `_MEDIUM`, `_HIGH`, `_VERY_HIGH`     |
| `formats`           | `TAPIRSCAN_FORMATS_RETAIL`    | `TAPIRSCAN_FORMAT_*` bits combined with `\|` |
| `ean_add_on_policy` | `TAPIRSCAN_EAN_ADD_ON_IGNORE` | `_IGNORE`, `_READ`, `_REQUIRE`               |

Presets: `TAPIRSCAN_FORMATS_RETAIL`, `_COMMON_1D`, `_COMMON`, `_LINEAR`, `_MATRIX`
and `_ALL`, from the generated `tapirscan_formats.h`. Retail formats are
supported; other readers remain experimental. See [format coverage](../../docs/FORMATS.md).

| Per-scan option   | Default | Meaning                                      |
| ----------------- | ------- | -------------------------------------------- |
| `formats`         | `0`     | Override readers for this call; 0 keeps them |
| `debug`           | `0`     | 1 adds engine evidence to the result JSON    |
| `extended_budget` | `0`     | 1 allows extra reader work for any format    |

`NULL` scan options equal a zero-initialized struct. `extended_budget` can cost
more time and does not promise exhaustive decoding; see [API design](../../docs/API_DESIGN.md).

## Results

`tapirscan_result_info` fills a `tapirscan_summary`: `barcode_count`,
`undecoded_count`, `best_index` (highest support, first on ties; -1 when empty),
`width`, `height`, `mode`, `elapsed_ms` and `unfinished`. Support is
reader-specific evidence, not a probability or a cross-format confidence.

`tapirscan_result_barcode` fills a `tapirscan_barcode` with the source-image
`polygon`, `support`, `format` and the lengths of its variable fields. Copy a
field with `tapirscan_result_copy` into a buffer larger than its length; the copy
is NUL-terminated and preserves embedded NUL bytes. Optional fields
(`TAPIRSCAN_FIELD_PAYLOAD_BYTES`, `_EAN_ADD_ON`, `_STRUCTURED_APPEND_ID`) report
`TAPIRSCAN_ABSENT` as their length when unavailable. `gs1` and
`reader_initialization` are -1 when the reader does not report them.

`tapirscan_result_undecoded` fills a `tapirscan_region` for each localized
proposal without an accepted decode; `format` is 0 when unknown. These can be
false candidates or deferred work. An empty list and `unfinished` 0 do not
guarantee exhaustive coverage.

`tapirscan_result_copy_json` copies schema-2 JSON of the decoded results, plus
unstable engine evidence when the scan requested `debug`. Destroy every result
with `tapirscan_result_destroy` and every scanner with `tapirscan_scanner_destroy`.

## Images

`tapirscan_image` describes gray8, RGB8 or RGBA8 pixels (`channels` 1, 3 or 4).
Alpha is ignored. `stride` is bytes per row; 0 means `width * channels`. Images
are at least 3×3, at most 32 megapixels, and `length` must cover
`(height - 1) * stride + width * channels` bytes, at most 128 MiB. Pixels are
borrowed only during the call. Decode image files and convert BGR, planar, float
or 16-bit pixels before scanning.

## Threads and limits

Calls are thread-safe. Scans on one scanner serialize; separate scanners run
concurrently. Handles are checked IDs: a destroyed or unknown handle returns
`TAPIRSCAN_INVALID_HANDLE`. At most 1024 scanners and 1024 results may be alive
at once. Native panics never unwind into C. Invalid raw pointers remain the
caller's responsibility. The ABI targets 64-bit platforms; macOS arm64 is
validated locally and Linux in CI.

## Building

```sh
python3 scripts/build_native.py
```

This builds `build/native/libtapirscan.{so,dylib}` (or `tapirscan.dll`). Compile
with `-Ibindings/c/include` and link the library. The CMake package in
[bindings/cpp](../cpp/README.md) installs the library and both headers for C and
C++ consumers. `tapirscan_abi_version()` returns `TAPIRSCAN_ABI_VERSION`; rebuild
applications and the library together after an ABI change.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

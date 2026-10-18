#include "tapirscan.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    /* The C compiler must agree with the Rust layouts. */
    assert(sizeof(tapirscan_image) == 48);
    assert(sizeof(tapirscan_summary) == 48);
    assert(sizeof(tapirscan_barcode) == 136);
    assert(sizeof(tapirscan_region) == 72);
    assert(tapirscan_abi_version() == TAPIRSCAN_ABI_VERSION);
    assert(strcmp(tapirscan_format_name(TAPIRSCAN_FORMAT_QR_CODE), "QRCode") == 0);
    assert(tapirscan_format_name(TAPIRSCAN_FORMATS_RETAIL) == NULL);

    const char *wifi = "WIFI:S:Cafe;T:WPA;P:secret;;";
    tapirscan_content content = 0;
    assert(tapirscan_parse_content((const uint8_t *)wifi, strlen(wifi), -1, &content, NULL) == TAPIRSCAN_OK);
    tapirscan_content_summary content_info;
    assert(tapirscan_content_info(content, &content_info) == TAPIRSCAN_OK);
    assert(content_info.kind == TAPIRSCAN_CONTENT_WIFI && content_info.field_count == 3);
    assert(strcmp(tapirscan_content_kind_name(content_info.kind), "WiFi") == 0);
    tapirscan_content_lengths field;
    assert(tapirscan_content_field(content, 0, &field) == TAPIRSCAN_OK && field.value_length == 4);
    char ssid[5];
    assert(tapirscan_content_copy(content, 0, TAPIRSCAN_CONTENT_FIELD_VALUE, (uint8_t *)ssid, sizeof(ssid)) == TAPIRSCAN_OK);
    assert(strcmp(ssid, "Cafe") == 0);
    assert(tapirscan_content_destroy(content) == TAPIRSCAN_OK);

    uint8_t pixels[64 * 64];
    memset(pixels, 255, sizeof(pixels));
    tapirscan_image image = {pixels, 1, 64, 64, 1, 0};
    tapirscan_scanner scanner = 0;
    tapirscan_result result = 123;
    tapirscan_scanner_options options = TAPIRSCAN_SCANNER_OPTIONS_INIT;
    assert(options.mode == TAPIRSCAN_MODE_MEDIUM && options.formats == TAPIRSCAN_FORMATS_RETAIL);
    assert(options.parallel == TAPIRSCAN_PARALLEL_DEFAULT);
    options.mode = TAPIRSCAN_MODE_HIGH;
    assert(tapirscan_scanner_create(&options, &scanner, NULL) == TAPIRSCAN_OK);
    tapirscan_error error;
    assert(tapirscan_scan(scanner, &image, NULL, &result, &error) == TAPIRSCAN_INVALID_ARGUMENT);
    assert(strstr(error.message, "buffer"));
    assert(result == 0);

    image.length = sizeof(pixels);
    assert(sizeof(tapirscan_scan_options) == 4);
    assert(tapirscan_scan(scanner, &image, NULL, &result, NULL) == TAPIRSCAN_OK);
    uint64_t count = 99;
    assert(tapirscan_result_count(result, &count) == TAPIRSCAN_OK && count == 0);
    tapirscan_summary unavailable;
    assert(tapirscan_result_info(result, &unavailable) == TAPIRSCAN_INVALID_ARGUMENT);
    assert(tapirscan_result_destroy(result) == TAPIRSCAN_OK);
    assert(tapirscan_inspect(scanner, &image, NULL, &result, NULL) == TAPIRSCAN_OK);
    assert(tapirscan_scanner_destroy(scanner) == TAPIRSCAN_OK);
    assert(tapirscan_scanner_destroy(scanner) == TAPIRSCAN_INVALID_HANDLE);

    /* An initializer written before the parallel field existed keeps the default. */
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wmissing-field-initializers"
    tapirscan_scanner_options three_fields = {TAPIRSCAN_MODE_LOW, TAPIRSCAN_FORMATS_RETAIL, TAPIRSCAN_EAN_ADD_ON_IGNORE};
#pragma GCC diagnostic pop
    assert(three_fields.parallel == TAPIRSCAN_PARALLEL_DEFAULT);
    tapirscan_scanner_options designated = {.mode = TAPIRSCAN_MODE_LOW, .formats = TAPIRSCAN_FORMATS_RETAIL};
    assert(designated.parallel == TAPIRSCAN_PARALLEL_DEFAULT);

    for (uint32_t parallel = TAPIRSCAN_PARALLEL_DEFAULT; parallel <= TAPIRSCAN_PARALLEL_ON + 1; parallel++) {
        tapirscan_scanner_options threads = TAPIRSCAN_SCANNER_OPTIONS_INIT;
        threads.parallel = parallel;
        tapirscan_scanner configured = 0;
        tapirscan_result threaded = 0;
        int32_t created = tapirscan_scanner_create(&threads, &configured, NULL);
        if (parallel > TAPIRSCAN_PARALLEL_ON) {
            assert(created == TAPIRSCAN_INVALID_ARGUMENT);
            continue;
        }
        assert(created == TAPIRSCAN_OK);
        assert(tapirscan_scan(configured, &image, NULL, &threaded, NULL) == TAPIRSCAN_OK);
        assert(tapirscan_result_destroy(threaded) == TAPIRSCAN_OK);
        assert(tapirscan_scanner_destroy(configured) == TAPIRSCAN_OK);
    }

    /* Results outlive their scanner. */
    tapirscan_summary info;
    assert(tapirscan_result_info(result, &info) == TAPIRSCAN_OK);
    assert(info.barcode_count == 0 && info.undecoded_count == 0);
    assert(info.mode == TAPIRSCAN_MODE_HIGH && info.width == 64 && info.height == 64);
    uint64_t length = 0;
    assert(tapirscan_result_json_length(result, &length) == TAPIRSCAN_OK);
    uint8_t *json = malloc(length + 1);
    assert(json);
    assert(tapirscan_result_copy_json(result, json, length) == TAPIRSCAN_BUFFER_TOO_SMALL);
    assert(tapirscan_result_copy_json(result, json, length + 1) == TAPIRSCAN_OK);
    assert(json[length] == 0);
    assert(strstr((char *)json, "\"searchWindows\""));
    free(json);
    assert(tapirscan_result_destroy(result) == TAPIRSCAN_OK);
    assert(tapirscan_result_info(result, &info) == TAPIRSCAN_INVALID_HANDLE);
    return 0;
}

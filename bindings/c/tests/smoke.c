#include "tapirscan.h"
#include <assert.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    /* The C compiler must agree with the Rust layouts. */
    assert(sizeof(tapirscan_image) == 48);
    assert(sizeof(tapirscan_summary) == 64);
    assert(sizeof(tapirscan_barcode) == 136);
    assert(sizeof(tapirscan_region) == 72);
    assert(tapirscan_abi_version() == TAPIRSCAN_ABI_VERSION);
    assert(strcmp(tapirscan_format_name(TAPIRSCAN_FORMAT_QR_CODE), "QRCode") == 0);
    assert(tapirscan_format_name(TAPIRSCAN_FORMATS_RETAIL) == NULL);

    uint8_t pixels[64 * 64];
    memset(pixels, 255, sizeof(pixels));
    tapirscan_image image = {pixels, 1, 64, 64, 1, 0};
    tapirscan_scanner scanner = 0;
    tapirscan_result result = 123;
    tapirscan_scanner_options options = {TAPIRSCAN_MODE_HIGH, TAPIRSCAN_FORMATS_RETAIL,
                                         TAPIRSCAN_EAN_ADD_ON_IGNORE};
    assert(tapirscan_scanner_create(&options, &scanner) == TAPIRSCAN_OK);
    assert(tapirscan_scan(scanner, &image, NULL, &result) == TAPIRSCAN_INVALID_ARGUMENT);
    assert(result == 0);

    image.length = sizeof(pixels);
    tapirscan_scan_options debug = {0, 1, 0};
    assert(tapirscan_scan(scanner, &image, &debug, &result) == TAPIRSCAN_OK);
    assert(tapirscan_scanner_destroy(scanner) == TAPIRSCAN_OK);
    assert(tapirscan_scanner_destroy(scanner) == TAPIRSCAN_INVALID_HANDLE);

    /* Results outlive their scanner. */
    tapirscan_summary info;
    assert(tapirscan_result_info(result, &info) == TAPIRSCAN_OK);
    assert(info.barcode_count == 0 && info.best_index == -1);
    assert(info.mode == TAPIRSCAN_MODE_HIGH && info.width == 64 && info.height == 64);
    uint8_t *json = malloc(info.json_length + 1);
    assert(json);
    assert(tapirscan_result_copy_json(result, json, info.json_length) == TAPIRSCAN_BUFFER_TOO_SMALL);
    assert(tapirscan_result_copy_json(result, json, info.json_length + 1) == TAPIRSCAN_OK);
    assert(json[info.json_length] == 0);
    assert(strstr((char *)json, "\"searchWindows\""));
    free(json);
    assert(tapirscan_result_destroy(result) == TAPIRSCAN_OK);
    assert(tapirscan_result_info(result, &info) == TAPIRSCAN_INVALID_HANDLE);
    return 0;
}

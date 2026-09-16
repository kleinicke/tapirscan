#ifndef TAPIRSCAN_H
#define TAPIRSCAN_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Native ABI v4. All functions use the platform C calling convention.
   Mode: 0 Low, 1 Medium, 2 High, 3 Very High. Handles belong to the library that created them.
   Pointer arguments must reference valid, aligned, nonoverlapping caller memory.
   Scan borrows immutable pixels for the call; it does not retain their address.
   Calls are thread-safe and scans on a scanner serialize. Destroy removes a
   handle; an already-started call may finish. Results outlive their scanner.
   Release every successful result and scanner. Maximum 1024 of each per library.
   Status failures never unwind through C; invalid raw memory and OOM are outside
   that guarantee. JSON copies include NUL; json_length excludes it. */
enum barcode_status { BARCODE_OK=0, BARCODE_INVALID_ARGUMENT=1, BARCODE_INVALID_HANDLE=2,
    BARCODE_BUFFER_TOO_SMALL=3, BARCODE_INTERNAL_ERROR=4, BARCODE_CAPACITY=5 };
enum barcode_scan_flags { BARCODE_SINGLE=1, BARCODE_INCLUDE_REGIONS=2,
    BARCODE_READ_EAN_ADDON=4, BARCODE_REQUIRE_EAN_ADDON=8, BARCODE_FINISH_CANDIDATES=16 };
/* READ_EAN_ADDON attempts optional 2/5-digit EAN/UPC supplements.
   REQUIRE_EAN_ADDON accepts retail reads only with a confirmed supplement.
   The two flags are mutually exclusive; neither affects non-retail formats.
   Supplement text is returned as eanAddOn in result JSON. Default: ignore. */
/* Flags 0: multiple results, no region evidence. SINGLE ranks a completed scan;
   it does not stop scanning early. JSON schema 2 omits region fields by default. */
typedef uint64_t tapirscan_handle;
typedef uint64_t barcode_result;
typedef struct barcode_read { double polygon[8]; uint32_t support; uint64_t text_length; char format[24]; } barcode_read;
typedef struct barcode_result_metadata {
    uint64_t json_length, barcode_count;
    uint32_t unfinished, localization_limited;
} barcode_result_metadata;
uint32_t barcode_abi_version(void);
/* Bit 0 enables BARCODE_FINISH_CANDIDATES: remove shared frame retry/association
   budgets for EAN13/UPCA. Per-candidate and other limits remain. Requires either
   primary format. Default off. A call has no deadline; unfinished can stay true. */
uint32_t barcode_capabilities(void);
uint32_t barcode_mode(void);
int32_t tapirscan_create(tapirscan_handle *out);
int32_t tapirscan_destroy(tapirscan_handle scanner);
int32_t barcode_scan(tapirscan_handle scanner, const uint8_t *pixels, uint64_t length,
    uint64_t width, uint64_t height, uint32_t channels, uint64_t stride, barcode_result *out);
int32_t barcode_scan_with_options(tapirscan_handle scanner, const uint8_t *pixels, uint64_t length,
    uint64_t width, uint64_t height, uint32_t channels, uint64_t stride, uint32_t flags, barcode_result *out);
/* Format bits: EAN13=1, UPCA=2, EAN8=4, UPCE=8, Code128=16, Code39=32,
   ITF=64, Codabar=128, Code93=256, QRCode=512, DataMatrix=1024, PDF417=2048,
   Aztec=4096, DataBar=8192, DataBarExpanded=16384, MaxiCode=131072. */
int32_t barcode_scan_formats(tapirscan_handle scanner, const uint8_t *pixels, uint64_t length,
    uint64_t width, uint64_t height, uint32_t channels, uint64_t stride, uint32_t flags, uint32_t formats, barcode_result *out);
/* text_length excludes the terminal NUL. Copy with text_length+1 capacity;
   use the explicit length to preserve embedded NULs. No truncation. */
int32_t barcode_result_copy_text(barcode_result result, uint64_t index, uint8_t *out, uint64_t capacity);
int32_t barcode_result_info(barcode_result result, barcode_result_metadata *out);
int32_t barcode_result_read(barcode_result result, uint64_t index, barcode_read *out);
int32_t barcode_result_copy_json(barcode_result result, uint8_t *out, uint64_t capacity);
int32_t barcode_result_destroy(barcode_result result);
#ifdef __cplusplus
}
#endif
#endif

#ifndef TAPIRSCAN_H
#define TAPIRSCAN_H
#include <stdint.h>
#include "tapirscan_formats.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Tapirscan native ABI 6: one library containing all four effort modes.

   Create a scanner once, scan any number of images, read the owned result,
   then destroy the result and the scanner. Calls are thread-safe; scans on one
   scanner serialize. Results outlive their scanner. Pointer arguments must
   reference valid, aligned, nonoverlapping caller memory. Status failures never
   unwind through C; invalid raw memory and out-of-memory are outside that
   guarantee. Handle IDs are never reused. */
#define TAPIRSCAN_ABI_VERSION 6u

typedef uint64_t tapirscan_scanner;
typedef uint64_t tapirscan_result;

typedef enum tapirscan_status {
    TAPIRSCAN_OK = 0,
    TAPIRSCAN_INVALID_ARGUMENT = 1,
    TAPIRSCAN_INVALID_HANDLE = 2,
    TAPIRSCAN_BUFFER_TOO_SMALL = 3,
    TAPIRSCAN_INTERNAL_ERROR = 4
} tapirscan_status;

typedef enum tapirscan_mode {
    TAPIRSCAN_MODE_LOW = 0,
    TAPIRSCAN_MODE_MEDIUM = 1,
    TAPIRSCAN_MODE_HIGH = 2,
    TAPIRSCAN_MODE_VERY_HIGH = 3
} tapirscan_mode;

/* Whether to read the adjacent two/five-digit EAN/UPC supplement. REQUIRE
   returns retail reads only with a readable supplement; other formats are
   unaffected. */
typedef enum tapirscan_ean_add_on_policy {
    TAPIRSCAN_EAN_ADD_ON_IGNORE = 0,
    TAPIRSCAN_EAN_ADD_ON_READ = 1,
    TAPIRSCAN_EAN_ADD_ON_REQUIRE = 2
} tapirscan_ean_add_on_policy;

/* Scanner configuration. Passing NULL to tapirscan_scanner_create selects
   TAPIRSCAN_MODE_MEDIUM, TAPIRSCAN_FORMATS_RETAIL and ..._ADD_ON_IGNORE. */
typedef struct tapirscan_scanner_options {
    uint32_t mode;              /* tapirscan_mode */
    uint32_t formats;           /* nonempty TAPIRSCAN_FORMAT_* mask */
    uint32_t ean_add_on_policy; /* tapirscan_ean_add_on_policy */
} tapirscan_scanner_options;
#define TAPIRSCAN_SCANNER_OPTIONS_INIT {TAPIRSCAN_MODE_MEDIUM, TAPIRSCAN_FORMATS_RETAIL, TAPIRSCAN_EAN_ADD_ON_IGNORE}

/* Optional caller-owned output for create/scan. Cleared on success; on failure
   holds NUL-terminated UTF-8, truncated at a character boundary if necessary.
   Use one per concurrent call. NULL discards details; the status is unchanged. */
typedef struct tapirscan_error { char message[512]; } tapirscan_error;

/* Gray8, RGB8 or RGBA8 pixels (alpha ignored), at least 3x3 and 32 megapixels.
   stride is the distance between rows in bytes; 0 means width * channels.
   length is the readable buffer size. The addressed layout,
   (height - 1) * stride + width * channels bytes, must fit in it and in 128 MiB;
   a larger backing buffer, such as a frame around a crop, is fine. */
typedef struct tapirscan_image {
    const uint8_t *data;
    uint64_t length;
    uint64_t width;
    uint64_t height;
    uint32_t channels;
    uint64_t stride;
} tapirscan_image;

/* Per-scan overrides. NULL or a zero-initialized struct uses the defaults:
   the scanner's formats, no diagnostics and the ordinary work budget. */
typedef struct tapirscan_scan_options {
    uint32_t formats;         /* 0 or a nonempty TAPIRSCAN_FORMAT_* mask */
    uint32_t extended_budget; /* 1 allows reader-specific extra work */
} tapirscan_scan_options;

/* Source-image pixels, origin top-left, x rightward and y downward. */
typedef struct tapirscan_point { double x, y; } tapirscan_point;

typedef struct tapirscan_summary {
    uint64_t barcode_count;
    uint64_t undecoded_count;
    uint64_t width, height;   /* supplied image size */
    double elapsed_ms;
    uint32_t mode;            /* tapirscan_mode */
} tapirscan_summary;

/* Lengths of absent optional fields are TAPIRSCAN_ABSENT. Tri-state flags are
   -1 (not reported), 0 or 1. structured_append_count is 0 when absent. */
#define TAPIRSCAN_ABSENT UINT64_MAX
typedef struct tapirscan_barcode {
    tapirscan_point polygon[4];
    uint64_t support;                 /* uncalibrated, reader-specific evidence */
    uint32_t format;                  /* one TAPIRSCAN_FORMAT_* bit */
    int32_t gs1;
    int32_t reader_initialization;
    int32_t structured_append_parity; /* -1 when absent */
    uint64_t structured_append_index; /* one-based */
    uint64_t structured_append_count;
    uint64_t text_length;
    uint64_t payload_bytes_length;
    uint64_t ean_add_on_length;
    uint64_t structured_append_id_length;
} tapirscan_barcode;

/* A localized region without an accepted decode. format is 0 when unknown. */
typedef struct tapirscan_region {
    tapirscan_point polygon[4];
    uint32_t format;
} tapirscan_region;

/* Variable-length barcode fields for tapirscan_result_copy. */
typedef enum tapirscan_field {
    TAPIRSCAN_FIELD_TEXT = 0,
    TAPIRSCAN_FIELD_PAYLOAD_BYTES = 1,
    TAPIRSCAN_FIELD_EAN_ADD_ON = 2,
    TAPIRSCAN_FIELD_STRUCTURED_APPEND_ID = 3
} tapirscan_field;

uint32_t tapirscan_abi_version(void);
/* Static NUL-terminated strings; tapirscan_format_name returns NULL for
   anything other than a single TAPIRSCAN_FORMAT_* bit. */
const char *tapirscan_status_message(int32_t status);
const char *tapirscan_format_name(uint32_t format);

int32_t tapirscan_scanner_create(const tapirscan_scanner_options *options, tapirscan_scanner *out, tapirscan_error *error);
int32_t tapirscan_scanner_destroy(tapirscan_scanner scanner);

/* Pixels are borrowed only for the call. No detection is a successful empty
   result. A call has no deadline. */
int32_t tapirscan_scan(tapirscan_scanner scanner, const tapirscan_image *image,
    const tapirscan_scan_options *options, tapirscan_result *out, tapirscan_error *error);

/* Inspection adds work status, unread regions and engine diagnostics. */
int32_t tapirscan_inspect(tapirscan_scanner scanner, const tapirscan_image *image,
    const tapirscan_scan_options *options, tapirscan_result *out, tapirscan_error *error);
int32_t tapirscan_result_count(tapirscan_result result, uint64_t *out);
/* info and undecoded require an inspection result; otherwise INVALID_ARGUMENT. */
int32_t tapirscan_result_info(tapirscan_result result, tapirscan_summary *out);
int32_t tapirscan_result_barcode(tapirscan_result result, uint64_t index, tapirscan_barcode *out);
int32_t tapirscan_result_undecoded(tapirscan_result result, uint64_t index, tapirscan_region *out);
/* Copy a field plus a terminating NUL; capacity must exceed its length.
   Embedded NUL bytes are preserved. Absent fields return INVALID_ARGUMENT. */
int32_t tapirscan_result_copy(tapirscan_result result, uint64_t index, uint32_t field,
    uint8_t *out, uint64_t capacity);
/* Serializes lazily; length excludes the terminating NUL. */
int32_t tapirscan_result_json_length(tapirscan_result result, uint64_t *out);
/* scan: barcode JSON array. inspect: schema-2 engine report. */
int32_t tapirscan_result_copy_json(tapirscan_result result, uint8_t *out, uint64_t capacity);
int32_t tapirscan_result_destroy(tapirscan_result result);
#ifdef __cplusplus
}
#endif
#endif

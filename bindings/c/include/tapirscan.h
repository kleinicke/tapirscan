#ifndef TAPIRSCAN_H
#define TAPIRSCAN_H
#include <stdint.h>
#include "tapirscan_formats.h"
#include "tapirscan_content_kinds.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Tapirscan native ABI 7: one library containing all four effort modes.

   Create a scanner once, scan any number of images, read the owned result,
   then destroy the result and the scanner. Calls are thread-safe; scans on one
   scanner serialize. Results outlive their scanner. Pointer arguments must
   reference valid, aligned, nonoverlapping caller memory. Status failures never
   unwind through C; invalid raw memory and out-of-memory are outside that
   guarantee. Handle IDs are never reused. */
#define TAPIRSCAN_ABI_VERSION 7u

typedef uint64_t tapirscan_scanner;
typedef uint64_t tapirscan_result;
typedef uint64_t tapirscan_content;

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

/* Whether one scan runs its formats on separate threads where the host allows
   it. Results are identical either way. Use OFF when you already run one
   scanner per core. DEFAULT is ON unless the process environment sets
   TAPIRSCAN_THREADS to 0 or 1. DEFAULT is 0, so an options struct initialized
   without this field, such as {mode, formats, policy}, keeps the default. */
typedef enum tapirscan_parallel {
    TAPIRSCAN_PARALLEL_DEFAULT = 0,
    TAPIRSCAN_PARALLEL_OFF = 1,
    TAPIRSCAN_PARALLEL_ON = 2
} tapirscan_parallel;

/* Scanner configuration. Passing NULL to tapirscan_scanner_create selects
   TAPIRSCAN_MODE_MEDIUM, TAPIRSCAN_FORMATS_RETAIL, ..._ADD_ON_IGNORE and
   TAPIRSCAN_PARALLEL_DEFAULT. */
typedef struct tapirscan_scanner_options {
    uint32_t mode;              /* tapirscan_mode */
    uint32_t formats;           /* nonempty TAPIRSCAN_FORMAT_* mask */
    uint32_t ean_add_on_policy; /* tapirscan_ean_add_on_policy */
    uint32_t parallel;          /* tapirscan_parallel */
} tapirscan_scanner_options;
#define TAPIRSCAN_SCANNER_OPTIONS_INIT {TAPIRSCAN_MODE_MEDIUM, TAPIRSCAN_FORMATS_RETAIL, TAPIRSCAN_EAN_ADD_ON_IGNORE, TAPIRSCAN_PARALLEL_DEFAULT}

/* Optional caller-owned output for create/scan. Cleared on success; on failure
   holds NUL-terminated UTF-8, truncated at a character boundary if necessary.
   Use one per concurrent call. NULL discards details; the status is unchanged. */
typedef struct tapirscan_error { char message[512]; } tapirscan_error;

/* Gray8, RGB8 or RGBA8 pixels (alpha ignored), 3 to 65,535 pixels per side,
   at most 256 megapixels. Images above 32 megapixels are scanned as a reduced
   gray copy, so barcodes that are small in them may be missed; results stay in
   source coordinates.
   stride is the distance between rows in bytes; 0 means width * channels.
   length is the readable buffer size. The addressed layout,
   (height - 1) * stride + width * channels bytes, must fit in it and in 1 GiB;
   a larger backing buffer, such as a frame around a crop, is fine. */
typedef struct tapirscan_image {
    const uint8_t *data;
    uint64_t length;
    uint64_t width;
    uint64_t height;
    uint32_t channels;
    uint64_t stride;
} tapirscan_image;

/* Per-scan overrides. NULL or a zero-initialized struct uses the scanner's formats. */
typedef struct tapirscan_scan_options {
    uint32_t formats; /* 0 or a nonempty TAPIRSCAN_FORMAT_* mask */
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

/* Inspection adds unread regions and engine diagnostics. */
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

/* Structured content of decoded text (docs/CONTENT.md). Parsing is independent
   of scanning and works on any UTF-8 text. */
typedef struct tapirscan_content_summary {
    uint32_t kind;        /* tapirscan_content_kind */
    uint64_t field_count;
} tapirscan_content_summary;

/* Byte lengths, excluding the terminating NUL. */
typedef struct tapirscan_content_lengths {
    uint64_t name_length;
    uint64_t value_length;
} tapirscan_content_lengths;

/* Parts for tapirscan_content_copy. */
#define TAPIRSCAN_CONTENT_FIELD_NAME 0u
#define TAPIRSCAN_CONTENT_FIELD_VALUE 1u

/* Static NUL-terminated name such as "WiFi", or NULL for an unknown kind. */
const char *tapirscan_content_kind_name(uint32_t kind);
/* gs1 is the barcode's tri-state flag; only 1 parses the text as a GS1 element
   string. Text without recognized structure succeeds with *out == 0, which
   needs no destroy. Invalid UTF-8 returns INVALID_ARGUMENT. */
int32_t tapirscan_parse_content(const uint8_t *text, uint64_t length, int32_t gs1,
    tapirscan_content *out, tapirscan_error *error);
int32_t tapirscan_content_info(tapirscan_content content, tapirscan_content_summary *out);
/* index is 0-based, below field_count. */
int32_t tapirscan_content_field(tapirscan_content content, uint64_t index, tapirscan_content_lengths *out);
/* Copy a field's name or value plus a terminating NUL; capacity must exceed its length. */
int32_t tapirscan_content_copy(tapirscan_content content, uint64_t index, uint32_t part,
    uint8_t *out, uint64_t capacity);
int32_t tapirscan_content_destroy(tapirscan_content content);
#ifdef __cplusplus
}
#endif
#endif

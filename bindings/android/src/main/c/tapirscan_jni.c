/* JNI bridge from org.tapirscan.android.Scanner and Content to the Tapirscan C library. */
#include <jni.h>
#include <stdlib.h>
#include <string.h>
#include <tapirscan.h>

/* Text crosses as UTF-8 byte[]: JNI's NewStringUTF expects modified UTF-8. */
static jbyteArray utf8_bytes(JNIEnv *env, const char *text, size_t length) {
    jbyteArray bytes = (*env)->NewByteArray(env, (jsize)length);
    if (bytes) (*env)->SetByteArrayRegion(env, bytes, 0, (jsize)length, (const jbyte *)text);
    return bytes;
}

static void throw_error(JNIEnv *env, int32_t status, const char *message) {
    if ((*env)->ExceptionCheck(env)) return;
    const char *text = message && *message ? message : tapirscan_status_message(status);
    jclass type = (*env)->FindClass(env, "org/tapirscan/android/ScannerException");
    jmethodID init = type ? (*env)->GetMethodID(env, type, "<init>", "(I[B)V") : NULL;
    jbyteArray bytes = init ? utf8_bytes(env, text, strlen(text)) : NULL;
    jobject error = bytes ? (*env)->NewObject(env, type, init, (jint)status, bytes) : NULL;
    if (error) (*env)->Throw(env, (jthrowable)error);
}

JNIEXPORT jlong JNICALL Java_org_tapirscan_android_Scanner_nativeCreate(
    JNIEnv *env, jclass type, jint mode, jint formats, jint add_on_policy, jint parallel) {
    (void)type;
    tapirscan_scanner_options options = TAPIRSCAN_SCANNER_OPTIONS_INIT;
    options.mode = (uint32_t)mode;
    options.formats = (uint32_t)formats;
    options.ean_add_on_policy = (uint32_t)add_on_policy;
    options.parallel = (uint32_t)parallel;
    tapirscan_scanner scanner = 0;
    tapirscan_error error = {{0}};
    int32_t status = tapirscan_scanner_create(&options, &scanner, &error);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, error.message);
        return 0;
    }
    return (jlong)scanner;
}

JNIEXPORT void JNICALL Java_org_tapirscan_android_Scanner_nativeDestroy(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env;
    (void)type;
    tapirscan_scanner_destroy((tapirscan_scanner)handle);
}

/* Copy one variable-length barcode field as UTF-8 bytes. NULL without an exception when the
   field is absent; NULL with a pending exception on failure. */
static jbyteArray field_bytes(JNIEnv *env, tapirscan_result result, uint64_t index, uint32_t field,
                              uint64_t length) {
    if (length == TAPIRSCAN_ABSENT) return NULL;
    char *buffer = malloc(length + 1);
    int32_t status = buffer ? tapirscan_result_copy(result, index, field, (uint8_t *)buffer, length + 1)
                            : TAPIRSCAN_INTERNAL_ERROR;
    jbyteArray bytes = status == TAPIRSCAN_OK ? utf8_bytes(env, buffer, length) : NULL;
    free(buffer);
    if (status != TAPIRSCAN_OK) throw_error(env, status, NULL);
    return bytes;
}

/* One result entry as a Barcode, or NULL with a pending exception. */
static jobject barcode_object(JNIEnv *env, jclass type, jmethodID init, tapirscan_result result,
                              uint64_t index) {
    tapirscan_barcode barcode;
    int32_t status = tapirscan_result_barcode(result, index, &barcode);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, NULL);
        return NULL;
    }
    jdouble corners[8];
    for (int c = 0; c < 4; c++) {
        corners[2 * c] = barcode.polygon[c].x;
        corners[2 * c + 1] = barcode.polygon[c].y;
    }
    jbyteArray text = field_bytes(env, result, index, TAPIRSCAN_FIELD_TEXT, barcode.text_length);
    jbyteArray payload = !text ? NULL
        : field_bytes(env, result, index, TAPIRSCAN_FIELD_PAYLOAD_BYTES, barcode.payload_bytes_length);
    jbyteArray add_on = (!text || (*env)->ExceptionCheck(env)) ? NULL
        : field_bytes(env, result, index, TAPIRSCAN_FIELD_EAN_ADD_ON, barcode.ean_add_on_length);
    jbyteArray append_id = (!text || (*env)->ExceptionCheck(env)) ? NULL
        : field_bytes(env, result, index, TAPIRSCAN_FIELD_STRUCTURED_APPEND_ID,
                      barcode.structured_append_id_length);
    jdoubleArray polygon = (!text || (*env)->ExceptionCheck(env)) ? NULL
        : (*env)->NewDoubleArray(env, 8);
    jobject value = NULL;
    if (polygon) {
        (*env)->SetDoubleArrayRegion(env, polygon, 0, 8, corners);
        value = (*env)->NewObject(env, type, init, text, (jint)barcode.format, polygon,
                                  (jlong)barcode.support, payload, add_on, (jint)barcode.gs1,
                                  (jint)barcode.reader_initialization,
                                  (jlong)barcode.structured_append_index,
                                  (jlong)barcode.structured_append_count, append_id,
                                  (jint)barcode.structured_append_parity);
    }
    if (text) (*env)->DeleteLocalRef(env, text);
    if (payload) (*env)->DeleteLocalRef(env, payload);
    if (add_on) (*env)->DeleteLocalRef(env, add_on);
    if (append_id) (*env)->DeleteLocalRef(env, append_id);
    if (polygon) (*env)->DeleteLocalRef(env, polygon);
    return value;
}

/* Every result barcode as a Barcode[]; NULL with a pending exception on failure. */
static jobjectArray barcode_array(JNIEnv *env, tapirscan_result result) {
    uint64_t count = 0;
    int32_t status = tapirscan_result_count(result, &count);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, NULL);
        return NULL;
    }
    jclass type = (*env)->FindClass(env, "org/tapirscan/android/Barcode");
    jmethodID init = type ? (*env)->GetMethodID(env, type, "<init>", "([BI[DJ[B[BIIJJ[BI)V") : NULL;
    jobjectArray out = init ? (*env)->NewObjectArray(env, (jsize)count, type, NULL) : NULL;
    for (uint64_t i = 0; out && i < count; i++) {
        jobject value = barcode_object(env, type, init, result, i);
        if (!value) break;
        (*env)->SetObjectArrayElement(env, out, (jsize)i, value);
        (*env)->DeleteLocalRef(env, value);
    }
    if (type) (*env)->DeleteLocalRef(env, type);
    return (*env)->ExceptionCheck(env) ? NULL : out;
}

/* Undecoded regions as eight polygon coordinates and the format bit each; NULL with a pending
   exception on failure. */
static jdoubleArray region_array(JNIEnv *env, tapirscan_result result, uint64_t count) {
    jdoubleArray out = (*env)->NewDoubleArray(env, (jsize)(9 * count));
    for (uint64_t i = 0; out && i < count; i++) {
        tapirscan_region region;
        int32_t status = tapirscan_result_undecoded(result, i, &region);
        if (status != TAPIRSCAN_OK) {
            throw_error(env, status, NULL);
            return NULL;
        }
        jdouble values[9];
        for (int c = 0; c < 4; c++) {
            values[2 * c] = region.polygon[c].x;
            values[2 * c + 1] = region.polygon[c].y;
        }
        values[8] = (jdouble)region.format;
        (*env)->SetDoubleArrayRegion(env, out, (jsize)(9 * i), 9, values);
    }
    return out;
}

/* The inspection payload after the barcodes: regions, summary and diagnostics JSON. Stores them
   in out[1..3]; leaves a pending exception on failure. */
static void fill_inspection(JNIEnv *env, tapirscan_result result, jobjectArray out) {
    tapirscan_summary summary;
    int32_t status = tapirscan_result_info(result, &summary);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, NULL);
        return;
    }
    jdoubleArray regions = region_array(env, result, summary.undecoded_count);
    if (!regions) return;
    (*env)->SetObjectArrayElement(env, out, 1, regions);
    jdouble values[4] = {(jdouble)summary.width, (jdouble)summary.height, (jdouble)summary.mode,
                         summary.elapsed_ms};
    jdoubleArray info = (*env)->NewDoubleArray(env, 4);
    if (!info) return;
    (*env)->SetDoubleArrayRegion(env, info, 0, 4, values);
    (*env)->SetObjectArrayElement(env, out, 2, info);
    uint64_t length = 0;
    status = tapirscan_result_json_length(result, &length);
    char *json = status == TAPIRSCAN_OK ? malloc(length + 1) : NULL;
    if (status == TAPIRSCAN_OK && !json) status = TAPIRSCAN_INTERNAL_ERROR;
    if (json) status = tapirscan_result_copy_json(result, (uint8_t *)json, length + 1);
    jbyteArray bytes = status == TAPIRSCAN_OK ? utf8_bytes(env, json, length) : NULL;
    free(json);
    if (status != TAPIRSCAN_OK) throw_error(env, status, NULL);
    if (bytes) (*env)->SetObjectArrayElement(env, out, 3, bytes);
}

/* Scan or inspect and convert the result: {Barcode[]} or {Barcode[], double[], double[], byte[]};
   NULL with a pending exception on failure. */
static jobjectArray scan(JNIEnv *env, jlong handle, const uint8_t *data, uint64_t length,
                         jint width, jint height, jint channels, jint stride, jint formats,
                         jboolean inspect) {
    if (width < 0 || height < 0 || channels < 0 || stride < 0) {
        throw_error(env, TAPIRSCAN_INVALID_ARGUMENT, "dimensions must not be negative");
        return NULL;
    }
    tapirscan_image image = {data, length, (uint64_t)width, (uint64_t)height, (uint32_t)channels,
                             (uint64_t)stride};
    tapirscan_scan_options options = {(uint32_t)formats};
    tapirscan_result result = 0;
    tapirscan_error error = {{0}};
    int32_t status = inspect
        ? tapirscan_inspect((tapirscan_scanner)handle, &image, &options, &result, &error)
        : tapirscan_scan((tapirscan_scanner)handle, &image, &options, &result, &error);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, error.message);
        return NULL;
    }
    jclass object = (*env)->FindClass(env, "java/lang/Object");
    jobjectArray out = object ? (*env)->NewObjectArray(env, inspect ? 4 : 1, object, NULL) : NULL;
    jobjectArray barcodes = out ? barcode_array(env, result) : NULL;
    if (barcodes) {
        (*env)->SetObjectArrayElement(env, out, 0, barcodes);
        if (inspect) fill_inspection(env, result, out);
    }
    tapirscan_result_destroy(result);
    if (object) (*env)->DeleteLocalRef(env, object);
    return (*env)->ExceptionCheck(env) ? NULL : out;
}

JNIEXPORT jobjectArray JNICALL Java_org_tapirscan_android_Scanner_nativeScanDirect(
    JNIEnv *env, jclass type, jlong handle, jobject buffer, jint offset, jint length, jint width,
    jint height, jint channels, jint stride, jint formats, jboolean inspect) {
    (void)type;
    uint8_t *base = (*env)->GetDirectBufferAddress(env, buffer);
    if (!base || offset < 0 || length < 0) {
        throw_error(env, TAPIRSCAN_INVALID_ARGUMENT, "expected a direct buffer");
        return NULL;
    }
    return scan(env, handle, base + offset, (uint64_t)length, width, height, channels, stride,
                formats, inspect);
}

JNIEXPORT jobjectArray JNICALL Java_org_tapirscan_android_Scanner_nativeScanArray(
    JNIEnv *env, jclass type, jlong handle, jbyteArray pixels, jint width, jint height,
    jint channels, jint stride, jint formats, jboolean inspect) {
    (void)type;
    jsize length = (*env)->GetArrayLength(env, pixels);
    jbyte *data = (*env)->GetByteArrayElements(env, pixels, NULL);
    if (!data) return NULL;
    jobjectArray out = scan(env, handle, (const uint8_t *)data, (uint64_t)length, width, height,
                            channels, stride, formats, inspect);
    (*env)->ReleaseByteArrayElements(env, pixels, data, JNI_ABORT);
    return out;
}

/* Copy one content field part as UTF-8 bytes; NULL with a pending exception on failure. */
static jbyteArray content_part(JNIEnv *env, tapirscan_content content, uint64_t index, uint32_t part,
                               uint64_t length) {
    char *buffer = malloc(length + 1);
    int32_t status = buffer ? tapirscan_content_copy(content, index, part, (uint8_t *)buffer, length + 1)
                            : TAPIRSCAN_INTERNAL_ERROR;
    jbyteArray bytes = status == TAPIRSCAN_OK ? utf8_bytes(env, buffer, length) : NULL;
    free(buffer);
    if (status != TAPIRSCAN_OK) throw_error(env, status, NULL);
    return bytes;
}

/* Parsed content as UTF-8 byte arrays: the kind name, then name and value of each field.
   NULL without an exception when the text has no recognized structure. */
JNIEXPORT jobjectArray JNICALL Java_org_tapirscan_android_Content_nativeParse(
    JNIEnv *env, jclass type, jbyteArray text, jboolean gs1) {
    (void)type;
    jsize length = (*env)->GetArrayLength(env, text);
    jbyte *data = (*env)->GetByteArrayElements(env, text, NULL);
    if (!data) return NULL;
    tapirscan_content content = 0;
    tapirscan_error error = {{0}};
    int32_t status = tapirscan_parse_content((const uint8_t *)data, (uint64_t)length, gs1 ? 1 : 0,
                                             &content, &error);
    (*env)->ReleaseByteArrayElements(env, text, data, JNI_ABORT);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, error.message);
        return NULL;
    }
    if (content == 0) return NULL;
    tapirscan_content_summary summary;
    status = tapirscan_content_info(content, &summary);
    jclass bytes_type = status == TAPIRSCAN_OK ? (*env)->FindClass(env, "[B") : NULL;
    jobjectArray out = bytes_type
        ? (*env)->NewObjectArray(env, (jsize)(1 + 2 * summary.field_count), bytes_type, NULL)
        : NULL;
    if (out) {
        const char *kind = tapirscan_content_kind_name(summary.kind);
        jbyteArray name = utf8_bytes(env, kind, strlen(kind));
        if (name) {
            (*env)->SetObjectArrayElement(env, out, 0, name);
            (*env)->DeleteLocalRef(env, name);
        }
    }
    for (uint64_t i = 0; out && !(*env)->ExceptionCheck(env) && i < summary.field_count; i++) {
        tapirscan_content_lengths lengths;
        status = tapirscan_content_field(content, i, &lengths);
        if (status != TAPIRSCAN_OK) break;
        jbyteArray name = content_part(env, content, i, TAPIRSCAN_CONTENT_FIELD_NAME, lengths.name_length);
        jbyteArray value = name ? content_part(env, content, i, TAPIRSCAN_CONTENT_FIELD_VALUE,
                                               lengths.value_length)
                                : NULL;
        if (value) {
            (*env)->SetObjectArrayElement(env, out, (jsize)(1 + 2 * i), name);
            (*env)->SetObjectArrayElement(env, out, (jsize)(2 + 2 * i), value);
        }
        if (name) (*env)->DeleteLocalRef(env, name);
        if (value) (*env)->DeleteLocalRef(env, value);
    }
    tapirscan_content_destroy(content);
    if (bytes_type) (*env)->DeleteLocalRef(env, bytes_type);
    if (status != TAPIRSCAN_OK) throw_error(env, status, NULL);
    return (*env)->ExceptionCheck(env) ? NULL : out;
}

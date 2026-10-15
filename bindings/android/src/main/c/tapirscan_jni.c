/* JNI bridge from org.tapirscan.android.BarcodeScanner to the Tapirscan C library. */
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
    jclass type = (*env)->FindClass(env, "org/tapirscan/android/TapirscanException");
    jmethodID init = type ? (*env)->GetMethodID(env, type, "<init>", "(I[B)V") : NULL;
    jbyteArray bytes = init ? utf8_bytes(env, text, strlen(text)) : NULL;
    jobject error = bytes ? (*env)->NewObject(env, type, init, (jint)status, bytes) : NULL;
    if (error) (*env)->Throw(env, (jthrowable)error);
}

JNIEXPORT jlong JNICALL Java_org_tapirscan_android_BarcodeScanner_nativeCreate(
    JNIEnv *env, jclass type, jint mode, jint formats) {
    (void)type;
    tapirscan_scanner_options options = TAPIRSCAN_SCANNER_OPTIONS_INIT;
    options.mode = (uint32_t)mode;
    options.formats = (uint32_t)formats;
    tapirscan_scanner scanner = 0;
    tapirscan_error error = {{0}};
    int32_t status = tapirscan_scanner_create(&options, &scanner, &error);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, error.message);
        return 0;
    }
    return (jlong)scanner;
}

JNIEXPORT void JNICALL Java_org_tapirscan_android_BarcodeScanner_nativeDestroy(
    JNIEnv *env, jclass type, jlong handle) {
    (void)env;
    (void)type;
    tapirscan_scanner_destroy((tapirscan_scanner)handle);
}

/* One result entry as a Barcode, or NULL with a pending exception. */
static jobject barcode_object(JNIEnv *env, jclass type, jmethodID init, tapirscan_result result,
                              uint64_t index) {
    tapirscan_barcode barcode;
    int32_t status = tapirscan_result_barcode(result, index, &barcode);
    char *text = status == TAPIRSCAN_OK ? malloc(barcode.text_length + 1) : NULL;
    if (status == TAPIRSCAN_OK && !text) status = TAPIRSCAN_INTERNAL_ERROR;
    if (text) {
        status = tapirscan_result_copy(result, index, TAPIRSCAN_FIELD_TEXT, (uint8_t *)text,
                                       barcode.text_length + 1);
    }
    if (status != TAPIRSCAN_OK) {
        free(text);
        throw_error(env, status, NULL);
        return NULL;
    }
    jdouble corners[8];
    for (int c = 0; c < 4; c++) {
        corners[2 * c] = barcode.polygon[c].x;
        corners[2 * c + 1] = barcode.polygon[c].y;
    }
    jbyteArray bytes = utf8_bytes(env, text, barcode.text_length);
    free(text);
    jstring format = bytes ? (*env)->NewStringUTF(env, tapirscan_format_name(barcode.format)) : NULL;
    jdoubleArray polygon = format ? (*env)->NewDoubleArray(env, 8) : NULL;
    jobject value = NULL;
    if (polygon) {
        (*env)->SetDoubleArrayRegion(env, polygon, 0, 8, corners);
        value = (*env)->NewObject(env, type, init, bytes, format, polygon, (jlong)barcode.support,
                                  (jint)barcode.gs1);
    }
    if (bytes) (*env)->DeleteLocalRef(env, bytes);
    if (format) (*env)->DeleteLocalRef(env, format);
    if (polygon) (*env)->DeleteLocalRef(env, polygon);
    return value;
}

/* Scan and convert the result into Barcode objects; NULL with a pending exception on failure. */
static jobjectArray scan(JNIEnv *env, jlong handle, const uint8_t *data, uint64_t length,
                         jint width, jint height, jint channels, jint stride) {
    if (width < 0 || height < 0 || channels < 0 || stride < 0) {
        throw_error(env, TAPIRSCAN_INVALID_ARGUMENT, "dimensions must not be negative");
        return NULL;
    }
    tapirscan_image image = {data, length, (uint64_t)width, (uint64_t)height, (uint32_t)channels,
                             (uint64_t)stride};
    tapirscan_result result = 0;
    tapirscan_error error = {{0}};
    int32_t status = tapirscan_scan((tapirscan_scanner)handle, &image, NULL, &result, &error);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, error.message);
        return NULL;
    }
    uint64_t count = 0;
    tapirscan_result_count(result, &count);
    jclass type = (*env)->FindClass(env, "org/tapirscan/android/Barcode");
    jmethodID init = type ? (*env)->GetMethodID(env, type, "<init>", "([BLjava/lang/String;[DJI)V")
                          : NULL;
    jobjectArray out = init ? (*env)->NewObjectArray(env, (jsize)count, type, NULL) : NULL;
    for (uint64_t i = 0; out && i < count; i++) {
        jobject value = barcode_object(env, type, init, result, i);
        if (!value) break;
        (*env)->SetObjectArrayElement(env, out, (jsize)i, value);
        (*env)->DeleteLocalRef(env, value);
    }
    tapirscan_result_destroy(result);
    if (type) (*env)->DeleteLocalRef(env, type);
    return (*env)->ExceptionCheck(env) ? NULL : out;
}

JNIEXPORT jobjectArray JNICALL Java_org_tapirscan_android_BarcodeScanner_nativeScanDirect(
    JNIEnv *env, jclass type, jlong handle, jobject buffer, jint offset, jint length, jint width,
    jint height, jint channels, jint stride) {
    (void)type;
    uint8_t *base = (*env)->GetDirectBufferAddress(env, buffer);
    if (!base || offset < 0 || length < 0) {
        throw_error(env, TAPIRSCAN_INVALID_ARGUMENT, "expected a direct buffer");
        return NULL;
    }
    return scan(env, handle, base + offset, (uint64_t)length, width, height, channels, stride);
}

JNIEXPORT jobjectArray JNICALL Java_org_tapirscan_android_BarcodeScanner_nativeScanArray(
    JNIEnv *env, jclass type, jlong handle, jbyteArray pixels, jint width, jint height,
    jint channels, jint stride) {
    (void)type;
    jsize length = (*env)->GetArrayLength(env, pixels);
    jbyte *data = (*env)->GetByteArrayElements(env, pixels, NULL);
    if (!data) return NULL;
    jobjectArray out = scan(env, handle, (const uint8_t *)data, (uint64_t)length, width, height,
                            channels, stride);
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

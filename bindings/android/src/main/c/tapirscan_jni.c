/* JNI bridge from org.tapirscan.android.BarcodeScanner to the Tapirscan C library. */
#include <jni.h>
#include <stdlib.h>
#include <string.h>
#include <tapirscan.h>

static void throw_error(JNIEnv *env, int32_t status, const char *message) {
    jclass type = (*env)->FindClass(env, "org/tapirscan/android/TapirscanException");
    if (!type) return;
    jmethodID init = (*env)->GetMethodID(env, type, "<init>", "(ILjava/lang/String;)V");
    jstring text = (*env)->NewStringUTF(env, message && *message ? message : tapirscan_status_message(status));
    jobject error = (*env)->NewObject(env, type, init, (jint)status, text);
    if (error) (*env)->Throw(env, (jthrowable)error);
}

JNIEXPORT jlong JNICALL Java_org_tapirscan_android_BarcodeScanner_nativeCreate(
    JNIEnv *env, jclass type, jint mode, jint formats) {
    (void)type;
    tapirscan_scanner_options options = TAPIRSCAN_SCANNER_OPTIONS_INIT;
    options.mode = (uint32_t)mode;
    options.formats = (uint32_t)formats;
    tapirscan_scanner scanner = 0;
    tapirscan_error error;
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

/* Scan and convert the result into Barcode objects; NULL with a pending exception on failure. */
static jobjectArray scan(JNIEnv *env, jlong handle, const uint8_t *data, uint64_t length,
                         jint width, jint height, jint channels, jint stride) {
    if (width < 0 || height < 0 || channels < 0 || stride < 0) {
        throw_error(env, TAPIRSCAN_INVALID_ARGUMENT, "dimensions must not be negative");
        return NULL;
    }
    tapirscan_image image = {data, length, (uint64_t)width, (uint64_t)height, (uint32_t)channels,
                             (uint64_t)stride};
    tapirscan_result result;
    tapirscan_error error;
    int32_t status = tapirscan_scan((tapirscan_scanner)handle, &image, NULL, &result, &error);
    if (status != TAPIRSCAN_OK) {
        throw_error(env, status, error.message);
        return NULL;
    }
    uint64_t count = 0;
    tapirscan_result_count(result, &count);
    jclass type = (*env)->FindClass(env, "org/tapirscan/android/Barcode");
    jmethodID init = type ? (*env)->GetMethodID(env, type, "<init>", "(Ljava/lang/String;Ljava/lang/String;[DJ)V") : NULL;
    jobjectArray out = init ? (*env)->NewObjectArray(env, (jsize)count, type, NULL) : NULL;
    for (uint64_t i = 0; out && i < count; i++) {
        tapirscan_barcode barcode;
        if (tapirscan_result_barcode(result, i, &barcode) != TAPIRSCAN_OK) continue;
        char *text = malloc(barcode.text_length + 1);
        if (!text) break;
        tapirscan_result_copy(result, i, TAPIRSCAN_FIELD_TEXT, (uint8_t *)text, barcode.text_length + 1);
        jdouble corners[8];
        for (int c = 0; c < 4; c++) {
            corners[2 * c] = barcode.polygon[c].x;
            corners[2 * c + 1] = barcode.polygon[c].y;
        }
        jdoubleArray polygon = (*env)->NewDoubleArray(env, 8);
        if (polygon) (*env)->SetDoubleArrayRegion(env, polygon, 0, 8, corners);
        jstring jtext = (*env)->NewStringUTF(env, text);
        jstring format = (*env)->NewStringUTF(env, tapirscan_format_name(barcode.format));
        free(text);
        if (!polygon || !jtext || !format) break;
        jobject value = (*env)->NewObject(env, type, init, jtext, format, polygon, (jlong)barcode.support);
        if (!value) break;
        (*env)->SetObjectArrayElement(env, out, (jsize)i, value);
        (*env)->DeleteLocalRef(env, value);
        (*env)->DeleteLocalRef(env, polygon);
        (*env)->DeleteLocalRef(env, jtext);
        (*env)->DeleteLocalRef(env, format);
    }
    tapirscan_result_destroy(result);
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

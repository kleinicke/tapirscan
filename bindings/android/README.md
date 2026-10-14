# Tapirscan for Android

A small Java API over the Tapirscan C library through JNI, usable from Kotlin and
Java on Android (API 24+, 64-bit ABIs arm64-v8a and x86_64). It scans camera planes
and pixel buffers and returns every decoded barcode with its text, format and
source-image polygon. (The JVM binding in `bindings/java` needs the Foreign Function
& Memory API, which Android lacks.)

## Setup

Build the AAR with the [Android NDK](https://developer.android.com/ndk/downloads):

```sh
ANDROID_NDK_HOME=/path/to/ndk python3 scripts/build_android.py
# writes build/android/tapirscan-android.aar
```

Add it to your app module:

```kotlin
dependencies {
    implementation(files("libs/tapirscan-android.aar"))
}
```

## CameraX

Scan the luma (Y) plane of each `ImageAnalysis` frame in place. Reuse one scanner,
and keep CameraX's default `STRATEGY_KEEP_ONLY_LATEST` so a slow scan skips frames:

```kotlin
val scanner = BarcodeScanner(BarcodeScanner.MODE_LOW, Formats.EAN13 or Formats.QR_CODE)

val analysis = ImageAnalysis.Builder()
    .setTargetResolution(Size(1280, 720))
    .build()
analysis.setAnalyzer(executor) { image ->
    val luma = image.planes[0]
    val barcodes = scanner.scan(luma.buffer, image.width, image.height, 1, luma.rowStride)
    image.close()
    if (barcodes.isNotEmpty()) runOnUiThread { /* show barcodes */ }
}
// Call scanner.close() when the camera is unbound.
```

## Other inputs

`scan(pixels, width, height, channels, stride)` takes a direct `ByteBuffer` (read in
place, starting at its position) or a `byte[]`, with 1 (gray), 3 (RGB) or 4 (RGBA,
alpha ignored) channels; `stride` 0 means packed rows. For a `Bitmap`, copy its
pixels with `copyPixelsToBuffer` into a reused direct buffer (`ARGB_8888` bitmaps
store RGBA bytes) and scan with 4 channels. Errors are thrown as
`TapirscanException` with the library's message.

## Fast scanning

- Select only the formats you need; each enabled 2D format searches the whole image.
- Use `MODE_LOW` and 720p or 1080p frames for live scanning; reuse one scanner.
- The camera's Y plane is the cheapest input: no copy and no color conversion.

See [fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md).

## Testing

`python3 scripts/test_android_jni.py` builds the JNI bridge for the desktop JVM and
runs its test; it needs only a JDK and the native library
(`python3 scripts/build_native.py`).

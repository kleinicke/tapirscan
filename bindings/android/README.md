# Tapirscan for Android

> **Preview:** built from source; the API may change in minor releases.

A small Java API over the Tapirscan C library through JNI, usable from Kotlin and
Java on Android (API 24+, 64-bit ABIs arm64-v8a and x86_64). It follows the shape of
the [Java binding](../java/README.md): `scan` returns a `ScanResult` with `values()`,
`barcodes()` and `best()`, and `inspect` returns an `InspectionResult` that adds
undecoded regions, image size, mode, elapsed time and diagnostics. Shared semantics
(support, undecoded regions, geometry, image limits) are in
[API design](../../docs/API_DESIGN.md). (The JVM binding in `bindings/java` needs the
Foreign Function & Memory API, which Android lacks.)

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

## Scan

```kotlin
val options = ScannerOptions.defaults()
    .withMode(Mode.LOW)
    .withFormats(setOf(Format.EAN13, Format.QR_CODE))
Scanner(options).use { scanner ->
    val result = scanner.scan(pixels, width, height, 1, 0)   // ByteBuffer or ByteArray
    for (barcode in result.barcodes()) println("${barcode.format()} ${barcode.text()} ${barcode.rect()}")
}
```

Calls block the calling thread. CameraX analyzers already run on an executor;
elsewhere in Kotlin, move a scan off the main thread with a coroutine:

```kotlin
val result = withContext(Dispatchers.Default) { scanner.scan(pixels, width, height, 4, 0) }
```

`ScannerOptions` holds the `Mode` (default `MEDIUM`), a `Set<Format>` (default
`Format.RETAIL`), the `EanAddOnPolicy` (default `IGNORE`) and `parallel`, fixed when
the scanner is created. `withParallel(false)` keeps each scan on the calling thread,
for apps that already scan on several threads; unset, the
[process default](../../docs/PERFORMANCE.md#threads-and-servers) applies. `Format` is generated from `config/formats.json` with the presets
`RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX` and `ALL`. A per-call
`ScanOptions.defaults().withFormats(...)` overrides the scanner's formats for that
call only.

For example, `ScannerOptions.defaults().withFormats(Format.COMMON)` selects the common group.
See the [format and keyword-group matrix](../../docs/FORMATS.md#format-matrix)
for group membership and opt-in formats.

Each `Barcode` has `text()`, `format()`, `polygon()`, `rect()`, `support()`,
`payloadBytes()`, `eanAddOn()`, `gs1()`, `readerInitialization()` and
`structuredAppend()`; optional fields are `Optional`s, empty when unavailable.
The class is named `Scanner` like its Java counterpart; import it by name rather
than with `java.util.*`.

## CameraX

Scan the luma (Y) plane of each `ImageAnalysis` frame in place. Reuse one scanner,
and keep CameraX's default `STRATEGY_KEEP_ONLY_LATEST` so a slow scan skips frames:

```kotlin
val scanner = Scanner(
    ScannerOptions.defaults().withMode(Mode.LOW).withFormats(setOf(Format.EAN13, Format.QR_CODE))
)

val analysis = ImageAnalysis.Builder()
    .setResolutionSelector(
        ResolutionSelector.Builder()
            .setResolutionStrategy(
                ResolutionStrategy(Size(1280, 720), ResolutionStrategy.FALLBACK_RULE_CLOSEST_HIGHER_THEN_LOWER)
            )
            .build()
    )
    .build()
analysis.setAnalyzer(executor) { image ->
    val luma = image.planes[0]
    val result = scanner.scan(luma.buffer, image.width, image.height, 1, luma.rowStride)
    image.close()
    if (result.barcodes().isNotEmpty()) runOnUiThread { /* show result.barcodes() */ }
}
// Call scanner.close() when the camera is unbound.
```

## Inspection

```java
InspectionResult inspection = scanner.inspect(luma.getBuffer(), width, height, 1, luma.getRowStride());
inspection.undecoded();    // localized regions without a decode
inspection.elapsedMs();
inspection.diagnostics();  // unstable engine JSON
```

## Errors

Calls throw `ScannerException`. Its `code` is the C status and `codeName()` is the
name shared with the Python and JavaScript bindings: `"invalid_input"`, `"disposed"`
or `"engine"`. Using a closed scanner throws `IllegalStateException`. No detection is
an empty result, not an error.

## Structured content

`Tapirscan.parseContent(barcode)` returns the named fields of GS1, Wi-Fi, contact,
payment and similar payloads as an `Optional<Content>`, empty for plain text;
`Tapirscan.parseContent(text)` and `parseContent(text, gs1)` parse any text.

```java
Tapirscan.parseContent(barcode).ifPresent(content -> {
    if (content.kind() == ContentKind.WIFI) {
        connect(content.get("ssid").orElse(""), content.get("password").orElse(""));
    }
});
```

`Content.getAll(name)` returns every value of a repeated field. Kinds and fields:
[structured content](../../docs/CONTENT.md).

## Other inputs

`scan(pixels, width, height, channels, stride)` takes a direct `ByteBuffer` (read in
place, starting at its position) or a `byte[]`, with 1 (gray), 3 (RGB) or 4 (RGBA,
alpha ignored) channels; `stride` 0 means packed rows. For a `Bitmap`, copy its
pixels with `copyPixelsToBuffer` into a reused direct buffer (`ARGB_8888` bitmaps
store RGBA bytes) and scan with 4 channels.

## Fast scanning

- Select only the formats you need; each enabled 2D format searches the whole image.
- Use `Mode.LOW` and 720p or 1080p frames for live scanning; reuse one scanner.
- The camera's Y plane is the cheapest camera input: it is read in place, with no
  color conversion on your side. Requesting RGBA frames from CameraX adds a
  conversion per frame.
- Release builds need no extra R8 rules; the AAR ships its own.

See [fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md).

## Testing

`python3 scripts/test_android_jni.py` builds the JNI bridge for the desktop JVM and
runs its test; it needs only a JDK and the native library
(`python3 scripts/build_native.py`).

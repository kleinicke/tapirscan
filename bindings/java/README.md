# Tapirscan for Java

`scan(image)` returns `ScanResult` with decoded text, format
and source-image polygons. Use `result.values()` for strings and
`result.barcodes()` for located reads; both are empty when nothing was decoded.

```java
var result = Tapirscan.scan(Image.gray(pixels, width, height));
for (var barcode : result.barcodes()) System.out.println(barcode.text());
```

Reuse `Scanner.scan` across images. Call `inspect` for an `InspectionResult`
with unread regions, timing and diagnostics.

Defaults are Medium effort and retail formats (EAN13, UPCA, EAN8 and UPCE). The
dependency-free binding needs JDK 22 or newer, where the foreign function and
memory API (FFM) is final, and uses one native library that contains all four
effort modes.

```java
import org.tapirscan.*;

byte[] pixels = new byte[640 * 480];
java.util.Arrays.fill(pixels, (byte) 255);
InspectionResult result = Tapirscan.inspect(Image.gray(pixels, 640, 480));
for (Barcode barcode : result.barcodes()) {
    System.out.println(barcode.text() + " " + barcode.format() + " " + barcode.polygon());
}
System.out.println(result.undecoded().size() + " undecoded");
```

If nothing is decoded, `barcodes()` is empty. Java-side validation can throw
`IllegalArgumentException`; scanning after close throws `IllegalStateException`.
Native validation and engine failures throw `ScannerException`, whose `code` is
the native status. Results are immutable records that outlive the scanner.
Support, undecoded regions and duplicate payloads are described in
[API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md).

## Reuse and configuration

```java
ScannerOptions options = ScannerOptions.defaults()
        .withMode(Mode.HIGH)
        .withFormats(java.util.Set.of(Format.EAN13, Format.QR_CODE));
try (Scanner scanner = new Scanner(options)) {
    var result = scanner.scan(Image.rgba(pixels, width, height),
            ScanOptions.defaults().withFormats(java.util.Set.of(Format.QR_CODE)));
    result.best().ifPresent(best -> System.out.println(best.text()));
}
```

`new Scanner()` uses the defaults. Reuse a scanner across images and close it,
or use try-with-resources. Scans on one scanner serialize; separate scanners run
concurrently.
`Tapirscan.inspect(image, options)` uses a temporary scanner for one image.

| Scanner option   | Default                  | Choices                                  |
| ---------------- | ------------------------ | ---------------------------------------- |
| `mode`           | `Mode.MEDIUM`            | `LOW`, `MEDIUM`, `HIGH`, `VERY_HIGH`     |
| `formats`        | `Format.RETAIL`          | Any nonempty `Set<Format>`, or a preset  |
| `eanAddOnPolicy` | `EanAddOnPolicy.IGNORE`  | `IGNORE`, `READ`, `REQUIRE`              |
| `parallel`       | `null` (process default) | `true`, `false`; set with `withParallel` |

`parallel` runs one scan's formats on separate threads where the host allows it;
results are identical either way. `null` is on unless the process environment sets
`TAPIRSCAN_THREADS` to `0` or `1`.

Presets: `Format.RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX` and
`ALL`. Set the supplement policy with `withEanAddOnPolicy`. See
[format and keyword-group matrix](https://github.com/kleinicke/tapirscan/blob/main/docs/FORMATS.md#format-matrix).

| Per-scan option | Default            | Meaning                                                |
| --------------- | ------------------ | ------------------------------------------------------ |
| `formats`       | `Optional.empty()` | Readers for this call, replacing the scanner's formats |

Per-call options never change the scanner's configuration.

## Results

`ScanResult` provides `barcodes()`, `values()` and `best()`. `best()` returns an
`Optional<Barcode>` holding the read with the largest `support` (first read wins
ties), empty when nothing was decoded.

`InspectionResult` has the same methods plus `undecoded()`, `width()`,
`height()`, `mode()`, `elapsedMs()` and `diagnostics()` (JSON text with an
unstable schema).

`Barcode` contains `text()`, `format()`, `polygon()` (four `Point`s in
source-image pixels, top-left origin), `support()`, and optional
`payloadBytes()`, `eanAddOn()`, `gs1()`, `readerInitialization()`,
`structuredAppend()` and `codabarStartStop()` (Codabar start and stop
characters such as `"AB"`, which `text()` omits). An empty optional means the
reader did not report it.
`rect()` returns `Rect(int left, int top, int width, int height)`. `Format.toString()`
gives names such as `"QRCode"`.

## Structured content

`Tapirscan.parseContent(barcode)` returns the named fields of GS1, Wi-Fi,
contact, payment and similar payloads; `parseContent(text)` and
`parseContent(text, gs1)` parse any text.

```java
Tapirscan.parseContent(barcode)
        .filter(content -> content.kind() == ContentKind.GS1)
        .flatMap(content -> content.get("01"))
        .ifPresent(gtin -> System.out.println("GTIN " + gtin));
```

Kinds and fields: [structured content](../../docs/CONTENT.md).

## Images

`Image.gray`, `Image.rgb` and `Image.rgba` take a byte array or a
`MemorySegment`, width and height; `.withStride(bytesPerRow)` describes padded
rows. Size limits and conversion rules are in
[API design](https://github.com/kleinicke/tapirscan/blob/main/docs/API_DESIGN.md#images).

Byte arrays are copied when scanning, limited to the addressed bytes, into a buffer the
scanner reuses until `close()`. Native segments are read in place without a copy, which
suits camera frames: wrap a direct `ByteBuffer` with `MemorySegment.ofBuffer(buffer)`.
Keep a native segment alive and unchanged until the scan returns. Pass the pixels
as you have them; packed RGBA is the cheapest input for the retail formats, so do
not convert it to gray for scanning.

## Fast scanning

Create one `Scanner` per thread and reuse it; `Tapirscan.scan` creates a scanner
per call. Select only the formats you need, since each enabled 2D format searches
the whole image, and use `Mode.LOW` for camera frames. Scans with several
formats also use extra threads internally; on servers that already run one scanner
per core, use `withParallel(false)`. Kotlin on the JVM uses the same API. See
[fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md).

## Building and loading the native library

```sh
python3 scripts/build_native.py
python3 scripts/build_java.py
java --enable-native-access=ALL-UNNAMED -Dtapirscan.library=build/native/libtapirscan.dylib \
    -cp build/java/tapirscan-1.5.0.jar:. MyApp
```

The library is located through the `tapirscan.library` system property (a
file), the `TAPIRSCAN_LIBRARY_DIR` environment variable (a directory), or the
operating system's library search path, in that order. If the library cannot be
found, creating the first scanner throws `IllegalArgumentException` from the
JDK's library lookup; an incompatible build throws `IllegalStateException`
("Native ABI mismatch" or a missing symbol). Rebuild the library to match.
`--enable-native-access=ALL-UNNAMED` avoids the JDK's native-access warning. The
JAR and the native library are distributed separately. This binding needs the
Foreign Function & Memory API, which Android lacks; Android apps use the
[Android AAR](../android/README.md), a JNI bridge with its own small API.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

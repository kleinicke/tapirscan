# Tapirscan for Java

`scan(image)` returns `ScanResult` with decoded text, format
and source-image polygons. Use `result.values()` for strings and
`result.barcodes()` for located reads; both are empty when nothing was decoded.

```java
var result = Tapirscan.scan(Image.gray(pixels, width, height));
for (var barcode : result.barcodes()) System.out.println(barcode.text());
```

Reuse `Scanner.scan` across images. Call `inspect` for an `InspectionResult` with unread
regions, work status, timing and diagnostics. There is no debug flag.

Both operations accept decoded pixels and return source-image barcode geometry.
Inspection also includes undecoded proposals and reported work limits. Defaults are Medium effort and
retail formats (EAN13, UPCA, EAN8 and UPCE). The dependency-free JDK 22+ binding
uses one native library that contains all four effort modes.

```java
import org.tapirscan.*;

byte[] pixels = new byte[640 * 480];
java.util.Arrays.fill(pixels, (byte) 255);
InspectionResult result = Tapirscan.inspect(Image.gray(pixels, 640, 480));
for (Barcode barcode : result.barcodes()) {
    System.out.println(barcode.text() + " " + barcode.format() + " " + barcode.polygon());
}
System.out.println(result.undecoded().size() + " undecoded; unfinished: " + result.unfinished());
```

No detection is an empty `barcodes()` list. Java-side validation can throw
`IllegalArgumentException`; scanning after close throws `IllegalStateException`.
Native validation and engine failures throw `ScannerException`, whose `code` is
the native status. Results are
immutable records that survive the scanner. Equal payloads at distinct
locations remain separate physical instances.

## Reuse and configuration

```java
ScannerOptions options = ScannerOptions.defaults()
        .withMode(Mode.HIGH)
        .withFormats(java.util.Set.of(Format.EAN13, Format.QR_CODE));
try (Scanner scanner = new Scanner(options)) {
    var result = scanner.scan(Image.rgba(pixels, width, height),
            ScanOptions.defaults().withExtendedBudget(true));
    result.best().ifPresent(best -> System.out.println(best.text()));
}
```

Reuse a scanner across images and close it, or use try-with-resources. Scans on
one scanner serialize; separate scanners run concurrently.
`Tapirscan.inspect(image, options)` uses a temporary scanner for one image.

| Scanner option   | Default                 | Choices                                 |
| ---------------- | ----------------------- | --------------------------------------- |
| `mode`           | `Mode.MEDIUM`           | `LOW`, `MEDIUM`, `HIGH`, `VERY_HIGH`    |
| `formats`        | `Format.RETAIL`         | Any nonempty `Set<Format>`, or a preset |
| `eanAddOnPolicy` | `EanAddOnPolicy.IGNORE` | `IGNORE`, `READ`, `REQUIRE`             |

Presets: `Format.RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX` and `ALL`.
Retail formats are enabled by default; additional formats are supported when selected. See
[format coverage](../../docs/FORMATS.md).

| Per-scan option  | Default            | Meaning                                |
| ---------------- | ------------------ | -------------------------------------- |
| `formats`        | `Optional.empty()` | Override readers for this call         |
| `extendedBudget` | `false`            | Allow extra reader work for any format |

`extendedBudget` can cost more time and does not promise exhaustive decoding;
see [API design](../../docs/API_DESIGN.md).

## Results

`ScanResult` provides decoded barcodes, `values()` and `best()`.
No detections produce empty collections; `best()` returns the language’s empty value.

`InspectionResult` exposes `barcodes()`, `undecoded()`, `width()`, `height()`,
`mode()`, `elapsedMs()`, `unfinished()` and `diagnostics()` JSON. `values()`
returns decoded text. `Tapirscan.best(barcodes)` returns the largest-support read
of any barcode list, keeping first-read ties; `InspectionResult.best()` is the same for
inspection results. Support is reader-specific and not comparable confidence
across formats.

`Barcode` contains `text()`, `format()`, `polygon()` (four `Point`s in
source-image pixels, top-left origin), `support()`, and optional
`payloadBytes()`, `eanAddOn()`, `gs1()`, `readerInitialization()` and
`structuredAppend()`. An empty optional means the reader did not report it.
`rect()` returns enclosing integer pixel bounds. `Format.toString()` gives names
such as `"QRCode"`.

`undecoded()` contains localized proposals without accepted decodes. These can
be false candidates or deferred work; an empty list and `unfinished() == false`
do not guarantee exhaustive coverage. Debug JSON schemas are unstable.

## Images

`Image.gray`, `Image.rgb` and `Image.rgba` take a byte array or a
`MemorySegment`, width and height; `.withStride(bytesPerRow)` describes padded
rows. Alpha is ignored. Images are at least 3×3 and at most 32 megapixels. The
addressed layout, `(height - 1) * stride + width * channels` bytes, must fit in
the pixels and in 128 MiB; a larger backing buffer, such as a frame around a
crop, is accepted.

Byte arrays are copied when scanning, limited to the addressed bytes. Native
segments are read in place without a copy, which suits camera frames: wrap a
direct `ByteBuffer` with `MemorySegment.ofBuffer(buffer)`. Keep a native segment
alive and unchanged until the scan returns. Decode image files and convert BGR,
ARGB, planar, float or 16-bit pixels before scanning.

## Building and loading the native library

```sh
python3 scripts/build_native.py
python3 scripts/build_java.py
java --enable-native-access=ALL-UNNAMED -Dtapirscan.library=build/native/libtapirscan.dylib \
    -cp build/java/tapirscan-1.3.0.jar:. MyApp
```

The library is located through the `tapirscan.library` system property (a
file), the `TAPIRSCAN_LIBRARY_DIR` environment variable (a directory), or the
operating system's library search path. `--enable-native-access=ALL-UNNAMED`
avoids the JDK's native-access warning. The JAR and the native library are
distributed separately; Android is not supported.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

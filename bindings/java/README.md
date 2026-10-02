# Tapirscan for Java

Scan decoded pixels and receive every accepted barcode, source-image geometry,
undecoded proposals and reported work limits. Defaults are Medium effort and
retail formats (EAN13, UPCA, EAN8 and UPCE). The dependency-free JDK 22+ binding
uses one native library that contains all four effort modes.

```java
import org.tapirscan.*;

byte[] pixels = new byte[640 * 480];
java.util.Arrays.fill(pixels, (byte) 255);
ScanResult result = Tapirscan.scan(Image.gray(pixels, 640, 480));
for (Barcode barcode : result.barcodes()) {
    System.out.println(barcode.text() + " " + barcode.format() + " " + barcode.polygon());
}
System.out.println(result.undecoded().size() + " undecoded; unfinished: " + result.unfinished());
```

No detection is an empty `barcodes()` list. Invalid input and engine failures
throw `ScannerException`, whose `code` is the native status. Results are
immutable records that survive the scanner. Equal payloads at distinct
locations remain separate physical instances.

## Reuse and configuration

```java
ScannerOptions options = ScannerOptions.defaults()
        .withMode(Mode.HIGH)
        .withFormats(java.util.Set.of(Format.EAN13, Format.QR_CODE));
try (Scanner scanner = new Scanner(options)) {
    ScanResult result = scanner.scan(Image.rgba(pixels, width, height),
            ScanOptions.defaults().withExtendedBudget(true));
    result.best().ifPresent(best -> System.out.println(best.text()));
}
```

Reuse a scanner across images and close it, or use try-with-resources. Scans on
one scanner serialize; separate scanners run concurrently.
`Tapirscan.scan(image, options)` uses a temporary scanner for one image.

| Scanner option   | Default                 | Choices                                 |
| ---------------- | ----------------------- | --------------------------------------- |
| `mode`           | `Mode.MEDIUM`           | `LOW`, `MEDIUM`, `HIGH`, `VERY_HIGH`    |
| `formats`        | `Format.RETAIL`         | Any nonempty `Set<Format>`, or a preset |
| `eanAddOnPolicy` | `EanAddOnPolicy.IGNORE` | `IGNORE`, `READ`, `REQUIRE`             |

Presets: `Format.RETAIL`, `COMMON_1D`, `COMMON`, `LINEAR`, `MATRIX` and `ALL`.
Retail formats are supported; other readers remain experimental. See
[format coverage](../../docs/FORMATS.md).

| Per-scan option  | Default            | Meaning                                      |
| ---------------- | ------------------ | -------------------------------------------- |
| `formats`        | `Optional.empty()` | Override readers for this call               |
| `debug`          | `false`            | Include engine evidence in `debug()` as JSON |
| `extendedBudget` | `false`            | Allow extra reader work for any format       |

`extendedBudget` can cost more time and does not promise exhaustive decoding;
see [API design](../../docs/API_DESIGN.md).

## Results

`ScanResult` exposes `barcodes()`, `undecoded()`, `width()`, `height()`,
`mode()`, `elapsedMs()`, `unfinished()` and optional `debug()` JSON. `values()`
returns decoded text; `best()` returns the largest-support read, keeping
first-read ties. Support is reader-specific and not comparable confidence across
formats.

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

`Image.gray`, `Image.rgb` and `Image.rgba` take a byte array, width and height;
`.withStride(bytesPerRow)` describes padded rows. Alpha is ignored. Pixels are
copied when scanning. Images are at least 3×3 and at most 32 megapixels; the
array must cover `(height - 1) * stride + width * channels` bytes, at most
128 MiB. Decode image files and convert BGR, ARGB, planar, float or 16-bit
pixels before scanning.

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

# Java binding (JDK 22+)

This dependency-free API uses Java's final Foreign Function & Memory API. It needs
JDK 22 or newer; it does not support Java 8/11/17 or Android. Local tests use JDK 25
and compile with `--release 22`. The [Java API documentation](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/java/lang/foreign/SymbolLookup.html)
describes native library lookup and its arena lifetime.

```sh
python3 scripts/build_native.py low medium high very-high
python3 scripts/build_java.py
# Output: build/java/tapirscan-1.1.0.jar
# Alternatively, with Maven installed: mvn -f bindings/java/pom.xml package
```

```java
import java.nio.file.Path;
import org.tapirscan.Tapirscan;
// rgbaBytes contains decoded RGBA pixels; width and height are pixel dimensions.
try (var scanner = new Tapirscan(Path.of("/absolute/path/to/build/native"),
        Tapirscan.Mode.HIGH)) {
    var result = scanner.scan(rgbaBytes, width, height, 4,
        new Tapirscan.ScanOptions(true, false, 1 | 16));
    var reads = result.barcodes();
    var best = result.best();
    for (var barcode : reads) {
        System.out.println(barcode.text() + " (" + barcode.format() + ")");
    }
}
```

Run classpath applications with `--enable-native-access=ALL-UNNAMED`. For example:
`java --enable-native-access=ALL-UNNAMED -cp tapirscan-1.1.0.jar:app.jar Main`
(use `;` as the classpath separator on Windows).

The JAR contains Java classes; provide the four native libraries separately. Mode
selection loads the matching library explicitly. Scanner implements `AutoCloseable`;
scan and close synchronize, repeated close is safe, and use after close fails.
Every result is copied into immutable Java records and a complete JSON string, so
it remains valid after close. The native result is freed before `scan` returns.

Input is byte[] gray8/RGB8/RGBA8, with optional explicit stride. ARGB/BGR images must
be converted. Each call snapshots input into native memory. Argument validation and
native status codes become Java exceptions. `ScanResult.unfinished()` and
`localizationLimited()` remain separate; `json()` preserves candidate evidence when `includeRegions` was requested.
The Maven coordinates are provisional and no package has been published.

`ScanOptions(multiple, includeRegions, formats)` defaults to `(true, false, 1)`
via `ScanOptions.defaults()`. The two-argument constructor also selects EAN13.
The example selects EAN13 and Code128 (`1 | 16`); use
[format bits](../../docs/FORMATS.md) to select other readers. Set `multiple`
to false to return at most the highest-support read after the full scan. The
immutable result and barcode-list types do not change. An empty list means no
successful read. Include region evidence explicitly when needed; otherwise JSON
omits localization/search windows/candidates. Native ABI 4 and the matching JAR
must be deployed together. Output options do not change the selected scanning effort.

## API reference

| Call                                                     | Purpose                                                                 |
| -------------------------------------------------------- | ----------------------------------------------------------------------- |
| `new Tapirscan(libraryDirectory, mode)`                  | Load a native mode from a Path. Mode is LOW, MEDIUM, HIGH or VERY_HIGH. |
| `scan(pixels, width, height, channels)`                  | Scan with tightly packed rows and default options.                      |
| `scan(pixels, width, height, channels, options)`         | Set multiple-result selection, diagnostics and format bits.             |
| `scan(pixels, width, height, channels, stride)`          | Default options with explicit row stride in bytes.                      |
| `scan(pixels, width, height, channels, stride, options)` | Set both stride and options.                                            |
| `mode()`                                                 | Return the loaded mode.                                                 |
| `close()`                                                | Release native resources; normally handled by try-with-resources.       |

`ScanResult` exposes `barcodes()`, `best()` (Optional), `unfinished()`,
`localizationLimited()` and `json()`. Each Barcode has `text()`, `format()`,
`polygon()` (immutable Point list), and `support()` (ranking, not confidence).
Points use input-image coordinates. Empty barcodes means no read. Unlike the
Python/JS convenience APIs, there is no values-only accessor, rectangle helper or
image decoder. Decode JPEG/TIFF using an image library before passing pixel bytes.

The binding is current with native ABI 4 and library version 1.1.0, but no Maven
Central package or bundled native JAR is published. JDK 22+ and separate native
libraries are requirements, not optional optimizations. See [validation](../../docs/VALIDATION.md).

## Finishing candidate work

Enable the fourth `ScanOptions` component, `finishCandidates` to remove shared frame retry and association budgets
for EAN13/UPC-A candidates. Default is disabled; selected formats must include
EAN13 or UPCA. Per-candidate effort and other limits remain; unfinished work is
still reported. See [API design](../../docs/API_DESIGN.md) for scope and cost.

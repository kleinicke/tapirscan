# API migration

## Upgrading from 1.4.0

Native ABI 7 is unchanged, so 1.4.0 native consumers load the new library
without rebuilding.

`DataBarLimited` (bit 8388608) joins `1D` and `all`, and `MicroQRCode`
(bit 16777216) joins `2D` and `all`, so those selections can return the new
formats and take a little longer on QR images. Selecting `QRCode` alone reads no
Micro QR. C's `TAPIRSCAN_FORMATS_LINEAR`, `TAPIRSCAN_FORMATS_MATRIX` and
`TAPIRSCAN_FORMATS_ALL` change value; recompile code that uses them.

The other new formats are opt-in, selected only by name: `Industrial25`
(bit 2097152), `Pharmacode` (bit 4194304), `Code39Extended` (bit 33554432),
`IATA25` (bit 67108864) and `Matrix25` (bit 134217728). C's
`TAPIRSCAN_FORMATS_OPT_IN` grows from 1835008 to 243007488.

Barcodes gain the optional `codabarStartStop` (`codabar_start_stop`), and C the
copy field `TAPIRSCAN_FIELD_CODABAR_START_STOP`. Only a library from this release
knows that field: header, binding and library must come from the same release.
Java record patterns of `Barcode` need the new last component, while its
previous constructor remains.

Rust needs source changes where code relied on the exact shape of result types:

- The result structs `Barcode`, `ScanResult`, `InspectionResult`,
  `StructuredAppend`, `UndecodedRegion`, `Diagnostics`, `Content` and `Field` are
  `#[non_exhaustive]`. Get them from a scan, deserialization or `parse_content`
  instead of a struct literal, and end patterns that destructure them with `..`.
  (`Format` and `ContentKind` were already `#[non_exhaustive]`.)

Exhaustive switches over the format and content-kind enums need the new members
or a default:

- Java: a `switch` expression over `Format` handles `INDUSTRIAL25`, `PHARMACODE`,
  `DATA_BAR_LIMITED`, `MICRO_QR_CODE`, `CODE39EXTENDED`, `IATA25` and `MATRIX25`,
  and one over `ContentKind` handles `ISBN`.
- C++: a `switch` over `Format` or `ContentKind` warns under `-Wswitch` until it
  covers the new values or has a `default`.
- Swift: a `switch` over `Format` or `ContentKind` needs the new cases or a
  `default`.
- Android: a Kotlin `when` or Java `switch` over `org.tapirscan.android.Format`
  or `ContentKind` needs the new members or an `else`/`default` branch.

Structured content recognizes more payloads, so apps that switch on
`content.kind` can see other kinds for the same text:

- 978 and 979 EAN-13 numbers (except 979-0, ISMN) and `ISBN …` or `urn:isbn:`
  text are `ISBN` instead of having no content.
- Bare e-mail addresses and `SMTP:` are `Email`; `www.` addresses and
  `URLTO:`/`MEBKM:` bookmarks are `URL`; `mms:` links are `SMS`; `BIZCARD:` is
  `Contact`. Leading whitespace no longer hides a prefix.
- `SMS` content can have a `subject` field and `URL` content a `title` field
  (from `mms:` links and `URLTO:`/`MEBKM:` bookmarks).
- Some text loses its content: `http(s)` URLs without a host or with
  whitespace, and an empty `TEL:`.

Swift: `scan` and `inspect` for `[UInt8]` pixels and `CGImage` have `async`
overloads. Inside an `async` function Swift selects them, so
`try scanner.scan(image)` there no longer compiles; write
`try await scanner.scan(image)`. Synchronous code and the `CVPixelBuffer`
overloads are unchanged.

Python wheel builds check the native library against the `native-build.json`
that `scripts/build_native.py` writes beside it. A `TAPIRSCAN_NATIVE_DIR` must
contain that file from a build of identical sources; see
[installing local packages](DEVELOPMENT.md#install-local-packages).

## Upgrading from 1.3.0

Version 1.4.0 is an explicit exception to the
[minor-release compatibility policy](../CONTRIBUTING.md#api-stability).
It keeps the 1.4.0 version number despite the Rust source changes below.

Rebuild native consumers. C++, Python and Java check the ABI version when
they load the library. C callers should compare `tapirscan_abi_version()` with
`TAPIRSCAN_ABI_VERSION` before creating a scanner. Headers, binding and library
must come from the same release. Native ABI 7 adds the [content parsing](CONTENT.md) functions and a
fourth `tapirscan_scanner_options` field, `parallel`. Its zero value is
`TAPIRSCAN_PARALLEL_DEFAULT`, so C and C++ initializers written for 1.3.0, with
three values or designated fields, keep their behaviour after recompiling; code
that hard-codes the struct size or layout must be updated. The other functions
are unchanged. The new formats `Code32`, `PZN` and `MSIPlessey` are opt-in: no
preset, `all` included, selects them.

Rust needs source changes where code relied on the exact shape of public types:

- `ScannerOptions` has a new `parallel` field, so a struct literal that names
  every 1.3.0 field fails with E0063. Add `..ScannerOptions::default()`; build
  `ScanOptions` the same way, since option structs may gain fields with default
  values in minor releases:

  ```rust
  let scanner = Scanner::new(ScannerOptions {
      mode: Mode::High,
      formats: Format::Ean13 | Format::QrCode,
      ean_add_on_policy: EanAddOnPolicy::Ignore,
      ..ScannerOptions::default()
  });
  ```

- `Format` and `ContentKind` are `#[non_exhaustive]`: a `match` on them needs a
  wildcard arm.
- `Format::ALL` is a slice (`&[Format]`) instead of an array; use `.len()` and
  iteration instead of a fixed array type.

Python and JavaScript callers need no source changes. Java and C++ need small ones:

- Java: an exhaustive `switch` over `Format` handles `CODE32`, `PZN` and
  `MSI_PLESSEY`, or has a `default`.
- C++: a `switch` over `Format` warns under `-Wswitch` until it covers the new
  values or has a `default`.
- C and C++: three-value `tapirscan_scanner_options` initializers from 1.3.0 keep
  working, but `-Wmissing-field-initializers` warns. Use
  `TAPIRSCAN_SCANNER_OPTIONS_INIT` or a designated initializer.

Two runtime changes affect every native binding:

- Native scans use extra threads by default. A server that runs one scanner per
  core can turn `parallel` off.
- The x86-64 native libraries require x86-64-v2. On an older CPU the process
  crashes with an illegal instruction instead of failing to load.

## Upgrading from 1.2.2

Version 1.3.0 changes the application API in every binding, as an explicit
early-library exception to the [compatibility policy](../CONTRIBUTING.md#api-stability).
Pin 1.2.2 until your callers are migrated. Medium effort, retail formats and
ignored supplements remain the defaults. Scanner improvements may change reads,
geometry, ordering and runtime.

### Scan and inspect

`scan` returns a lightweight result with `values`, `barcodes` and `best`
(properties in JavaScript and Python; `values()` and `best()` in Rust, C++ and
Java). Each barcode has its text, format, polygon and enclosing rectangle.
`inspect` returns the same fields plus timing, unread regions and diagnostics.
C reads the same barcodes through count and copy accessors, with polygons but
no `best` or rectangle helper; see the [C guide](../bindings/c/README.md).

```js
const result = await scan(image);
console.log(result.values);
for (const barcode of result.barcodes) {
  console.log(barcode.text, barcode.polygon, barcode.rect);
}
```

```python
result = tapirscan.scan(image)
print(result.values)
for barcode in result.barcodes:
    print(barcode.text, barcode.polygon, barcode.rect)
```

| In 1.2.2                                    | In 1.3.0                                                         |
| ------------------------------------------- | ---------------------------------------------------------------- |
| Timing or `undecoded` on a scan result      | Call `inspect` instead of `scan`                                 |
| `debug` scan option and `result.debug`      | Call `inspect` and read `result.diagnostics`                     |
| `unfinished` result flag                    | Removed; it was set on nearly every scan                         |
| `extended_budget` / `extendedBudget`        | Removed; it did not change results in Medium, High or Very High  |
| Free `best(barcodes)` function              | `result.best`                                                    |
| Iterating, indexing or `len` on a result    | `result.barcodes` (Python and Rust)                              |
| Python `if result:` (false when empty)      | `if result.barcodes:`; a result object is always truthy          |
| Supplement policy `"Ignore"`, `"Read"`, ... | Lowercase: `"ignore"`, `"read"`, `"require"` (JavaScript/Python) |
| Python `report.to_raw_dict()`               | `report.diagnostics.to_raw_dict()`                               |
| Python `ScannerError.code` as a number      | `.code` is a string shared with JavaScript; `.status` the number |
| Rust, C++, Java `rect()` as floating point  | Integer `Rect { left, top, width, height }`                      |

Separate barcodes with the same text remain separate entries, including in
`values`. `best` keeps the first read on equal support; support is not a
cross-format confidence. JavaScript `JSON.stringify(result)` and Python
`result.as_dict()` of a scan contain only `barcodes`, `values` and `best`.

In JavaScript, per-call `formats` now override the scanner's formats for that
call instead of having to be a subset of them.

JavaScript also changes two package details:

- The `formatBits` export is removed. Select formats by name (`"EAN13"`,
  `"QRCode"`, or presets such as `"retail"`); the bit values remain internal.
- The WASM files are renamed from `<mode>-release-1.2.2-….wasm` to `low.wasm`,
  `medium.wasm`, `high.wasm`, `very-high.wasm` and `experimental-turbo2.wasm` to
  `-turbo16.wasm`, importable as `tapirscan/wasm/<name>.wasm`. Update copy steps,
  `wasmBaseUrl` hosting and custom `loadWasm` URLs. `tapirscan/browser` needs no
  setup.

### Browser applications

The new `tapirscan/browser` entry accepts files and browser image sources and
scans in a bundled worker: `new Scanner(options)`, then `await scanner.scan(source)`.
The core `tapirscan` entry still uses `await Scanner.create(options)` and
synchronous scans over decoded pixels. See
[browser setup](../bindings/javascript/README.md#browser-apps-react-and-svelte).

### Native bindings

C, C++ and Java now use native ABI 6: one library containing all four modes,
the same options and result model as the other bindings, caller-owned error
details and typed result access. Rebuild native consumers against the 1.3.0
headers and library; see the [C](../bindings/c/README.md),
[C++](../bindings/cpp/README.md) and [Java](../bindings/java/README.md) guides.

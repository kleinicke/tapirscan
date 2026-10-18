# API migration

## Upgrading from 1.3.0

Rebuild native consumers. C, C++, Python and Java check the ABI version when
they load the library, so headers, binding and library must come from the same
release. Native ABI 7 adds the [content parsing](CONTENT.md) functions and a
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

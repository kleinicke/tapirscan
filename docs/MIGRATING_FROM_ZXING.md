# Migrating from ZXing

Covers `@zxing/library` with `@zxing/browser`, `zxing-wasm` and `zxing-cpp` (Python). Tapirscan
returns every barcode in an image as `text`, `format`, `polygon` and `rect`; see the
[JavaScript](../bindings/javascript/README.md) and [Python](../bindings/python/README.md) guides for
the full API.

## Install and scan an image

**JavaScript, browser:** `npm install tapirscan`. Worker, WASM and CSP setup:
[browser deployment](BROWSER_DEPLOYMENT.md).

```js
// Before: @zxing/browser
import { BrowserMultiFormatReader } from "@zxing/browser";
const result = await new BrowserMultiFormatReader().decodeFromImageUrl(url);
console.log(result.getText(), result.getBarcodeFormat());

// After
import { Scanner } from "tapirscan/browser/medium";
const scanner = new Scanner({ formats: ["EAN13", "QRCode"] });
const { barcodes } = await scanner.scan(await (await fetch(url)).blob());
for (const b of barcodes) console.log(b.text, b.format);
scanner.dispose(); // stops the worker; reuse the scanner for more images instead
```

**JavaScript, Node or decoded pixels:** `Scanner.create` is async, `scan` is synchronous.
`ImageData` or `{ data, width, height, channels }`; encoded JPEG/PNG bytes are not accepted, so
decode them first (for example with sharp).

```js
// Before: zxing-wasm
import { readBarcodes } from "zxing-wasm/reader";
const results = await readBarcodes(imageData, { formats: ["EAN13", "QRCode"] });
console.log(results[0].text, results[0].format);

// After
import { Scanner } from "tapirscan";
const scanner = await Scanner.create({ formats: ["EAN13", "QRCode"] });
const { barcodes } = scanner.scan(imageData);
scanner.dispose();
```

**Python:** `pip install tapirscan`.

```python
# Before
import zxingcpp
from zxingcpp import BarcodeFormat

results = zxingcpp.read_barcodes(
    image, formats=[BarcodeFormat.EAN13, BarcodeFormat.QRCode]
)

# After
import tapirscan

result = tapirscan.scan(image, formats=["EAN13", "QRCode"])
for b in result.barcodes:
    print(b.text, b.format, b.rect)
```

`image` is a Pillow image or NumPy array in both. zxing-cpp reads arrays as BGR; Tapirscan reads
RGB, so pass `color_order="BGR"` for OpenCV frames. Without `formats`, Tapirscan reads only EAN13,
UPCA, EAN8 and UPCE, where ZXing reads everything.

## Formats

| zxing-cpp / zxing-wasm                                   | @zxing/library                      | Tapirscan                                  |
| -------------------------------------------------------- | ----------------------------------- | ------------------------------------------ |
| `EAN13`, `UPCA`, `EAN8`, `UPCE`                          | `EAN_13`, `UPC_A`, `EAN_8`, `UPC_E` | `EAN13`, `UPCA`, `EAN8`, `UPCE`            |
| `EAN2`, `EAN5`                                           | `UPC_EAN_EXTENSION`                 | no format; `eanAddOnPolicy: "read"`        |
| `ISBN`                                                   | none                                | `EAN13`                                    |
| `Code128`                                                | `CODE_128`                          | `Code128` (includes GS1-128)               |
| `Code39`, `Code39Std`                                    | `CODE_39`                           | `Code39`                                   |
| `Code39Ext`                                              | none                                | none (no full-ASCII expansion)             |
| `Code32`, `PZN`                                          | none                                | `Code32`, `PZN` (opt-in)                   |
| `Code93`, `Codabar`                                      | `CODE_93`, `CODABAR`                | `Code93`, `Codabar`                        |
| `ITF`, `ITF14`                                           | `ITF`                               | `ITF`                                      |
| `DataBar`, `DataBarOmni`, `DataBarStk`, `DataBarStkOmni` | `RSS_14`                            | `DataBar`                                  |
| `DataBarExp`, `DataBarExpStk`                            | `RSS_EXPANDED`                      | `DataBarExpanded`                          |
| `DataBarLtd`, `Telepen*`, `DXFilmEdge`                   | none                                | none                                       |
| none                                                     | none                                | `MSIPlessey` (opt-in)                      |
| `QRCode`, `QRCodeModel2`                                 | `QR_CODE`                           | `QRCode`                                   |
| `QRCodeModel1`, `MicroQRCode`, `RMQRCode`                | `MICRO_QR_CODE`                     | none                                       |
| `DataMatrix`                                             | `DATA_MATRIX`                       | `DataMatrix`                               |
| `PDF417`, `CompactPDF417`                                | `PDF_417`                           | `PDF417`                                   |
| `MicroPDF417`                                            | none                                | none                                       |
| `Aztec`, `AztecCode`, `AztecRune`                        | `AZTEC`                             | `Aztec` (includes Rune)                    |
| `MaxiCode`                                               | `MAXICODE`                          | `MaxiCode`                                 |
| `All`, `AllReadable`                                     |                                     | `"all"` (opt-in formats stay excluded)     |
| `AllLinear`, `AllMatrix`                                 |                                     | `"1D"`, `"2D"`                             |
| `AllRetail`                                              |                                     | `"retail"` (no DataBar; add it explicitly) |

Result `format` values equal zxing-wasm's names except for the grouped rows. zxing-cpp's
`str(format)` gives `"EAN-13"`; use `format.name` for `"EAN13"`.

Text differs for four formats:

| Format  | ZXing                                                                | Tapirscan                                                    |
| ------- | -------------------------------------------------------------------- | ------------------------------------------------------------ |
| UPC-A   | zxing-cpp/wasm: 13 digits `0036000291452`; @zxing/library: 12 digits | `UPCA` selected: 12 digits; only `EAN13` selected: 13 digits |
| UPC-E   | zxing-cpp/wasm: expanded to 13 digits `0042100005264`                | 8 digits as encoded, `04252614`                              |
| Codabar | start and stop kept, `A123456A`                                      | stripped, `123456`                                           |
| DataBar | zxing-cpp/wasm `(01)...`; @zxing/library GTIN only                   | `0112345678901231`: AI 01, no brackets                       |

With `EAN13` and `UPCA` both selected, a zero-prefixed EAN-13 is returned as `UPCA` with 12 digits.

## Geometry

`polygon` is four source-image points (`[x, y]` floats in JavaScript, `Point(x, y)` in Python);
`rect` is the enclosing integer box. For 2D symbols the order follows the symbol's own orientation,
as in ZXing: top-left, top-right, bottom-right, bottom-left, rotating with the symbol. For 1D
symbols rotated by 180 or 270 degrees the points are in image order, so ZXing's names for them
differ. ZXing's 1D positions follow the scanline that decoded and need not bound a tilted symbol;
Tapirscan's polygon does.

```js
// zxing-wasm: result.position
const toPosition = ([tl, tr, br, bl]) => {
  const p = ([x, y]) => ({ x, y });
  return { topLeft: p(tl), topRight: p(tr), bottomRight: p(br), bottomLeft: p(bl) };
};

// @zxing/library: result.getResultPoints()
const toResultPoints = (b) => b.polygon.map(([x, y]) => ({ getX: () => x, getY: () => y }));
```

```python
from types import SimpleNamespace


def to_position(barcode):  # zxing-cpp: barcode.position
    tl, tr, br, bl = barcode.polygon
    return SimpleNamespace(top_left=tl, top_right=tr, bottom_right=br, bottom_left=bl)
```

`@zxing/library` returns different points per symbology: two scanline ends for 1D, three
finder-pattern centres plus an alignment point for QR, eight for PDF417. Code that expects those
needs the symbology-specific points computed from the polygon; for a 1D scanline use the midpoints
of the left and right edges, `[mid(tl, bl), mid(tr, br)]`.

## GS1

| Library                   | Text of GS1-128 / Data Matrix / QR                                  | Marker                                                                                  |
| ------------------------- | ------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| zxing-cpp, zxing-wasm     | `(01)123...(10)ABC123(21)XY9`, no separators (default `HRI`)        | `content_type` / `contentType` `GS1`, `symbology_identifier` `]C1`, `]d8`, `]Q3`, `]e0` |
| same, `text_mode="Plain"` | `01123...10ABC123\x1d21XY9`                                         | same                                                                                    |
| @zxing/library, Code 128  | FNC1 dropped: `...10ABC12321XY9`; with `ASSUME_GS1`: `]C1` + `\x1d` | none                                                                                    |
| @zxing/library, QR        | `\x1d` kept                                                         | none                                                                                    |
| Tapirscan                 | `01123...10ABC123\x1d21XY9`                                         | `gs1: true`, `content.kind === "GS1"`                                                   |

Tapirscan keeps the `\x1d` group separators and adds no brackets or `]C1` prefix. Your app decides
how to store them. `content` has one field per application identifier, so prefer it over splitting
text:

```python
if b.content and b.content.kind == "GS1":
    gtin, lot = b.content.get("01"), b.content.get("10")
    hri = "".join(f"({f.name}){f.value}" for f in b.content.fields)  # ZXing HRI text
    flat = b.text.replace("\x1d", "")  # separator-free storage
    marked = b.text.replace("\x1d", "<GS>")  # zxing "Escaped" style
```

```js
const gs1 = b.content?.kind === "GS1";
const hri = b.content?.fields.map((f) => `(${f.name})${f.value}`).join("");
const flat = b.text.replaceAll("\x1d", "");
```

Removing separators is only reversible with the application identifier length table, because
variable-length elements such as `10` end at the separator. Tapirscan returns no symbology
identifier: store `]C1` yourself for `gs1` Code128 reads if downstream code expects it.
`parse_content(text, gs1=True)` parses a string from another source.

## Multiple barcodes, effort and options

`scan` returns all barcodes; `result.best` is the highest-support one. zxing-cpp and zxing-wasm also
return all (`maxNumberOfSymbols` caps zxing-wasm at 255; use `barcodes.slice(0, n)`).
`@zxing/library` returns one and exposes no multi-read reader.

Effort is a mode, fixed when the scanner is created: in the browser import
`tapirscan/browser/<mode>`; the core, `tapirscan/browser` and Python take `mode`.

| ZXing                                                                                           | Tapirscan                                            |
| ----------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| `tryHarder: false`                                                                              | Low (camera frames)                                  |
| `tryHarder: true` (zxing-wasm default)                                                          | Medium (default); High for slow scans                |
| none                                                                                            | Very High for hard stills                            |
| `minLineCount`, `isPure`, `tryDownscale`                                                        | no equivalent                                        |
| `binarizer`, `tryDenoise`, `tryInvert`                                                          | no equivalent                                        |
| `tryRotate`                                                                                     | no option; orientation is always searched            |
| `textMode`, `characterSet`, `returnErrors`, `validateOptionalChecksum`, `tryCode39ExtendedMode` | no equivalent                                        |
| `eanAddOnSymbol` Ignore / Read / Require                                                        | `eanAddOnPolicy` `"ignore"` / `"read"` / `"require"` |
| `bytes` / `getRawBytes()`                                                                       | `payload_bytes` / `payloadBytes`, 2D formats only    |
| `orientation`, `isMirrored`, `ecLevel`                                                          | none; derive rotation from `polygon`                 |

## No result

`@zxing/library` throws `NotFoundException` when nothing decodes. zxing-cpp and zxing-wasm return an
empty list. Tapirscan returns empty `barcodes` and `values` and `best` is `undefined` / `None`. It
throws only for invalid input or engine failure (`TypeError` / `ValueError`, `ScannerError`); delete
`NotFoundException` checks rather than translating them.

## Live camera (JavaScript)

```js
// Before: @zxing/browser
const controls = await reader.decodeFromVideoDevice(undefined, video, (result, error) => {
  if (result) show(result.getText());
});
controls.stop();

// After
import { Scanner } from "tapirscan/browser/medium";
import { CameraScanner } from "tapirscan/camera";
const scanner = new Scanner({ formats: ["EAN13", "QRCode"] });
const camera = new CameraScanner(video, scanner, {
  onScan(result) {
    /* every scanned frame; result.barcodes may be empty */
  },
  onError(error) {
    /* camera or scanner failure; scanning has stopped */
  },
});
await camera.start();
camera.stop();
```

An empty `onScan` replaces the per-frame `NotFoundException` callback. Opening failures
(permission, no camera) reject `start()`; later failures arrive in `onError`. `controls.switchTorch` becomes `camera.setTorch(on)`; `start({ deviceId })` replaces the
`deviceId` argument. Options, camera selection and torch:
[camera scanning](../bindings/javascript/README.md#camera-scanning).

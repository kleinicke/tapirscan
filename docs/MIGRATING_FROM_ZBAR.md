# Migrating from ZBar

For [pyzbar](https://pypi.org/project/pyzbar/) and
[`@undecaf/zbar-wasm`](https://www.npmjs.com/package/@undecaf/zbar-wasm)
(`zbar.wasm` has the same API). [zbarlight](https://pypi.org/project/zbarlight/)
wraps the same library; replace it like pyzbar, with `formats` for its code
types.

- **No system library.** pyzbar fails at import without `libzbar0` (apt) or
  `zbar` (Homebrew); Tapirscan wheels bundle the native library and the npm
  package its WASM.
- **Default formats are `retail`** (EAN13, UPCA, EAN8, UPCE); ZBar enables most
  symbologies. Set `formats` ([configuration](#configuration)).
- **Results are `text` and a format identifier**, plus exact bytes for matrix
  codes ([data type](#data-type)).

## Install and scan

```python
# Before: sudo apt install libzbar0 (or brew install zbar); pip install pyzbar pillow
from PIL import Image
from pyzbar.pyzbar import ZBarSymbol, decode

with Image.open("label.png") as image:
    for d in decode(image, symbols=[ZBarSymbol.EAN13, ZBarSymbol.QRCODE]):
        print(d.type, d.data.decode("utf-8"), d.rect)
```

```python
# After: pip install tapirscan pillow
import tapirscan
from PIL import Image

with Image.open("label.png") as image:
    result = tapirscan.scan(image, formats=["EAN13", "QRCode"])

for barcode in result.barcodes:
    print(barcode.format, barcode.text, barcode.rect)
```

NumPy arrays work in both (Tapirscan: uint8 HW, HWC or CHW; OpenCV needs
`color_order="BGR"`). pyzbar's `decode((data, width, height))` becomes
`tapirscan.scan(tapirscan.PixelImage(data, width=width, height=height, channels=1))`.
For many images, reuse a `tapirscan.Scanner` or call `tapirscan.scan_many`
([Python guide](../bindings/python/README.md#image-input)).

```js
// Before: npm install @undecaf/zbar-wasm
import { scanImageData } from "@undecaf/zbar-wasm";

const imageData = context.getImageData(0, 0, canvas.width, canvas.height);
for (const symbol of await scanImageData(imageData)) {
  console.log(symbol.typeName, symbol.decode()); // "ZBAR_EAN13", "4006381333931"
}
```

```js
// After, browser: npm install tapirscan. Accepts canvas, <img>, <video>, File, ImageData.
import { Scanner } from "tapirscan/browser/medium";

const scanner = new Scanner({ formats: ["EAN13", "QRCode"] });
const result = await scanner.scan(canvas);
for (const barcode of result.barcodes) console.log(barcode.format, barcode.text);
scanner.dispose();
```

In Node, `import { Scanner } from "tapirscan"` and
`await Scanner.create(options)` scan `ImageData`-shaped pixels synchronously:
`scanner.scan({ data, width, height })` (RGBA; add `channels` for 1 or 3).
Decode image files first; filenames and JPEG/PNG bytes are not inputs.

Live camera: `new CameraScanner(video, scanner, { onScan, onError })` from
`tapirscan/camera`, then `start()` and `stop()`, replaces the frame loop
([camera scanning](../bindings/javascript/README.md#camera-scanning); CSP and
hosting in [browser deployment](BROWSER_DEPLOYMENT.md)). The JavaScript guide
also shows [drawing results over the video](../bindings/javascript/README.md#drawing-results-over-the-video),
[scanning a region](../bindings/javascript/README.md#scanning-a-region), a
[camera picker](../bindings/javascript/README.md#camera-picker-and-zoom) and
[recorded video](../bindings/javascript/README.md#recorded-video).

**`zbarcam`:** with OpenCV, reuse one scanner for every frame;
`cv2.VideoCapture("clip.mp4")` reads a video file the same way.

```python
import cv2
import numpy as np
import tapirscan

capture = cv2.VideoCapture(0)
with tapirscan.Scanner(mode="low", formats=["EAN13", "QRCode"]) as scanner:
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        for barcode in scanner.scan(cv2.cvtColor(frame, cv2.COLOR_BGR2RGBA)).barcodes:
            print(f"{barcode.format}:{barcode.text}")
            corners = np.array([(p.x, p.y) for p in barcode.polygon], np.int32)
            cv2.polylines(frame, [corners], True, (0, 255, 0), 2)
        cv2.imshow("scan", frame)
        if cv2.waitKey(1) == 27:  # Esc
            break
capture.release()
cv2.destroyAllWindows()
```

Low keeps up with live frames; use Medium for recorded video. To scan only a
region, pass a slice (`scanner.scan(rgba[y0:y1, x0:x1])`) and add `x0` and `y0`
to the coordinates.

**`zbarimg`:** Tapirscan has no CLI. Replacement (first argument is a format or
group; print `b.text` alone for `--raw`):

```sh
python -c 'import sys; from PIL import Image; import tapirscan; [print(f"{b.format}:{b.text}") for p in sys.argv[2:] for b in tapirscan.scan(Image.open(p), formats=sys.argv[1]).barcodes]' 1D image.png
```

Like ZBar, Tapirscan only reads barcodes. To create them, use a generator such
as [segno](https://pypi.org/project/segno/) (Python) or
[bwip-js](https://github.com/metafloor/bwip-js) (JavaScript).

## Format mapping

| ZBar (`type`, `ZBarSymbol`) | Tapirscan                    | Notes                                                           |
| --------------------------- | ---------------------------- | --------------------------------------------------------------- |
| `EAN13`                     | `EAN13`                      | Becomes `UPCA` for zero-prefixed reads when `UPCA` is selected. |
| `UPCA`                      | `UPCA`                       | 12 digits.                                                      |
| `EAN8`, `UPCE`              | `EAN8`, `UPCE`               |                                                                 |
| `ISBN13`, `ISBN10`          | `EAN13`                      | [Convert yourself](#ean-13-upc-a-and-isbn).                     |
| `EAN2`, `EAN5`              | none                         | [`ean_add_on_policy`](#ean-2-and-ean-5-supplements).            |
| `I25`                       | `ITF`                        |                                                                 |
| `CODE39`                    | `Code39`                     | Standard characters only; check characters stay in `text`.      |
| `CODE93`, `CODE128`         | `Code93`, `Code128`          | Code128 includes GS1-128: [GS1](#gs1).                          |
| `CODABAR`                   | `Codabar`                    |                                                                 |
| `DATABAR`, `DATABAR_EXP`    | `DataBar`, `DataBarExpanded` | Stacked variants included.                                      |
| `QRCODE`                    | `QRCode`                     | Model 2; no Micro QR, Model 1 or rMQR.                          |
| `PDF417`                    | `PDF417`                     | No Micro PDF417.                                                |
| `SQCODE`, `COMPOSITE`       | none                         | Not supported.                                                  |

Tapirscan also reads `DataMatrix`, `Aztec`, `MaxiCode`, and, only when named,
`Code32`, `PZN` and `MSIPlessey`. Identifiers are case-sensitive. zbar-wasm
`typeName` is `"ZBAR_EAN13"`; strip `ZBAR_`.

### EAN-13, UPC-A and ISBN

A UPC-A symbol is an EAN-13 with a leading zero.

| Selection                          | `0036000291452` returns                                          |
| ---------------------------------- | ---------------------------------------------------------------- |
| pyzbar default                     | `EAN13` `0036000291452`                                          |
| pyzbar with `ZBarSymbol.UPCA` also | `UPCA` `036000291452`                                            |
| Tapirscan `formats=["EAN13"]`      | `EAN13` `0036000291452`                                          |
| Tapirscan `retail` (default)       | `UPCA` `036000291452`                                            |
| Tapirscan `formats=["UPCA"]`       | `UPCA` `036000291452`; non-zero-prefixed EAN-13 are not returned |

ZBar reports `ISBN13`/`ISBN10` only when enabled; Tapirscan always reports
`EAN13` (978/979 prefix). ISBN-10 conversion (port to JavaScript the same way):

```python
def isbn10(ean13: str) -> str | None:
    if len(ean13) != 13 or not ean13.startswith("978"):
        return None  # 979 numbers have no ISBN-10
    body = ean13[3:12]
    check = (11 - sum((10 - i) * int(d) for i, d in enumerate(body)) % 11) % 11
    return body + ("X" if check == 10 else str(check))
```

### EAN-2 and EAN-5 supplements

ZBar returns a supplement as its own `EAN2`/`EAN5` result (rect width 0), only
when that type is enabled. Tapirscan attaches it to the main barcode. Policies:
`"ignore"` (default), `"read"` (main barcode stays if no supplement reads),
`"require"` (EAN/UPC reads need a readable supplement).

```python
# Before
symbols = [ZBarSymbol.EAN13, ZBarSymbol.EAN5, ZBarSymbol.EAN2]
found = {d.type: d.data.decode() for d in decode(image, symbols=symbols)}
isbn, addon = found.get("EAN13"), found.get("EAN5") or found.get("EAN2")

# After
for barcode in tapirscan.scan(
    image, formats=["EAN13"], ean_add_on_policy="read"
).barcodes:
    print(barcode.text, barcode.ean_add_on)  # "9783161484100" "52495"
```

JavaScript: `eanAddOnPolicy: "read"` on the scanner, then `barcode.eanAddOn`.
`polygon` and `rect` cover the main barcode only.

## Data type

pyzbar returns `bytes`, Tapirscan `str`.

| Library   | Non-ASCII and binary content                                                                                                                       |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| pyzbar    | `data` is ZBar's UTF-8 conversion. QR bytes `E9` arrive as `C3 A9`; originals are unrecoverable. A UTF-8 QR without ECI can come back mis-decoded. |
| zbar-wasm | `data` is an `Int8Array` of the raw bytes.                                                                                                         |
| Tapirscan | `text` is a decoded string; for arbitrary binary it is a guess (a payload starting `FF FE` decoded as UTF-16).                                     |

For exact bytes use `payload_bytes` (Python `bytes`) / `payloadBytes`
(JavaScript `number[]`). QR Code, Data Matrix, Aztec, PDF417 and MaxiCode supply
them, before character-set interpretation (a Kanji-mode QR gives Shift JIS bytes
while `text` is Unicode). Other formats give `None` / `undefined`; `text` is
their data. `text.encode("utf-8")` does not reconstruct the payload.

Bytes for any result:
`bytes(b.payload_bytes) if b.payload_bytes is not None else b.text.encode()`.

## Geometry

Source-image pixels, origin top left, y down, in all libraries.

| Field     | pyzbar                                                                           | Tapirscan                                                                                               |
| --------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `rect`    | `Rect(left, top, width, height)`, ints                                           | Same fields in Python; `{ left, top, width, height }` in JavaScript. Can differ by a pixel.             |
| `polygon` | `Point(x, y)` ints, convex hull: four or five points, counterclockwise on screen | Exactly four corners, clockwise on screen: `Point(x, y)` floats in Python, `[x, y]` pairs in JavaScript |

The first Tapirscan corner follows the reading direction (a barcode rotated 90
degrees starts at the bottom left); do not assume top left. zbar-wasm `points`
holds every scan-line end point for linear codes (hundreds) and four corners for
QR; use `polygon` or `rect` instead.

pyzbar-style integer points:
`[(round(p.x), round(p.y)) for p in barcode.polygon]`.

## Orientation and quality

Neither exists. `support` is the nearest field, but it is an uncalibrated
per-reader ranking value, not comparable across formats; do not threshold on it.
Select only expected formats and validate content instead.

## Configuration

| ZBar                                                       | Tapirscan                                                                                                  |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `symbols=[...]`; `setConfig(NONE, ENABLE, 0)` plus enables | `formats=[...]` or a group (`retail`, `common1D`, `common`, `1D`, `2D`, `all`); unselected formats are off |
| `ENABLE` for ISBN, EAN2, EAN5                              | [Supplement policy](#ean-2-and-ean-5-supplements); ISBN is not a format                                    |
| `X_DENSITY`, `Y_DENSITY`                                   | None; choose a mode: Python `mode=`, browser `tapirscan/browser/<mode>`                                    |
| `MIN_LEN`, `MAX_LEN`                                       | None; filter on `len(barcode.text)`                                                                        |
| `ADD_CHECK`, `EMIT_CHECK`                                  | None; check digits are returned as read                                                                    |
| `ASCII` (Code 39 full ASCII)                               | Not supported                                                                                              |
| `BINARY`                                                   | `payload_bytes`                                                                                            |

Use Low for camera frames (`mode="low"`, `tapirscan/browser/low`), `"high"` for hard stills
([effort levels](FORMATS.md#effort-levels)). ZBar's usual symbologies:

```python
ZBAR_LIKE = [
    "EAN13", "UPCA", "EAN8", "UPCE", "Code128", "Code39", "Code93", "ITF",
    "Codabar", "DataBar", "DataBarExpanded", "QRCode",
]  # fmt: skip
result = tapirscan.scan(image, formats=ZBAR_LIKE)
```

Each enabled 2D format searches the whole image; avoid `"all"` in production.

## GS1

ZBar and Tapirscan return the same text for GS1-128: no AI parentheses, leading
FNC1 dropped, FNC1 between variable-length elements as `\x1d`. Tapirscan adds
`barcode.gs1` (`True` when the reader saw a GS1 symbol) and `barcode.content`,
the validated elements as named fields ([structured content](CONTENT.md)).

```python
# FNC1 01 09506000134352 10 AB12 FNC1 17 261231
barcode.text  # "010950600013435210AB12\x1d17261231"
content = barcode.content
if content and content.kind == "GS1":
    content.get("01"), content.get("10"), content.get("17")  # GTIN, lot, expiry
```

JavaScript: `barcode.content.kind === "GS1"`, and
`barcode.content.fields.find((f) => f.name === "01")?.value`.

Stripping `\x1d` loses element boundaries; replace it
(`barcode.text.replace("\x1d", "|")`) or store the fields. A string that fails
GS1 validation (bad check digit, unknown identifier) has no `content`; `text` is
still returned.

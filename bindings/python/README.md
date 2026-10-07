# Tapirscan for Python

`scan(image)` returns every decoded barcode: `result.values` gives the decoded
strings and `result.barcodes` pairs each one with its `format` and source-image
`polygon` / `rect`. Both are empty when nothing is decoded.

[Try the live demo](https://tapirscan.f-kleinicke.de) · [Quick start](#quick-start) · [Functions](#functions) · [All options](#all-options) · [Results](#results)

```python
import tapirscan

result = tapirscan.scan(image)
print(result.values)
for barcode in result.barcodes:
    print(barcode.text, barcode.polygon)
```

`image` can be a Pillow image, a NumPy array or a PyTorch tensor. Use `inspect`
instead of `scan` for timing, unread regions and diagnostics.

## Installation

Python 3.10+ is required. Platform wheels bundle the native library with all four
effort modes:

```sh
pip install tapirscan
```

## Quick start

Install the image libraries you use. For these examples:

```sh
pip install tapirscan tifffile pillow torch
```

### TIFF with tifffile

```python
import tifffile
import tapirscan

pixels = tifffile.imread("label.tif", key=0)  # NumPy array: first TIFF page
result = tapirscan.scan(pixels)
print(result.values)
```

This assumes an 8-bit grayscale or RGB image. Defaults are Medium effort and the
retail formats (EAN13, UPCA, EAN8, UPCE).

### JPEG with Pillow

```python
from PIL import Image
import tapirscan

with Image.open("label.jpg") as image:
    result = tapirscan.scan(image, mode="high", formats="1D")

for barcode in result.barcodes:
    print(barcode.text, barcode.format, barcode.polygon)
```

`formats="1D"` enables all linear formats. `"retail"` selects EAN13, UPCA, EAN8
and UPCE; `"common1D"` adds Code128, Code39 and ITF; `"common"` adds QRCode and
DataMatrix; `"2D"` and `"all"` are also available. See
[format coverage](../../docs/FORMATS.md).

### PyTorch tensors

```python
import torch

tensor = torch.from_numpy(pixels)  # the array from the TIFF example
result = tapirscan.scan(tensor)
print(result.values)
```

GPU tensors and tensors with `requires_grad=True` work directly: Tapirscan copies
the pixels to CPU without changing your tensor or its autograd graph. Decoding
runs on CPU. See [array and tensor inputs](#array-and-tensor-inputs).

## Functions

```python
# One image, with automatic cleanup:
result = tapirscan.scan(image, mode="medium", formats=["EAN13"])

# Reuse native initialization across images:
with tapirscan.Scanner(mode="high", formats="1D") as scanner:
    result = scanner.scan(image)
    print(result.values)
```

Signatures (all settings are optional). `inspect` and `scanner.inspect` take the
same arguments as their `scan` counterparts and return `InspectionResult`:

```text
scan(image, *, mode="medium", formats=None, ean_add_on_policy="Ignore", extended_budget=False,
     layout="auto", value_range="auto", color_order="RGB", library_dir=None) -> ScanResult
Scanner(mode="medium", *, formats=None, ean_add_on_policy="Ignore", library_dir=None)
scanner.scan(image, *, formats=None, extended_budget=False,
             layout="auto", value_range="auto", color_order="RGB") -> ScanResult
scanner.close()
```

Use the context manager or call `close()` in a `finally` block; repeated `close()`
is safe and results stay valid afterwards. Calls on one scanner run one at a time;
separate scanners can run concurrently. `scanner.mode` and `scanner.formats` are
read-only; create another scanner to change effort. Per-call `formats` apply to
that call only; `None` uses the scanner's selection.

## All options

| Option              | Where                     | Default         | Meaning                                                                                                                |
| ------------------- | ------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `mode`              | Creation / one-shot       | `"medium"`      | `"low"`, `"medium"`, `"high"`, `"very-high"`.                                                                          |
| `formats`           | Creation / scan           | retail          | An identifier, `"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"`, `"all"`, or a nonempty list/tuple of identifiers. |
| `ean_add_on_policy` | Creation / one-shot       | `"Ignore"`      | `"Ignore"`, `"Read"`, `"Require"`; see [supplements](#eanupc-supplements).                                             |
| `extended_budget`   | Scan / one-shot           | `False`         | Allow extra reader work on difficult images. Still bounded.                                                            |
| `layout`            | Scan, arrays/tensors only | `"auto"`        | `"HW"`, `"HWC"`, `"CHW"`; specify when the channel position is ambiguous.                                              |
| `value_range`       | Scan, arrays/tensors only | `"auto"`        | `"0_1"` or `"0_255"`. Auto uses [0,1] for floats and [0,255] for integers, independent of image contents.              |
| `color_order`       | Scan, arrays/tensors only | `"RGB"`         | `"BGR"` for OpenCV BGR/BGRA pixels. Grayscale is unaffected.                                                           |
| `library_dir`       | Creation / one-shot       | Bundled library | Directory of a custom native build. Lookup order: this argument, `TAPIRSCAN_LIBRARY_DIR`, then the wheel's library.    |

Effort modes tune EAN/UPC, common linear formats and QR Code; other matrix readers
use a fixed effort. Format group exports: `retail_formats`, `common_linear_formats`,
`common_formats`, `linear_formats`, `matrix_formats`. Resizing, cropping, rotation
and camera capture are up to the caller.

## Image input

### Array and tensor inputs

One HW/HWC/CHW image, optionally with a leading batch axis of size one, with 1, 3
or 4 channels. NumPy views and noncontiguous tensors are accepted. Colors default
to RGB/RGBA. Booleans become black/white. NaN, infinity and out-of-range values
are rejected rather than clipped: scale higher-bit-depth intensities and undo
mean/std normalization first. Floats stored in byte units need
`value_range="0_255"`.

CUDA, MPS and other device tensors are copied to CPU, which adds latency. Sparse,
quantized, complex and meta tensors are rejected.

### OpenCV

```python
import cv2
import tapirscan

image = cv2.imread("label.jpg")
if image is None:
    raise ValueError("Could not load label.jpg")
result = tapirscan.scan(image, color_order="BGR")
```

OpenCV is not a dependency. `color_order="BGR"` does not modify your array.

### Pillow

Pillow `I`, `F` and `I;16*` images are read as intensities in [0,255], matching
Pillow's usual conversion convention; scale higher-bit-depth images explicitly.
`layout`, `value_range` and `color_order` do not apply to Pillow images.

### Raw pixels

```python
image = tapirscan.PixelImage(pixels, width=640, height=480, channels=3)
result = tapirscan.scan(image)
```

`pixels` contains decoded bytes, not a JPEG/PNG file.

| `PixelImage` field | Default                | Meaning                                                  |
| ------------------ | ---------------------- | -------------------------------------------------------- |
| `data`             | Required, positional   | `bytes`, `bytearray`, or a contiguous byte `memoryview`. |
| `width`, `height`  | Required, keyword-only | Integer dimensions, at least 3 pixels each.              |
| `channels`         | `1`                    | 1 grayscale, 3 RGB, 4 RGBA. Alpha is ignored.            |
| `stride`           | `width * channels`     | Bytes between row starts. Larger values allow padding.   |

Storage must cover `(height - 1) * stride + width * channels` bytes.

### Limits

Images may have at most 32 megapixels and 128 MiB of pixel data; this is checked
before any conversion or device transfer. Coordinates start at the top left. If
you resize before scanning, map coordinates back when drawing on the original.

## Results

`scan` returns a `ScanResult`; `inspect` returns an `InspectionResult` with the
same fields plus the ones marked _inspect_. Results are immutable.

| Field/method                    | Meaning                                                                    |
| ------------------------------- | -------------------------------------------------------------------------- |
| `result.values`                 | List of decoded strings, one per barcode.                                  |
| `result.barcodes`               | Tuple of immutable `Barcode` objects.                                      |
| `result.best`                   | Highest-support barcode (first on ties), or None.                          |
| `result.as_dict()`              | JSON-compatible dictionary of the public fields, without diagnostics.      |
| `result.image`                  | _inspect_: `ImageSize(width, height)` of the supplied pixels.              |
| `result.mode`                   | _inspect_: effort mode used.                                               |
| `result.elapsed_ms`             | _inspect_: native scan time, excluding image conversion.                   |
| `result.undecoded`              | _inspect_: [regions without a decode](#undecoded-regions).                 |
| `result.diagnostics`            | _inspect_: [engine evidence](#diagnostics).                                |
| `barcode.text`, `.format`       | Decoded text and format identifier.                                        |
| `barcode.polygon`               | Tuple of four `Point(x, y)` source-image coordinates.                      |
| `barcode.rect`                  | Enclosing integer `Rect(left, top, width, height)`.                        |
| `barcode.support`               | Reader-specific ranking evidence used by `best`.                           |
| `barcode.payload_bytes`         | Decoded matrix payload bytes, or None.                                     |
| `barcode.gs1`                   | GS1 indicator, or None if not supplied by the reader.                      |
| `barcode.reader_initialization` | Reader initialization indicator, or None; never executed.                  |
| `barcode.structured_append`     | `StructuredAppend(index, count, id, parity)`, or None; index is one-based. |
| `barcode.ean_add_on`            | EAN/UPC supplement, with `ean_add_on_policy` `"Read"` or `"Require"`.      |

Separate labels with the same value stay separate entries. `barcode.as_dict()`
exports a single read, and `json.dumps(result.as_dict())` works directly.

`support` is an uncalibrated, reader-specific ranking heuristic, not a
probability, and is not comparable across formats. In a mixed-format image,
`best` is therefore not necessarily the most reliable read; select by format or
payload when your application knows what it expects. Checksums reduce
wrong reads but cannot rule them out.

`payload_bytes` is supplied by QR Code, Data Matrix, Aztec, PDF417 and MaxiCode.
It holds decoded data bytes before character-set interpretation, not raw
codewords; Aztec Rune gives its value as decimal ASCII. Encoding `text` as UTF-8
does not reconstruct these bytes.

Exported types include `ScanResult`, `InspectionResult`, `Barcode`, `Format`,
`FormatSelection`, `Mode`, `Layout`, `ValueRange`, `ColorOrder`, `ImageInput` and
`PixelImage`. The package includes `py.typed`.

## EAN/UPC supplements

Set `ean_add_on_policy` when creating a scanner or calling one-shot `scan` or `inspect`.

| Policy      | Behavior                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| `"Ignore"`  | Decode the main barcode without reading its supplement.                                          |
| `"Read"`    | Try reading the two- or five-digit supplement; keep the main barcode if none is readable.        |
| `"Require"` | Return an EAN/UPC barcode only when its supplement is readable. Other formats remain unaffected. |

The supplement appears in `barcode.ean_add_on`; `barcode.text` is the main payload,
and `polygon` and `rect` describe the main barcode. Reading supplements adds
decoding work. Retail reads rejected by `"Require"` appear in `result.undecoded`.

## Undecoded regions

`inspect` results list `undecoded` regions: localized proposals without an
accepted decode, each with a source-image `polygon` and a `format` hint
(`"Unknown"` when unavailable). They can overlap or be false candidates, and an
empty list does not prove that every barcode was found.

## Diagnostics

```python
report = tapirscan.inspect(image)
regions = report.diagnostics.regions
if regions is not None:
    print(regions.proposals, regions.search_windows, regions.candidates)
```

`proposals` and `search_windows` are None when a reader does not expose them, and
empty tuples when it found nothing. `report.diagnostics.barcodes` lists per-read
evidence; region types are in `tapirscan.results`. `report.to_raw_dict()` returns
the engine's own JSON evidence, whose fields vary by reader and may change between
releases. Candidate indices inside a recovery crop are local to that crop, not
identifiers for tracking between frames.

## Errors

Invalid input raises `ValueError` or `TypeError`. Native failures raise
`ScannerError` with a message and a numeric `.code`: 1 invalid arguments,
2 invalid or closed handle, 3 result buffer too small, 4 internal failure.
Scanning after `close()` raises `RuntimeError`. A custom native library from
`library_dir` must implement native ABI 6; mismatches are reported with
rebuild instructions.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

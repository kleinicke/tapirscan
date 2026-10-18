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
    print(barcode.text, barcode.format, barcode.rect)
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

Tapirscan has no image-library dependencies; install the ones your inputs need.

### TIFF with tifffile

```sh
pip install tapirscan tifffile
```

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

```sh
pip install tapirscan pillow
```

```python
from PIL import Image
import tapirscan

with Image.open("label.jpg") as image:
    result = tapirscan.scan(image, mode="high", formats="1D")

for barcode in result.barcodes:
    print(barcode.text, barcode.format, barcode.rect)
```

`formats="1D"` enables all linear formats. `"retail"` selects EAN13, UPCA, EAN8
and UPCE; `"common1D"` adds Code128, Code39 and ITF; `"common"` adds QRCode and
DataMatrix; `"2D"` and `"all"` are also available. See
[format coverage](https://github.com/kleinicke/tapirscan/blob/main/docs/FORMATS.md).

### PyTorch tensors

```sh
pip install tapirscan torch
```

```python
import torch

tensor = torch.from_numpy(pixels)  # the array from the TIFF example
result = tapirscan.scan(tensor)
print(result.values)
```

GPU tensors and tensors with `requires_grad=True` work directly: Tapirscan copies
the pixels to CPU without changing your tensor or its autograd graph. Decoding
runs on CPU. See [array and tensor inputs](#array-and-tensor-inputs).

## Fast scanning

- **Select only the formats you need.** Each enabled 2D format searches the whole
  image; `formats="all"` is for exploration.
- **Reuse a `Scanner`** for more than one image, and use `mode="low"` for camera
  frames.
- **Inputs, cheapest first:** contiguous uint8 NumPy arrays (RGBA, then RGB or gray,
  a small extra copy for large frames); uint8 tensors; float arrays and tensors (validated
  and rounded, on several threads for large arrays); Pillow images (one `tobytes()`
  copy). CUDA tensors are converted on the GPU and transferred once, so scan
  tensors where they already are; moving a CPU tensor to the GPU only for scanning
  does not pay off for uint8 and saves little for large float tensors. Do not
  convert RGBA to gray for scanning; for OpenCV, see below.
- **Large photos:** when the barcodes are large in the frame, let Pillow decode a
  JPEG at reduced size before it loads: `image = Image.open(path)`, then
  `image.draft("RGB", (image.width // 2, image.height // 2))`. A 12 MP photo with
  `formats="all"` then scans several times faster. Check that your
  smallest codes still read, and multiply coordinates by the factor to map them
  back to the original.
- **Batches:** `tapirscan.scan_many(images, ...)` scans on all CPU cores with one
  scanner per thread, reading a generator only a few images ahead. It already
  keeps every core busy, so its scanners run each scan on one thread
  (`parallel=False`).
- **Threads and servers:** scanning releases the GIL, so one scanner per thread
  scales with CPU cores. Create one per worker thread,
  for example with `threading.local()` or a `ThreadPoolExecutor` initializer. In
  FastAPI and other async frameworks, scan in a sync endpoint or thread pool so the
  event loop keeps running. With `multiprocessing`, create the scanner inside each
  process. Scans with several formats also use extra threads internally; pass
  `parallel=False` when you already run one scanner per core.

```python
import threading
from concurrent.futures import ThreadPoolExecutor

import tapirscan

local = threading.local()


def scan(image):
    if not hasattr(local, "scanner"):
        local.scanner = tapirscan.Scanner(formats=["EAN13", "QRCode"])
    return local.scanner.scan(image).values


with ThreadPoolExecutor() as pool:
    results = list(pool.map(scan, images))
```

See [fast end-to-end scanning](https://github.com/kleinicke/tapirscan/blob/main/docs/PERFORMANCE.md)
for measured costs.

## Functions

```python
# One image, with automatic cleanup:
result = tapirscan.scan(image, mode="medium", formats=["EAN13"])

# Reuse native initialization across images:
with tapirscan.Scanner(mode="high", formats="1D") as scanner:
    result = scanner.scan(image)
    print(result.values)

# Many images on all CPU cores, results in input order:
results = tapirscan.scan_many(images, formats=["EAN13", "QRCode"])
```

Signatures (all settings are optional). `inspect` and `scanner.inspect` take the
same arguments as their `scan` counterparts and return `InspectionResult`:

```text
scan(image, *, mode="medium", formats=None, ean_add_on_policy="ignore", parallel=None,
     layout="auto", value_range="auto", color_order="RGB", library_dir=None) -> ScanResult
Scanner(mode="medium", *, formats=None, ean_add_on_policy="ignore", parallel=None,
        library_dir=None)
scanner.scan(image, *, formats=None,
             layout="auto", value_range="auto", color_order="RGB") -> ScanResult
scanner.close()
scan_many(images, *, mode="medium", formats=None, ean_add_on_policy="ignore",
          parallel=False, layout="auto", value_range="auto", color_order="RGB",
          library_dir=None, workers=None) -> list[ScanResult]
```

`scan_many` scans on `workers` threads (default: the CPU count), each with its own
scanner, and returns results in input order. If an image fails, its error is
raised after the other scans finish.

Use the context manager or call `close()` in a `finally` block; repeated `close()`
is safe and results stay valid afterwards. Calls on one scanner run one at a time;
separate scanners can run concurrently ([threads and servers](#fast-scanning)).
`scanner.mode` and `scanner.formats` are
read-only; create another scanner to change effort. Per-call `formats` apply to
that call only; `None` uses the scanner's selection. A scanner copies each image
into a pixel buffer it reuses, as large as the largest image so far, until
`close()`.

## All options

| Option              | Where                     | Default         | Meaning                                                                                                                                                    |
| ------------------- | ------------------------- | --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mode`              | Creation / one-shot       | `"medium"`      | `"low"`, `"medium"`, `"high"`, `"very-high"`.                                                                                                              |
| `formats`           | Creation / scan           | retail          | An identifier, `"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"`, `"all"`, or a nonempty list/tuple of identifiers.                                     |
| `ean_add_on_policy` | Creation / one-shot       | `"ignore"`      | `"ignore"`, `"read"`, `"require"`; see [supplements](#eanupc-supplements).                                                                                 |
| `parallel`          | Creation / one-shot       | `None`          | Run one scan's formats on separate threads; results are identical. `None` is on unless `TAPIRSCAN_THREADS` is `0` or `1`. `scan_many` defaults to `False`. |
| `layout`            | Scan, arrays/tensors only | `"auto"`        | `"HW"`, `"HWC"`, `"CHW"`; specify when the channel position is ambiguous.                                                                                  |
| `value_range`       | Scan, arrays/tensors only | `"auto"`        | `"0_1"` or `"0_255"`. Auto uses [0,1] for floats and [0,255] for integers, independent of image contents.                                                  |
| `color_order`       | Scan, arrays/tensors only | `"RGB"`         | `"BGR"` for OpenCV BGR/BGRA pixels. Grayscale is unaffected.                                                                                               |
| `library_dir`       | Creation / one-shot       | Bundled library | Directory of a custom native build. Lookup order: this argument, `TAPIRSCAN_LIBRARY_DIR`, then the wheel's library.                                        |

Effort modes tune every format; see
[effort levels](../../docs/FORMATS.md#effort-levels). Format group exports: `retail_formats`, `common_linear_formats`,
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

Scanning always runs on the CPU. Large float NumPy arrays are converted on several
threads. CUDA tensors are validated and converted to bytes on the GPU, so only the final
8-bit pixels are transferred; MPS and other device tensors are copied to CPU first,
which adds latency. Sparse, quantized, complex and meta tensors are rejected.

### OpenCV

```python
import cv2
import tapirscan

image = cv2.imread("label.jpg")
if image is None:
    raise ValueError("Could not load label.jpg")
result = tapirscan.scan(image, color_order="BGR")
```

OpenCV is not a dependency. `color_order="BGR"` does not modify your array; it
reorders the channels in a copy. Converting with OpenCV is faster: for a 12 MP
frame, `scanner.scan(cv2.cvtColor(image, cv2.COLOR_BGR2RGBA))` is about twice as
fast as `color_order="BGR"`. For a camera or video, reuse one scanner:

```python
capture = cv2.VideoCapture(0)
with tapirscan.Scanner(mode="low", formats=["EAN13", "QRCode"]) as scanner:
    while True:
        ok, frame = capture.read()
        if not ok:
            break
        rgba = cv2.cvtColor(frame, cv2.COLOR_BGR2RGBA)
        for barcode in scanner.scan(rgba).barcodes:
            print(barcode.text, barcode.rect)
```

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

Images above 32 megapixels are scanned as a reduced copy; results stay in source
coordinates, but barcodes that are small in such an image may be missed. Images may have
at most 65,535 pixels per side, 256 megapixels and 1 GiB of pixel data; this is checked
before any conversion or device transfer. Coordinates start at the top left. If you
resize before scanning, map coordinates back when drawing on the original.

## Results

`scan` returns a `ScanResult`; `inspect` returns an `InspectionResult` with the
same fields plus the ones marked _inspect_. Results are immutable.

| Field/method                    | Meaning                                                                                     |
| ------------------------------- | ------------------------------------------------------------------------------------------- |
| `result.values`                 | List of decoded strings, one per barcode.                                                   |
| `result.barcodes`               | Tuple of immutable `Barcode` objects.                                                       |
| `result.best`                   | Highest-support barcode (first on ties), or None.                                           |
| `result.as_dict()`              | JSON-compatible dictionary of the public fields, without diagnostics.                       |
| `result.image`                  | _inspect_: `ImageSize(width, height)` of the supplied pixels.                               |
| `result.mode`                   | _inspect_: effort mode used.                                                                |
| `result.elapsed_ms`             | _inspect_: native scan time, excluding image conversion.                                    |
| `result.undecoded`              | _inspect_: [regions without a decode](#undecoded-regions).                                  |
| `result.diagnostics`            | _inspect_: [engine evidence](#diagnostics).                                                 |
| `barcode.text`, `.format`       | Decoded text and format identifier.                                                         |
| `barcode.polygon`               | Tuple of four `Point(x, y)` source-image coordinates.                                       |
| `barcode.rect`                  | Enclosing integer `Rect(left, top, width, height)`.                                         |
| `barcode.support`               | Reader-specific ranking evidence used by `best`.                                            |
| `barcode.payload_bytes`         | Decoded matrix payload bytes, or None.                                                      |
| `barcode.gs1`                   | GS1 indicator, or None if not supplied by the reader.                                       |
| `barcode.reader_initialization` | Data Matrix, PDF417, Aztec and MaxiCode reader-initialization flag; None for other formats. |
| `barcode.structured_append`     | `StructuredAppend(index, count, id, parity)`, or None; index is one-based.                  |
| `barcode.ean_add_on`            | EAN/UPC supplement, with `ean_add_on_policy` `"read"` or `"require"`.                       |

Separate labels with the same value stay separate entries. `barcode.as_dict()`
exports a single read, and `json.dumps(result.as_dict())` works directly.
Each exported barcode has a `content` entry, `{"kind", "fields": [{"name",
"value"}]}` or `None` for plain text. It is parsed when `as_dict()` runs, not
while scanning.

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

## Structured content

`parse_content(barcode)` returns the named fields of GS1, Wi-Fi, contact,
payment and similar payloads, or `None` for plain text. It also accepts a `str`.

```python
content = tapirscan.parse_content(barcode)  # or a str; gs1=True for element strings
if content and content.kind == "GS1":
    gtin, expiry = content.get("01"), content.get("17")
```

Kinds and fields: [structured content](../../docs/CONTENT.md).

## EAN/UPC supplements

Set `ean_add_on_policy` when creating a scanner or calling one-shot `scan` or `inspect`.

| Policy      | Behavior                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| `"ignore"`  | Decode the main barcode without reading its supplement.                                          |
| `"read"`    | Try reading the two- or five-digit supplement; keep the main barcode if none is readable.        |
| `"require"` | Return an EAN/UPC barcode only when its supplement is readable. Other formats remain unaffected. |

The supplement appears in `barcode.ean_add_on`; `barcode.text` is the main payload,
and `polygon` and `rect` describe the main barcode. Reading supplements adds
decoding work. Retail reads rejected by `"require"` appear in `result.undecoded`.

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
evidence; region types are in `tapirscan.results`. `report.diagnostics.to_raw_dict()`
returns the engine's own JSON evidence, whose fields vary by reader and may change between
releases. Candidate indices inside a recovery crop are local to that crop, not
identifiers for tracking between frames.

## Errors

Invalid input raises `ValueError` or `TypeError`. Native failures raise
`ScannerError` with a message, the native `.status` number and a `.code` shared
with JavaScript: `"invalid_input"`, `"disposed"` or `"engine"`.
Scanning after `close()` raises `RuntimeError`. A custom native library from
`library_dir` must implement native ABI 7; mismatches are reported with
rebuild instructions.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

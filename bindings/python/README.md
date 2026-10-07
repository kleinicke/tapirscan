# Tapirscan for Python

`scan(image)` returns `list[Barcode]`. Each barcode has `text`, `format`, and
source-image `polygon` / `rect` coordinates. No detection returns `[]`.

```python
import tapirscan

barcodes = tapirscan.scan(image)
for barcode in barcodes:
    print(barcode.text, barcode.polygon)
```

Reuse `Scanner.scan(image)` for repeated calls. Use `inspect(image)` or
`Scanner.inspect(image)` for timing, work status, unread regions and diagnostics.
There is no `debug` argument. Both operations accept Pillow images, NumPy arrays
and PyTorch tensors.
[Try the live demo](https://tapirscan.f-kleinicke.de) · [Quick start](#quick-start) · [Functions](#functions) · [All options](#all-options) · [Results](#results-and-public-types)

## Installation

Python 3.10+ is required. Platform wheels bundle the native library with all four modes.
Install from [PyPI](https://pypi.org/project/tapirscan/) with `pip install tapirscan`.

## Quick start

Install `tapirscan` plus the image libraries you use. For these examples:

```sh
pip install tapirscan tifffile pillow torch
```

**TIFF → NumPy array, using defaults:**

```python
import tifffile
import tapirscan

pixels = tifffile.imread("label.tif", key=0)  # NumPy array: first TIFF page
barcodes = tapirscan.scan(pixels)
print([barcode.text for barcode in barcodes])
```

This example assumes an 8-bit grayscale or RGB image. Defaults are Medium effort
and the retail formats (EAN13, UPCA, EAN8, UPCE). No intermediate file or explicit scanner object is needed.

**JPEG, with optional settings:**

```python
from PIL import Image
import tapirscan

with Image.open("label.jpg") as image:
    barcodes = tapirscan.scan(image, mode="high", formats="1D")

for barcode in barcodes:
    print(barcode.text, barcode.format, barcode.polygon)
```

`formats="1D"` enables all supported linear formats; `"2D"` and `"all"` are also
available. See [format coverage](../../docs/FORMATS.md) for supported formats and variants.

`formats="retail"` selects EAN13, UPCA,
EAN8 and UPCE. `"common1D"` adds Code128, Code39 and ITF; `"common"` adds
QRCode and DataMatrix to `"common1D"`. See
[format presets and runtime behavior](../../docs/FORMATS.md).

**PyTorch tensor:**

```python
import torch

# Using the NumPy array from the TIFF example:
tensor = torch.from_numpy(pixels)
barcodes = tapirscan.scan(tensor)
print([barcode.text for barcode in barcodes])
```

GPU tensors and tensors with `requires_grad=True` work directly. Tapirscan
detaches internally and transfers pixels to CPU; your tensor and its autograd
graph are unchanged. Barcode decoding runs on CPU.

## Functions

```python
# One image, with automatic cleanup:
barcodes = tapirscan.scan(image, mode="medium", formats=["EAN13"])

# Reuse native initialization across images:
with tapirscan.Scanner(mode="high", formats="1D") as scanner:
    barcodes = scanner.scan(image)
    best = tapirscan.best(barcodes)  # Barcode or None; all reads remain in barcodes
```

Signatures (all settings are optional). `inspect` and `scanner.inspect` accept
the same arguments as their `scan` counterparts and return `ScanResult`:

```text
scan(image, *, mode="medium", formats=None, ean_add_on_policy="Ignore", extended_budget=False,
     layout="auto", value_range="auto", color_order="RGB", library_dir=None) -> list[Barcode]
Scanner(mode="medium", *, formats=None, ean_add_on_policy="Ignore", library_dir=None)
scanner.scan(image, *, formats=None, extended_budget=False,
             layout="auto", value_range="auto", color_order="RGB") -> list[Barcode]
scanner.close()
```

The one-shot helper closes its scanner on success or failure. For repeated images,
use the context manager above or `close()` in a `finally` block. Repeated close is
safe. Results survive closure. Calls on one scanner serialize; separate scanners
can run concurrently. `scanner.mode` and `scanner.formats` are read-only. Create another scanner to change effort. Per-call formats
override the constructor selection for that call only; `None` inherits it.

## All options

| Option              | Where                     | Default         | Meaning                                                                                                                                                                         |
| ------------------- | ------------------------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `image`             | Scan                      | Required        | Pillow image, NumPy array, PyTorch tensor, or `PixelImage`. Encoded bytes and filenames must be decoded first.                                                                  |
| `mode`              | Creation / one-shot       | `"medium"`      | `"low"`, `"medium"`, `"high"`, `"very-high"`: select EAN13/UPCA, Common1D and QR Code search effort.                                                                            |
| `formats`           | Creation / scan           | retail          | A single identifier, `"retail"`, `"common1D"`, `"common"`, `"1D"`, `"2D"`, `"all"`, or a nonempty list/tuple of exact identifiers. On a scanner, `None` inherits its selection. |
| `ean_add_on_policy` | Creation / one-shot       | `"Ignore"`      | `"Ignore"`, `"Read"`, `"Require"`; optional EAN/UPC supplement policy.                                                                                                          |
| `extended_budget`   | Scan / one-shot           | `False`         | Allow extra reader work for any format. Exact budgets may evolve.                                                                                                               |
| `layout`            | Scan, arrays/tensors only | `"auto"`        | `"HW"`, `"HWC"`, `"CHW"`; specify when channel position is ambiguous.                                                                                                           |
| `value_range`       | Scan, arrays/tensors only | `"auto"`        | `"0_1"` or `"0_255"`. Auto uses [0,1] for all floats and [0,255] for integers, independently of image contents. Byte-unit floats require `"0_255"`.                             |
| `color_order`       | Scan, arrays/tensors only | `"RGB"`         | Optional `"BGR"` for OpenCV BGR/BGRA pixels; alpha is preserved and ignored by decoding. Grayscale is unchanged.                                                                |
| `library_dir`       | Creation / one-shot       | Bundled library | Path/string for custom native builds. Lookup: explicit path, then `TAPIRSCAN_LIBRARY_DIR`, then the wheel's library. The working directory is never searched implicitly.        |

Custom native libraries must implement ABI 6. Scanner creation reports ABI
mismatches with version details and rebuild instructions.

All scans return all decoded instances, including spatially separate copies of the
same value. Use `tapirscan.best(barcodes)` on any barcode list, or `result.best`
on an inspection result, for one highest-support read, keeping the first on ties;
this does not reduce scanning work. Support is a ranking heuristic, not a confidence probability.

Formats and group exports: `Format`, `FormatSelection`, `retail_formats`,
`common_formats`, `common_linear_formats`, `linear_formats`, `matrix_formats`. See [identifiers and reader limitations](../../docs/FORMATS.md).
Retail formats (EAN13, UPCA, EAN8 and UPCE) are enabled by default; select additional supported formats as needed. Modes tune EAN13/UPCA, Common1D and QR Code; other matrix readers use fixed effort.
ROI, resizing, rotation and camera acquisition belong to the caller. Exact work
budgets, timeouts and confidence thresholds are not exposed as scan options.

## EAN/UPC supplements

Set `ean_add_on_policy="Read"` in Python when creating a scanner or calling one-shot `inspect()`.
The policy is fixed for that scanner; its default is `"Ignore"`.

| Policy      | Behavior                                                                                         |
| ----------- | ------------------------------------------------------------------------------------------------ |
| `"Ignore"`  | Decode the main barcode without reading its supplement.                                          |
| `"Read"`    | Try reading the two- or five-digit supplement; keep the main barcode if none is readable.        |
| `"Require"` | Return an EAN/UPC barcode only when its supplement is readable. Other formats remain unaffected. |

`barcode.polygon` and `barcode.rect` describe the main barcode, excluding the
supplement. Supplement geometry is not exposed separately.

The supplement appears separately in `barcode.ean_add_on`; `barcode.text` remains
the main payload. Reading supplements enables additional decoding
work independently of the effort mode. Retail reads rejected
by `"Require"` remain available in `result.undecoded`.

## Evidence and work limits

Most applications need `barcode.text`, `.format`, `.polygon` and `.rect`.
`barcode.support` exposes the evidence used by `.best`. It is an uncalibrated,
reader-specific ranking heuristic, not a certainty percentage; values are not
comparable confidence across formats or effort modes. Consequently, `.best` means the largest support value,
not the most reliable barcode in a mixed-format image. Select by the format or
payload your application needs when that distinction matters. Checksums and consistency
checks reduce wrong reads but cannot guarantee that every returned decode is correct.

Use `mode="low"` through `"very-high"` at creation to select EAN13/UPCA, Common1D and QR Code search
effort. Other matrix readers use fixed effort. `result.unfinished` is available without
and combines reported decoding and localization limits. Returned reads are
still usable. Candidate, retry and parsing caps are reported, including bounded
searches that also returned reads. False does not promise exhaustive scanning. Exact budgets and interruptible timeouts are not
public options.

Inspection includes attempted search windows, localization proposals, candidate
outcomes and engine traces. It is unnecessary for drawing decoded barcode locations.

Pillow `I`, `F` and `I;16*` images use byte-unit intensities in [0,255], matching
Pillow's usual conversion convention. Nonfinite and out-of-range pixels are rejected
rather than silently clipped. Scale higher-bit-depth images explicitly; array/tensor
`value_range` overrides do not apply to Pillow images.

## Raw pixels

```python
image = tapirscan.PixelImage(pixels, width=640, height=480, channels=3)
result = tapirscan.inspect(image)
```

`pixels` contains decoded bytes, not a JPEG/PNG file. Dimensions and storage
settings live on the image, so scan options always describe scanning.

| `PixelImage` field | Default                | Meaning                                                  |
| ------------------ | ---------------------- | -------------------------------------------------------- |
| `data`             | Required, positional   | `bytes`, `bytearray`, or a contiguous byte `memoryview`. |
| `width`, `height`  | Required, keyword-only | Integer dimensions, at least 3 pixels each.              |
| `channels`         | `1`                    | 1 grayscale, 3 RGB, 4 RGBA. Alpha is ignored.            |
| `stride`           | `width * channels`     | Bytes between row starts. Larger values allow padding.   |

Storage must cover `(height - 1) * stride + width * channels` bytes and fit the
128 MiB input limit. Images are limited to 32 megapixels (33,554,432 pixels), checked before copying
raw buffers, converting Pillow/NumPy inputs, or transferring tensors to CPU. Scanning snapshots the addressed bytes before the native
call. `layout`, `value_range` and `color_order` overrides apply only to arrays/tensors, not `PixelImage` or Pillow.

## Array and tensor inputs

Accepts one HW/HWC/CHW image, optionally with a leading batch axis of size one,
with 1/3/4 channels. NumPy views and noncontiguous tensors are accepted. Colors
default to RGB/RGBA. OpenCV users can opt into BGR/BGRA with `color_order="BGR"`. Booleans become black/white. NaN, infinity and
out-of-range values are rejected; explicitly scale higher-bit-depth intensities
and undo mean/std normalization. The input buffer limit is 128 MiB.

GPU tensors (including CUDA and MPS) and `requires_grad=True` tensors can be passed
directly. The adapter detaches internally, copies to CPU as needed, and makes
pixels contiguous without changing the input or its autograd graph. The scan is
not differentiable and decoding does not run on GPU. Device transfer adds latency.
Sparse, quantized, complex and meta tensors are rejected.

### Optional OpenCV input

```python
import cv2
import tapirscan

image = cv2.imread("label.jpg")
if image is None:
    raise ValueError("Could not load label.jpg")
result = tapirscan.inspect(image, color_order="BGR")
```

OpenCV is not a dependency. Existing RGB, Pillow and grayscale calls need no new
argument. BGR conversion leaves your array or tensor unchanged, supports CHW/HWC
and preserves alpha. Floating-point images default to unit intensities; explicitly
use `value_range="0_255"` for floats stored in byte units.

## Results and public types

`scan` returns a list of `Barcode` objects. The `result.*` fields below belong
to the `ScanResult` returned by `inspect`; `barcode.*` fields are available from
both operations.

`ScanResult` is an immutable sequence: iterate, index, slice, use `len(result)` or
check its truth value. Empty results are false.

| Field/method                    | Meaning                                                                                                         |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `result.barcodes`               | Tuple of immutable `Barcode` objects.                                                                           |
| `result.values`                 | Fresh list of decoded strings.                                                                                  |
| `result.best`                   | Highest-support barcode, or None. Support is not a confidence probability.                                      |
| `result.image`                  | `ImageSize(width, height)` of supplied pixels.                                                                  |
| `result.mode`                   | Applied effort mode.                                                                                            |
| `result.elapsed_ms`             | Native scanner time; excludes image conversion and result construction.                                         |
| `result.unfinished`             | Incomplete scanning work; returned reads can still be useful.                                                   |
| `result.undecoded`              | Localized proposals without accepted decodes; always available.                                                 |
| `result.diagnostics`            | `Diagnostics`, from inspection.                                                                                 |
| `result.as_dict()`              | Independent JSON-compatible application result, including decoded and undecoded geometry; excludes diagnostics. |
| `result.to_raw_dict()`          | Independent native schema-2 JSON; contains the inspection evidence.                                             |
| `barcode.payload_bytes`         | Immutable decoded payload bytes before character-set interpretation, or None when unavailable.                  |
| `barcode.text`, `.format`       | Decoded text and format identifier.                                                                             |
| `barcode.polygon`               | Tuple of `Point(x, y)` source-image coordinates.                                                                |
| `barcode.rect`                  | Enclosing integer `Rect(left, top, width, height)`.                                                             |
| `barcode.support`               | Reader-specific ranking evidence; not confidence or a probability.                                              |
| `barcode.gs1`                   | GS1 indicator, or None if not supplied by the reader.                                                           |
| `barcode.reader_initialization` | Whether the payload is reader initialization data, or None if unspecified; never executed.                      |
| `barcode.structured_append`     | Immutable `StructuredAppend(index, count, id, parity)`, or None; index is one-based.                            |
| `barcode.ean_add_on`            | Optional EAN supplement text; populated when `ean_add_on_policy` is `"Read"` or `"Require"`.                    |

Coordinates start at the top left. The API returns geometry, not a cropped bitmap.
If you resize before scanning, map coordinates back when drawing on the original.
Other exported option types are `Mode`, `Layout`, `ValueRange`, `ColorOrder`, `ImageInput` and `PixelImage`.
The package includes `py.typed` for static type checkers.

### Saving or returning results

```python
import json

payload = result.as_dict()  # also works after scanner.close()
print(json.dumps(payload))
```

`as_dict()` uses public Python field names, polygon coordinate pairs and rectangle
objects. Payload bytes become lists of integers (or null). Each call returns an
independent nested dictionary; no private bytes or native-schema fields leak into
it. `barcode.as_dict()` exports a single read. Export inspection evidence separately
with `result.diagnostics.to_raw_dict()` when requested.

`payload_bytes` is currently supplied by QR Code, Data Matrix, Aztec, PDF417 and
MaxiCode readers. These are decoded data bytes, not error-correction codewords;
Aztec Rune represents its numeric value as decimal ASCII. Other readers return
None. UTF-8 encoding `.text` is not a substitute for original payload bytes.
Unsupported character encodings can still prevent a decode; this change preserves
the readers' existing decoding behavior.

## Diagnostics

```python
result = tapirscan.inspect(image)
if result.diagnostics is not None and result.diagnostics.regions is not None:
    print(result.diagnostics.regions.proposals)
    print(result.diagnostics.regions.search_windows)
    print(result.diagnostics.regions.undecoded)
    print(result.diagnostics.regions.candidates)
```

`regions.undecoded` contains immutable `UndecodedRegion` objects with `polygon`
and a `format` hint (`"Unknown"` when unavailable). These are geometry evidence,
not decoded `Barcode` objects: they have no text or payload. Use `result.barcodes`
for successful reads.
`proposals` and `search_windows` are None when that reader does not expose them;
an empty tuple means evidence was available but contained no entries.

`Diagnostics` also exposes `barcodes` (support, axis, candidate indices in result
order), `localization_limited` and `to_raw_dict()`. Region types are available in
`tapirscan.results`. GS1, reader initialization and structured append are available directly on each
barcode during ordinary scanning; raw metadata is also retained in JSON. Evidence varies by reader. Candidate indices inside
a recovery crop are local to that crop, not identifiers for tracking across frames.

## Errors

Invalid input raises ValueError/TypeError; native errors raise `ScannerError`
with a descriptive message and a numeric `.code` attribute. Native codes are:
1 invalid arguments, 2 invalid/closed handle, 3 result buffer too small,
4 internal failure, and 5 resource capacity exceeded. For capacity errors, close
unused scanners. Unknown codes retain their number. Scanning after close raises RuntimeError.

`result.to_raw_dict()` and `result.diagnostics.to_raw_dict()` return independent native
schema-2 dictionaries. Inspection results include engine evidence,
because public undecoded geometry uses that evidence. These are engine exports,
not serialization of the
public Python object. Support is available directly as `barcode.support` and in `result.diagnostics.barcodes`.

## Extended work budget

Use `scanner.inspect(image, extended_budget=True)` to allow additional reader work. The default
is false. This option is valid for every format; the exact budgets and stages are
implementation details that may evolve. Effort mode remains a separate setting.

Today this relaxes shared EAN-13/UPC-A retry and association limits. Other readers
currently retain their existing budgets. Per-candidate limits and intentional
deferrals remain; `unfinished` can still be true. This is not unlimited search,
an exhaustiveness guarantee or a wall-clock deadline. Custom primary-reader
engines must support the extended-work capability or report an error.

## Undecoded regions

`result.undecoded` is always available, in inspection reports. Each entry has
a source-image polygon and a format hint. It is a localized proposal without an
accepted decode, not proof of a real or permanently unreadable barcode. Entries
can overlap or describe false candidates. An empty collection does not prove
that every barcode was found. Raw candidate attempts remain in debug diagnostics.

## License

Tapirscan is dual-licensed under **MIT OR Apache-2.0**, at your option.
See the [full license texts](https://tapirscan.f-kleinicke.de/license/).
Third-party components retain their own licenses and notices.

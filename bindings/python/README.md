# Tapirscan for Python

Scan Pillow images, NumPy arrays and PyTorch tensors with `tapirscan.scan(image)`.
[Quick start](#quick-start) · [Functions](#functions) · [All arguments](#all-arguments) · [Results](#results-and-public-types)

## Installation

Python 3.10+ is required. Platform wheels bundle all four native modes.
Registry publication is pending; until then, use the [local build guide](../../docs/DEVELOPMENT.md).

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
result = tapirscan.scan(pixels)
print(result.values)
```

This example assumes an 8-bit grayscale or RGB image. Defaults are Medium effort
and EAN-13. No intermediate file or explicit scanner object is needed.

**JPEG, with optional settings:**

```python
from PIL import Image
import tapirscan

with Image.open("label.jpg") as image:
    result = tapirscan.scan(image, mode="high", formats="1D")

for barcode in result:
    print(barcode.text, barcode.format, barcode.polygon)
```

`formats="1D"` enables all supported linear formats; `"2D"` and `"all"` are also
available. Additional formats are experimental.

**PyTorch tensor:**

```python
import torch

# Using the NumPy array from the TIFF example:
tensor = torch.from_numpy(pixels)
result = tapirscan.scan(tensor)
print(result.values)
```

GPU tensors and tensors with `requires_grad=True` work directly. Tapirscan
detaches internally and transfers pixels to CPU; your tensor and its autograd
graph are unchanged. Barcode decoding runs on CPU.

## Moving from pyzbar or ZXing-C++

For a Pillow image or grayscale NumPy array, an EAN-13 values-only call changes
like this:

```python
# Before (pyzbar):
values = [barcode.data.decode("utf-8") for barcode in decode(image)]
# Before (zxing-cpp):
values = [barcode.text for barcode in zxingcpp.read_barcodes(image)]
# After (Tapirscan):
values = tapirscan.scan(image).values
```

The old calls assume their existing imports. Select your application's formats
explicitly when it needs more than EAN13, for example `formats=["EAN13", "Code128"]`.
Additional readers are experimental; verify coverage before replacing an existing
scanner. API conventions are documented by [pyzbar](https://github.com/NaturalHistoryMuseum/pyzbar)
and [ZXing-C++](https://github.com/zxing-cpp/zxing-cpp/tree/master/wrappers/python).

| Existing assumption     | What to check when migrating                                                                     |
| ----------------------- | ------------------------------------------------------------------------------------------------ |
| All formats enabled     | Tapirscan defaults to EAN13. Use explicit formats or a preset.                                   |
| OpenCV BGR arrays       | Convert to RGB or grayscale; Tapirscan uses RGB luminance.                                       |
| `.data` or `.text`      | Prefer `.text`; Tapirscan's `.data` is UTF-8 encoded text, not a general raw binary-payload API. |
| Format strings / enums  | Map to Tapirscan identifiers; names and types can differ.                                        |
| Polygon / position      | Use `.polygon` or `.rect`; geometry need not match another reader's boundary.                    |
| Orientation / quality   | `.orientation` is None; `.quality` is not comparable to ZBar's score.                            |
| Reader-specific options | Options such as pyzbar's `symbols` do not transfer unchanged.                                    |

Start by running both libraries on your own images and comparing missing values,
wrong reads and runtime. Preserve separate copies of equal-value symbols when
counting instances. Reuse a `Scanner` for repeated images. A matching function
shape alone does not make this a drop-in replacement.

## Functions

### Single-image function

```text
tapirscan.scan(
    image, width=None, height=None, *,
    mode="medium", library_dir=None,
    channels=1, stride=None, multiple=True, debug=False,
    include_regions=None, formats=None, layout="auto", value_range="auto",
)
```

Returns `ScanResult`. Creates and closes a scanner automatically, including on
failure. `image` is required; all other arguments are optional. Arguments after
`*` must be passed by keyword. `width` and `height` are only needed for raw buffers.

### Reusable scanner

```text
tapirscan.Scanner(mode="medium", *, formats=None, library_dir=None)
scanner.scan(
    image, width=None, height=None, *,
    channels=1, stride=None, multiple=True, debug=False,
    include_regions=None, formats=None, layout="auto", value_range="auto",
)
scanner.close()
```

Use it for successive images to avoid initialization on every call:

```python
import tapirscan

with tapirscan.Scanner(mode="high", formats="1D") as scanner:
    for image in images:
        result = scanner.scan(image)
        print(result.values)
```

The context manager calls `close()` even on exceptions. You can instead call
`close()` in a `finally` block; repeated close is safe. Results remain valid after
closure. Calls on one scanner serialize; separate scanners can run concurrently.
Create another scanner to change mode. Per-call formats override the constructor
selection for that call only; None inherits it.

## All arguments

| Argument          | Default    | Accepted values and behavior                                                                                                             |
| ----------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `image`           | Required   | Pillow image, NumPy array, PyTorch tensor, `(pixels, width, height)` grayscale tuple, or raw byte buffer with explicit dimensions.       |
| `mode`            | `"medium"` | `"low"`, `"medium"`, `"high"`, `"very-high"`. Constructor/module function only.                                                          |
| `formats`         | None       | Default EAN13; `"1D"`, `"2D"`, `"all"`, or nonempty list/tuple of exact identifiers. On a reusable scanner, None inherits its selection. |
| `library_dir`     | None       | Path/string for custom native builds. Constructor/module function only.                                                                  |
| `width`, `height` | None       | Both required for raw buffers, inferred for image objects. Minimum 3 pixels each.                                                        |
| `channels`        | `1`        | Raw buffers only: 1 grayscale, 3 RGB, 4 RGBA. Alpha ignored.                                                                             |
| `stride`          | None       | Raw buffers only: bytes between row starts; defaults to `width * channels`. Padding allowed.                                             |
| `multiple`        | True       | False keeps only the highest-support read after scanning; no early exit.                                                                 |
| `debug`           | False      | Include search diagnostics in `result.debug`. Polygons are always returned.                                                              |
| `include_regions` | None       | Compatibility alias for `debug`; prefer `debug` in new code.                                                                             |
| `layout`          | `"auto"`   | NumPy/tensors: `"HW"`, `"HWC"`, `"CHW"`. Specify when channel position is ambiguous.                                                     |
| `value_range`     | `"auto"`   | NumPy/tensors: `"0_1"` or `"0_255"`. Auto scales floats in [0,1]; otherwise values must fit [0,255].                                     |

Library lookup: explicit `library_dir`, then `TAPIRSCAN_LIBRARY_DIR`, then bundled
wheel libraries. The working directory is never searched implicitly.

Formats and group exports: `Format`, `FormatSelection`, `retail_formats`,
`linear_formats`, `matrix_formats`. See [exact identifiers and reader limitations](../../docs/FORMATS.md).
Presets cover supported symbologies. Additional readers use fixed effort; the
four effort modes tune the EAN13/UPCA path.

## Array and tensor inputs

Accepts one HW/HWC/CHW image, optionally with a leading batch axis of size one,
with 1/3/4 channels. NumPy views and noncontiguous tensors are accepted. Colors
are RGB/RGBA, not OpenCV BGR. Booleans become black/white. NaN, infinity and
out-of-range values are rejected; explicitly scale higher-bit-depth intensities
and undo mean/std normalization. The input buffer limit is 128 MiB.

GPU tensors (including CUDA and MPS) and `requires_grad=True` tensors can be passed
directly. The adapter detaches internally, copies to CPU as needed, and makes
pixels contiguous without changing the input or its autograd graph. The scan is
not differentiable and decoding does not run on GPU. Device transfer adds latency.
Sparse, quantized, complex and meta tensors are rejected.

## Results and public types

`ScanResult` is an immutable sequence: iterate, index, slice, use `len(result)` or
check its truth value. Empty results are false.

| Field/method               | Meaning                                                                    |
| -------------------------- | -------------------------------------------------------------------------- |
| `result.barcodes`          | Tuple of immutable `Barcode` objects.                                      |
| `result.values`            | Fresh list of decoded strings.                                             |
| `result.best`              | Highest-support barcode, or None. Support is not a confidence probability. |
| `result.image`             | `ImageSize(width, height)` of supplied pixels.                             |
| `result.mode`, `.multiple` | Applied effort and result-selection options.                               |
| `result.elapsed_ms`        | Native scanner time; excludes image conversion and result construction.    |
| `result.unfinished`        | Incomplete scanning work; returned reads can still be useful.              |
| `result.debug`             | `Diagnostics`, or None when not requested.                                 |
| `result.to_dict()`         | Independent copy of the native schema-2 JSON, with requested diagnostics.  |
| `barcode.text`, `.format`  | Decoded text and format identifier.                                        |
| `barcode.polygon`          | Tuple of `Point(x, y)` source-image coordinates.                           |
| `barcode.rect`             | Enclosing integer `Rect(left, top, width, height)`.                        |

Coordinates start at the top left. The API returns geometry, not a cropped bitmap.
If you resize before scanning, map coordinates back when drawing on the original.
Other exported option types are `Mode`, `Layout`, `ValueRange` and `ImageInput`.
The package includes `py.typed` for static type checkers.

## Diagnostics

```python
result = tapirscan.scan(image, debug=True)
if result.debug is not None and result.debug.regions is not None:
    print(result.debug.regions.proposals)
    print(result.debug.regions.search_windows)
    print(result.debug.regions.candidates)
```

`Diagnostics` also exposes `barcodes` (support, axis, candidate indices in result
order), `localization_limited` and `to_dict()`. Region types are available in
`tapirscan.results`. Reader-specific raw metadata, including GS1 and structured
append, is retained in JSON. Evidence varies by reader. Candidate indices inside
a recovery crop are local to that crop, not identifiers for tracking across frames.

## Errors and migration helpers

Invalid input raises ValueError/TypeError; native errors raise `ScannerError`
with a `.code` attribute. Scanning after close raises RuntimeError.

`tapirscan.pyzbar.decode` aliases `tapirscan.scan`. Barcode `.data` returns UTF-8
bytes, `.type` aliases format, `.quality` exposes the support heuristic, and
`.orientation` is None. These ease migration but are not complete pyzbar
compatibility; there is no `symbols` argument. Use the main API for new code.

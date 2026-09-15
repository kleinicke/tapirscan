# Result selection and Python API direction

Implemented defaults:

- Multiple decoded barcodes: enabled.
- Localization and attempted-region evidence: omitted unless requested.
- One-result choice: select the highest-support read after full scanning; no early exit.
- A stable barcode-list result type: zero entries on failure, at most one in single mode.
- Decoded positions, input dimensions and unfinished flag: always available.

Python uses one typed result interface (see [Python guide](../bindings/python/README.md)):

```python
import tapirscan

result = tapirscan.scan(image, multiple=False, debug=True)
print(result.values)
if result.debug is not None and result.debug.regions is not None:
    localized = result.debug.regions.proposals
    search_windows = result.debug.regions.search_windows
    candidates = result.debug.regions.candidates
```

Search coverage is not a claim of exhaustive decoding. Candidate evidence remains
separate from the selected decoded-result list, and localization confidence is
not interchangeable with decode support. Effort levels and one/multiple are
independent axes. Single mode can still return a wrong read; support is uncalibrated.

## Python image inputs and migration

`tapirscan.scan(image)` and `scanner.scan(image)` return the same immutable,
iterable `ScanResult`. Simple consumers use `.values` or iterate barcodes and read
`.text`/`.data`; advanced consumers access typed status and optional `.debug`.
`to_dict()` preserves the complete native diagnostics as independent JSON data.

The old `tapirscan.pyzbar.decode` import is a direct alias, not a separate
API implementation or result conversion. `.quality` aliases support and is not
comparable to ZBar quality; `.orientation` remains None. EAN-13 is the default; additional formats are opt-in.
The earlier unpublished `symbols` parameter and dictionary-returning scan contract
have been removed. Multiple results remain the default.

Input preparation:

- Pillow: grayscale `L` stays grayscale; other modes convert to RGB before native
  luminance conversion. Alpha is ignored.
- NumPy: the same layout/range rules as tensors below. Noncontiguous views and
  ndarray subclasses work. Colors are RGB/RGBA; convert OpenCV BGR to RGB first.
  Out-of-range values are rejected instead of silently cast to uint8.
- Raw tuple: `(pixels, width, height)` containing exactly width*height grayscale
  bytes. Encoded JPEG/PNG bytes must first be opened with Pillow or OpenCV.
- PyTorch: dense real HW, CHW or HWC image tensors, with 1/3/4 channels and an
  optional leading batch dimension of size 1. CPU and accelerator tensors are
  automatically detached, synchronously copied to CPU, and made contiguous.
  No original pixels or autograd state are modified; scanning is not differentiable.
  Tensor colors are RGB/RGBA and alpha is ignored. uint8/integer values use
  [0,255], booleans use black/white, floating values use [0,1] when all values
  fit that range, otherwise [0,255]. Use `value_range="0_1"` or `"0_255"`
  to resolve dark-image ambiguity. Use `layout="CHW"` or `"HWC"` when both
  first and last axes could be channels. NaN/infinity, values outside these
  ranges, batches larger than one, sparse/quantized/complex/meta tensors are
  rejected clearly. Undo model-specific mean/std normalization before scanning.

```python
result = scanner.scan(tensor, debug=True)  # even CUDA + requires_grad
reads = tapirscan.scan(tensor, multiple=False, library_dir="build/native")
```

Pillow, NumPy and torch remain optional: install only the libraries used to
create your images. Torch conversion does not require NumPy. Package extras
`pillow`, `numpy`, and `torch` describe these optional dependencies.

## Installation

The package and import name are `tapirscan`. Version 1.0.0 platform wheels bundle
all four native modes and load them from the installed package. Callers do not
need Rust, ZBar or ZXing. Source checkouts can explicitly select a native build
with `library_dir` or `TAPIRSCAN_LIBRARY_DIR`; the current directory is never
searched implicitly. The first registry publication and cross-platform artifact
validation remain separate steps. See [release preparation](RELEASING.md).

## Provenance

[Pyzbar is MIT licensed](https://github.com/NaturalHistoryMuseum/pyzbar/blob/master/LICENSE.txt).
It is copyrighted; the license permits use, modification and redistribution subject
to retaining its copyright and permission notices in copies or substantial portions.
An independently implemented convenience interface need not copy pyzbar source.
No pyzbar code or ZBar engine is incorporated here, and this does not select the
license for the scanner itself. Copying code later requires preserving its notices.

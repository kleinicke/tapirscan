"""Convert optional image objects without importing unused imaging packages."""

from __future__ import annotations

import ctypes
import os
import sys
from concurrent.futures import ThreadPoolExecutor
from functools import cache
from math import isfinite
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    from collections.abc import Iterator

    import numpy as np
    from numpy.typing import NDArray
    from torch import Tensor

    from .inputs import ImageInput
    from .results import ColorOrder, Layout, ValueRange

LIMIT = 1024 * 1024 * 1024
MAX_IMAGE_PIXELS = 256 * 1024 * 1024
MAX_SIDE = 65_535
MAX_PIXEL = 255
MIN_SIZE = 3
HW_DIMENSIONS = 2
COLOR_DIMENSIONS = 3
BATCH_DIMENSIONS = 4
CHANNELS = (1, 3, 4)
BGR_CHANNELS = {3: [2, 1, 0], 4: [2, 1, 0, 3]}
# Float samples rounded per block: a few MiB of float64 stays in cache.
ROUND_BLOCK = 1 << 18
# Arrays with at least this many samples are rounded on up to 8 threads.
PARALLEL_SAMPLES = 1 << 22
MAX_CONVERT_THREADS = 8
# CPU tensors round larger blocks: PyTorch spreads each operation over threads.
# 3 Mi samples (24 MiB of float64) measured fastest overall on x86-64 and Apple M1.
TENSOR_BLOCK = 3 << 20


def _size(width: object, height: object, channels: int = 1) -> tuple[int, int]:
    if (
        isinstance(width, bool)
        or isinstance(height, bool)
        or not isinstance(width, int)
        or not isinstance(height, int)
        or width < MIN_SIZE
        or height < MIN_SIZE
    ):
        msg = "Image dimensions must be integers >=3"
        raise ValueError(msg)
    if max(width, height) > MAX_SIDE:
        msg = "Image sides must be at most 65,535 pixels"
        raise ValueError(msg)
    if width * height > MAX_IMAGE_PIXELS:
        msg = "Image exceeds 256 megapixel limit (268,435,456 pixels)"
        raise ValueError(msg)
    if width * height * channels > LIMIT:
        msg = "Image exceeds 1 GiB limit"
        raise ValueError(msg)
    return width, height


def _validate_options(
    layout: Layout, value_range: ValueRange, color_order: ColorOrder
) -> None:
    if color_order not in ("RGB", "BGR"):
        msg = "color_order must be RGB or BGR"
        raise ValueError(msg)
    if layout not in ("auto", "HW", "HWC", "CHW"):
        msg = "layout must be auto, HW, HWC or CHW"
        raise ValueError(msg)
    if value_range not in ("auto", "0_1", "0_255"):
        msg = "value_range must be auto, 0_1 or 0_255"
        raise ValueError(msg)


def image_bytes(
    image: ImageInput,
    *,
    layout: Layout = "auto",
    value_range: ValueRange = "auto",
    color_order: ColorOrder = "RGB",
) -> tuple[bytes | memoryview, int, int, int]:
    """Return owned pixels or a validated byte view and native image dimensions."""
    _validate_options(layout, value_range, color_order)
    if "torch" in sys.modules:
        from torch import Tensor

        if isinstance(image, Tensor):
            return _tensor_bytes(image, layout, value_range, color_order)
    if "numpy" in sys.modules:
        import numpy as np

        if isinstance(image, np.ndarray):
            return _numpy_bytes(image, layout, value_range, color_order)
    if layout != "auto" or value_range != "auto" or color_order != "RGB":
        msg = "layout/value_range/color_order apply only to NumPy arrays and tensors"
        raise ValueError(msg)
    if "PIL.Image" in sys.modules:
        from PIL.Image import Image

        if isinstance(image, Image):
            width, height = image.size
            _size(width, height)
            precision = image.mode in ("I", "F") or image.mode.startswith("I;16")
            if precision:
                # F stores native floats. Extrema can hide NaN; check every sample.
                samples = (
                    memoryview(image.tobytes()).cast("f")
                    if image.mode == "F"
                    # I;16L/I;16B have no getextrema(); "I" holds every 16-bit value.
                    else cast("tuple[int, int]", image.convert("I").getextrema())
                )
                if any(not isfinite(p) or not 0 <= p <= MAX_PIXEL for p in samples):
                    msg = "Pillow pixels must be finite in [0,255]; rescale explicitly"
                    raise ValueError(msg)
            target = "L" if image.mode == "L" or precision else "RGB"
            # convert() copies even to the same mode; tobytes() already copies.
            converted = image if image.mode == target else image.convert(target)
            channels = 1 if converted.mode == "L" else 3
            _size(width, height, channels)
            return converted.tobytes(), width, height, channels
    msg = "Expected a Pillow image, NumPy array, PyTorch tensor or PixelImage"
    raise TypeError(msg)


def _single_image_batch(shape: tuple[int, ...], layout: Layout) -> bool:
    """Whether a leading axis of size one is a batch to drop.

    A 4D input has one; so does (1, H, W) with layout="HW", a gray single-image batch.
    """
    if shape[:1] != (1,):
        return False
    return len(shape) == BATCH_DIMENSIONS or (
        layout == "HW" and len(shape) == COLOR_DIMENSIONS
    )


def _numpy_layout(image: NDArray[np.generic], layout: Layout) -> NDArray[np.generic]:
    if _single_image_batch(tuple(image.shape), layout):
        image = image[0]
    if image.ndim == HW_DIMENSIONS:
        if layout not in ("auto", "HW"):
            msg = "2D arrays require HW layout"
            raise ValueError(msg)
        image = image[:, :, None]
    elif image.ndim == COLOR_DIMENSIONS:
        if layout == "auto":
            first, last = image.shape[0] in CHANNELS, image.shape[2] in CHANNELS
            if first == last:
                msg = "Ambiguous array shape; specify layout='CHW' or 'HWC'"
                raise ValueError(msg)
            layout = "CHW" if first else "HWC"
        if layout == "CHW":
            image = image.transpose(1, 2, 0)
        elif layout != "HWC":
            msg = "3D arrays require CHW or HWC layout"
            raise ValueError(msg)
    else:
        msg = "Expected one HW, CHW or HWC image (optional batch dimension of size 1)"
        raise ValueError(msg)
    return image


def _numpy_bytes(
    image: NDArray[np.generic],
    layout: Layout,
    value_range: ValueRange,
    color_order: ColorOrder,
) -> tuple[memoryview, int, int, int]:
    import numpy as np

    image = _numpy_layout(image, layout)
    height, width, channels = map(int, image.shape)
    if channels not in CHANNELS:
        msg = "Array must have 1, 3 or 4 channels"
        raise ValueError(msg)
    _size(width, height, channels)
    pixels = _numpy_pixels(image, value_range)
    if color_order == "BGR" and channels != 1:
        # One strided copy per channel: about 4x faster than fancy indexing.
        reordered = np.empty(pixels.shape, dtype=np.uint8)
        for target, source in enumerate(BGR_CHANNELS[channels]):
            reordered[:, :, target] = pixels[:, :, source]
        pixels = reordered
    # The scanner snapshots this view once; avoid an intermediate bytes copy.
    return np.ascontiguousarray(pixels).data.cast("B"), width, height, channels


def _numpy_pixels(
    image: NDArray[np.generic], value_range: ValueRange, source: str = "Array"
) -> NDArray[np.uint8]:
    import numpy as np

    if image.dtype.kind not in "buif":
        msg = "NumPy input must contain real numeric image pixels"
        raise ValueError(msg)
    if image.dtype.kind == "b":
        pixels: NDArray[np.generic] = image.astype(np.uint8) * 255
        return cast("NDArray[np.uint8]", pixels)
    if image.dtype == np.uint8 and value_range != "0_1":
        return cast("NDArray[np.uint8]", image.astype(np.uint8, copy=False))
    unit = value_range == "0_1" or (value_range == "auto" and image.dtype.kind == "f")
    ceiling = 1 if unit else 255
    if image.dtype.kind == "f":
        return _round_bytes(image, ceiling, source)
    _check_range(float(image.min()), float(image.max()), ceiling, source)
    # Validated integers are exact bytes: 0..255, or 0/1 scaled to 0/255.
    return cast("NDArray[np.uint8]", image.astype(np.uint8) * np.uint8(255 // ceiling))


def _check_range(low: float, high: float, ceiling: int, source: str) -> None:
    """Reject extrema outside [0, ceiling]; source is "Array" or "Tensor"."""
    # Min and max propagate NaN and infinities, so they also check finiteness.
    if not (isfinite(low) and isfinite(high)):
        msg = f"{source} pixels must be finite"
        raise ValueError(msg)
    if low < 0 or high > ceiling:
        fix = "undo normalization" if source == "Tensor" else "rescale explicitly"
        msg = (
            f"{source} pixels exceed the selected range; auto uses [0,1] for floats. "
            f"Use value_range='0_255' for byte-unit floats or {fix}"
        )
        raise ValueError(msg)


@cache
def _converters() -> ThreadPoolExecutor:
    """Threads for large conversions; NumPy releases the GIL in the block work."""
    workers = min(MAX_CONVERT_THREADS, os.cpu_count() or 1)
    return ThreadPoolExecutor(workers, thread_name_prefix="tapirscan-convert")


def _round_bytes(
    image: NDArray[np.generic], ceiling: int, source: str
) -> NDArray[np.uint8]:
    """Validate and round float pixels to bytes in one pass of cache-sized blocks."""
    import numpy as np

    # float16 times 255 is exact in float32; wider floats need float64.
    exact = np.float32 if image.dtype.itemsize <= 2 else np.float64  # noqa: PLR2004
    pixels = np.empty(image.shape, dtype=np.uint8)
    rows = max(1, ROUND_BLOCK // max(1, image[0].size))

    def convert(start: int) -> ValueError | None:
        block = image[start : start + rows].astype(exact)
        # Widened blocks have fast min/max even for float16.
        try:
            _check_range(float(block.min()), float(block.max()), ceiling, source)
        except ValueError as error:
            return error
        if ceiling == 1:
            np.multiply(block, 255, out=block)
        pixels[start : start + rows] = np.rint(block, out=block)
        return None

    starts = range(0, image.shape[0], rows)
    errors: Iterator[ValueError | None]
    if image.size < PARALLEL_SAMPLES or (os.cpu_count() or 1) == 1:
        errors = (convert(start) for start in starts)
    else:
        # Blocks are independent; the first error in block order is raised, as in
        # the sequential loop.
        errors = iter(list(_converters().map(convert, starts)))
    error = next((error for error in errors if error is not None), None)
    if error is not None:
        raise error
    return pixels


def _tensor_layout(image: Tensor, layout: Layout) -> Tensor:
    if _single_image_batch(tuple(image.shape), layout):
        image = image[0]
    if image.ndim == HW_DIMENSIONS:
        if layout not in ("auto", "HW"):
            msg = "2D tensors require HW layout"
            raise ValueError(msg)
        return image.unsqueeze(-1)
    if image.ndim != COLOR_DIMENSIONS:
        msg = "Expected one HW, CHW or HWC image (optional batch dimension of size 1)"
        raise ValueError(msg)
    if layout == "auto":
        first, last = image.shape[0] in CHANNELS, image.shape[2] in CHANNELS
        if first == last:
            msg = "Ambiguous tensor shape; specify layout='CHW' or 'HWC'"
            raise ValueError(msg)
        layout = "CHW" if first else "HWC"
    if layout == "CHW":
        image = image.permute(1, 2, 0)
    elif layout != "HWC":
        msg = "3D tensors require CHW or HWC layout"
        raise ValueError(msg)
    if image.shape[2] not in CHANNELS:
        msg = "Tensor must have 1, 3 or 4 channels"
        raise ValueError(msg)
    return image


def _tensor_pixels(image: Tensor, value_range: ValueRange) -> Tensor:
    import torch

    # Out-of-place conversion of a detached tensor preserves the autograd graph.
    pixels = image.detach()
    # CUDA converts on the device and transfers only bytes. Other accelerators
    # (MPS lacks float64) convert on the CPU.
    if pixels.device.type not in ("cpu", "cuda"):
        pixels = pixels.to(device="cpu")
    pixels = pixels.resolve_neg()
    if pixels.dtype == torch.bool:
        return pixels.to(torch.uint8) * 255
    if pixels.dtype == torch.uint8 and value_range != "0_1":
        return pixels
    unit = value_range == "0_1" or (value_range == "auto" and image.is_floating_point())
    ceiling = 1 if unit else 255
    if pixels.device.type == "cpu" and pixels.is_floating_point():
        return _round_tensor(pixels, ceiling)
    # One device synchronization reads both extrema.
    low, high = torch.stack(torch.aminmax(pixels)).to(torch.float64).tolist()
    _check_range(low, high, ceiling, "Tensor")
    if not pixels.is_floating_point():
        # Validated integers are exact bytes: 0..255, or 0/1 scaled to 0/255.
        return pixels.to(torch.uint8) * (255 if unit else 1)
    # float16/bfloat16 times 255 is exact in float32; wider floats need float64
    # to round exactly like the NumPy adapter.
    half = pixels.dtype in (torch.float16, torch.bfloat16)
    exact = torch.float32 if half else torch.float64
    scaled = pixels.to(exact)
    if unit:
        scaled = scaled * 255
    return scaled.round_().to(torch.uint8)


def _round_tensor(pixels: Tensor, ceiling: int) -> Tensor:
    """Validate and round CPU float pixels to bytes, one reused block at a time."""
    import torch

    # float16/bfloat16 times 255 is exact in float32; wider floats need float64
    # to round exactly like the NumPy adapter.
    half = pixels.dtype in (torch.float16, torch.bfloat16)
    exact = torch.float32 if half else torch.float64
    out = torch.empty(pixels.shape, dtype=torch.uint8)
    rows = max(1, TENSOR_BLOCK // max(1, pixels[0].numel()))
    block = torch.empty((min(rows, pixels.shape[0]), *pixels.shape[1:]), dtype=exact)
    for start in range(0, pixels.shape[0], rows):
        source = pixels[start : start + rows]
        scaled = block[: source.shape[0]].copy_(source)
        low, high = torch.aminmax(scaled)
        _check_range(low.item(), high.item(), ceiling, "Tensor")
        if ceiling == 1:
            scaled.mul_(255)
        out[start : start + rows].copy_(scaled.round_())
    return out


def _tensor_bytes(
    image: Tensor, layout: Layout, value_range: ValueRange, color_order: ColorOrder
) -> tuple[memoryview, int, int, int]:
    import torch

    if (
        image.is_meta
        or image.layout != torch.strided
        or image.is_quantized
        or image.is_complex()
    ):
        msg = "Tensor must contain dense, real, non-quantized image pixels"
        raise ValueError(msg)
    image = _tensor_layout(image, layout)
    height, width, channels = map(int, image.shape)
    _size(width, height, channels)
    pixels = _tensor_pixels(image, value_range)
    if color_order == "BGR" and channels != 1:
        pixels = pixels[:, :, BGR_CHANNELS[channels]]
    pixels = pixels.contiguous()
    if pixels.device.type != "cpu":
        # Reordered on the device; transfer only the final bytes, into pinned memory
        # that PyTorch caches across calls (no fresh pages, faster DMA).
        host = torch.empty(pixels.shape, dtype=torch.uint8, pin_memory=True)
        pixels = host.copy_(pixels)
    if "numpy" in sys.modules:
        import numpy as np

        # Zero-copy view; the scanner snapshots it once.
        return np.asarray(pixels).data.cast("B"), width, height, channels
    # Own the bytes before native scanning, without requiring NumPy.
    raw = ctypes.string_at(pixels.data_ptr(), pixels.numel())
    return memoryview(raw), width, height, channels

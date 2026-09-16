"""Convert optional image objects without importing unused imaging packages."""

from __future__ import annotations

import ctypes
import sys
from math import isfinite
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    import numpy as np
    from numpy.typing import NDArray
    from torch import Tensor

    from .inputs import ImageInput
    from .results import ColorOrder, Layout, ValueRange

LIMIT = 128 * 1024 * 1024
MAX_IMAGE_PIXELS = 32 * 1024 * 1024
MAX_PIXEL = 255
MIN_SIZE = 3
HW_DIMENSIONS = 2
COLOR_DIMENSIONS = 3
BATCH_DIMENSIONS = 4
CHANNELS = (1, 3, 4)
BGR_CHANNELS = {3: [2, 1, 0], 4: [2, 1, 0, 3]}


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
    if width * height > MAX_IMAGE_PIXELS:
        msg = "Image exceeds 32 megapixel limit (33,554,432 pixels)"
        raise ValueError(msg)
    if width * height * channels > LIMIT:
        msg = "Image exceeds 128 MiB limit"
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
                    else cast("tuple[int, int]", image.getextrema())
                )
                if any(not isfinite(p) or not 0 <= p <= MAX_PIXEL for p in samples):
                    msg = "Pillow pixels must be finite in [0,255]; rescale explicitly"
                    raise ValueError(msg)
            converted = image.convert("L" if image.mode == "L" or precision else "RGB")
            channels = 1 if converted.mode == "L" else 3
            _size(width, height, channels)
            return converted.tobytes(), width, height, channels
    msg = "Expected a Pillow image, NumPy array, PyTorch tensor or PixelImage"
    raise TypeError(msg)


def _numpy_layout(image: NDArray[np.generic], layout: Layout) -> NDArray[np.generic]:
    if image.ndim == BATCH_DIMENSIONS and image.shape[0] == 1:
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
) -> tuple[bytes, int, int, int]:
    import numpy as np

    image = _numpy_layout(image, layout)
    height, width, channels = map(int, image.shape)
    if channels not in CHANNELS:
        msg = "Array must have 1, 3 or 4 channels"
        raise ValueError(msg)
    _size(width, height, channels)
    pixels = _numpy_pixels(image, value_range)
    if color_order == "BGR" and channels != 1:
        pixels = pixels[:, :, BGR_CHANNELS[channels]]
    return np.ascontiguousarray(pixels).tobytes(), width, height, channels


def _numpy_pixels(
    image: NDArray[np.generic], value_range: ValueRange
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
    if not np.isfinite(image).all():
        msg = "Array pixels must be finite"
        raise ValueError(msg)
    low, high = float(image.min()), float(image.max())
    unit = value_range == "0_1" or (value_range == "auto" and image.dtype.kind == "f")
    if low < 0 or high > (1 if unit else 255):
        msg = (
            "Array pixels exceed the selected range; auto uses [0,1] for floats. "
            "Use value_range='0_255' for byte-unit floats or rescale explicitly"
        )
        raise ValueError(msg)
    return cast(
        "NDArray[np.uint8]",
        np.rint(image.astype(np.float64) * (255 if unit else 1)).astype(np.uint8),
    )


def _tensor_layout(image: Tensor, layout: Layout) -> Tensor:
    if image.ndim == BATCH_DIMENSIONS and image.shape[0] == 1:
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

    # Blocking transfer and out-of-place conversion preserve the autograd graph.
    pixels = image.detach().to(device="cpu").resolve_neg()
    if pixels.dtype == torch.bool:
        return pixels.to(torch.uint8) * 255
    if pixels.dtype == torch.uint8 and value_range != "0_1":
        return pixels
    pixels = pixels.to(torch.float64)
    if not torch.isfinite(pixels).all().item():
        msg = "Tensor pixels must be finite"
        raise ValueError(msg)
    low, high = float(pixels.min().item()), float(pixels.max().item())
    unit = value_range == "0_1" or (value_range == "auto" and image.is_floating_point())
    ceiling = 1 if unit else 255
    if low < 0 or high > ceiling:
        msg = (
            "Tensor pixels exceed the selected range; auto uses [0,1] for floats. "
            "Use value_range='0_255' for byte-unit floats or undo normalization"
        )
        raise ValueError(msg)
    return (pixels * (255 if unit else 1)).round().to(torch.uint8)


def _tensor_bytes(
    image: Tensor, layout: Layout, value_range: ValueRange, color_order: ColorOrder
) -> tuple[bytes, int, int, int]:
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
    # Own the bytes before native scanning, without requiring NumPy.
    return ctypes.string_at(pixels.data_ptr(), pixels.numel()), width, height, channels

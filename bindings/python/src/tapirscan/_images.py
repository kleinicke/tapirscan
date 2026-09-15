"""Convert optional image objects without importing unused imaging packages."""

from __future__ import annotations

import ctypes
import sys
from typing import TYPE_CHECKING, cast

if TYPE_CHECKING:
    import numpy as np
    from numpy.typing import NDArray
    from torch import Tensor

    from .inputs import ImageInput, RawPixels
    from .results import Layout, ValueRange

LIMIT = 128 * 1024 * 1024
MIN_SIZE = 3
HW_DIMENSIONS = 2
COLOR_DIMENSIONS = 3
BATCH_DIMENSIONS = 4
CHANNELS = (1, 3, 4)


def _size(width: object, height: object, channels: int = 1) -> tuple[int, int]:
    if (
        isinstance(width, bool)
        or isinstance(height, bool)
        or not isinstance(width, int)
        or not isinstance(height, int)
        or width < MIN_SIZE
        or height < MIN_SIZE
        or width * height * channels > LIMIT
    ):
        msg = "Image dimensions must be >=3 and decoded pixels <=128 MiB"
        raise ValueError(msg)
    return width, height


def _validate_options(layout: Layout, value_range: ValueRange) -> None:
    if layout not in ("auto", "HW", "HWC", "CHW"):
        msg = "layout must be auto, HW, HWC or CHW"
        raise ValueError(msg)
    if value_range not in ("auto", "0_1", "0_255"):
        msg = "value_range must be auto, 0_1 or 0_255"
        raise ValueError(msg)


def image_bytes(
    image: ImageInput, *, layout: Layout = "auto", value_range: ValueRange = "auto"
) -> tuple[bytes | memoryview, int, int, int]:
    """Return owned pixels or a validated byte view and native image dimensions."""
    _validate_options(layout, value_range)
    if "torch" in sys.modules:
        from torch import Tensor

        if isinstance(image, Tensor):
            return _tensor_bytes(image, layout, value_range)
    if "numpy" in sys.modules:
        import numpy as np

        if isinstance(image, np.ndarray):
            return _numpy_bytes(image, layout, value_range)
    if layout != "auto" or value_range != "auto":
        msg = "layout/value_range overrides apply only to NumPy arrays and tensors"
        raise ValueError(msg)
    if "PIL.Image" in sys.modules:
        from PIL.Image import Image

        if isinstance(image, Image):
            width, height = image.size
            _size(width, height)
            converted = image.convert("L" if image.mode == "L" else "RGB")
            channels = 1 if converted.mode == "L" else 3
            _size(width, height, channels)
            return converted.tobytes(), width, height, channels
    if isinstance(image, tuple):
        return _raw_bytes(*image)
    msg = "Expected Pillow, NumPy, PyTorch or (pixels, width, height)"
    raise TypeError(msg)


def _numpy_bytes(
    image: NDArray[np.generic], layout: Layout, value_range: ValueRange
) -> tuple[bytes, int, int, int]:
    import numpy as np

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
    height, width, channels = map(int, image.shape)
    if channels not in CHANNELS:
        msg = "Array must have 1, 3 or 4 channels"
        raise ValueError(msg)
    _size(width, height, channels)
    pixels = _numpy_pixels(image, value_range)
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
    unit = value_range == "0_1" or (
        value_range == "auto" and image.dtype.kind == "f" and high <= 1
    )
    if low < 0 or high > (1 if unit else 255):
        msg = (
            "Array pixels must be in [0,1] or [0,255]; rescale higher-bit-depth "
            "or normalized images explicitly"
        )
        raise ValueError(msg)
    return cast(
        "NDArray[np.uint8]",
        np.rint(image.astype(np.float64) * (255 if unit else 1)).astype(np.uint8),
    )


def _raw_bytes(
    pixels: RawPixels, width: int, height: int
) -> tuple[memoryview, int, int, int]:
    width, height = _size(width, height)
    view = memoryview(pixels)
    if not view.c_contiguous or view.itemsize != 1 or view.nbytes != width * height:
        msg = "Raw tuple requires exactly width*height 8-bit grayscale bytes"
        raise ValueError(msg)
    return view.cast("B"), width, height, 1


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
    unit = value_range == "0_1" or (
        value_range == "auto" and image.is_floating_point() and high <= 1
    )
    ceiling = 1 if unit else 255
    if low < 0 or high > ceiling:
        msg = "Tensor pixels must be in [0,1] or [0,255]; undo normalization first"
        raise ValueError(msg)
    return (pixels * (255 if unit else 1)).round().to(torch.uint8)


def _tensor_bytes(
    image: Tensor, layout: Layout, value_range: ValueRange
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
    pixels = _tensor_pixels(image, value_range).contiguous()
    # Own the bytes before native scanning, without requiring NumPy.
    return ctypes.string_at(pixels.data_ptr(), pixels.numel()), width, height, channels

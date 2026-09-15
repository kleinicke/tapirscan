"""Input types; optional imaging packages are never imported at runtime."""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, TypeAlias

if TYPE_CHECKING:
    import numpy as np
    from PIL.Image import Image
    from torch import Tensor

RawPixels: TypeAlias = bytes | bytearray | memoryview
ImageInput: TypeAlias = (
    "RawPixels | tuple[RawPixels, int, int] | np.ndarray[Any, Any] | Image | Tensor"
)

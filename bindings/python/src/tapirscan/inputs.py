"""Input types; optional imaging packages are never imported at runtime."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Any, TypeAlias

if TYPE_CHECKING:
    import numpy as np
    from PIL.Image import Image
    from torch import Tensor

RawPixels: TypeAlias = bytes | bytearray | memoryview


@dataclass(frozen=True)
class PixelImage:
    """Decoded byte pixels. Dimensions and storage options are keyword-only.

    channels is 1 (gray), 3 (RGB) or 4 (RGBA). Alpha is ignored.
    stride defaults to width * channels; larger values allow row padding.
    The caller owns data; scanning snapshots its addressed bytes.
    """

    data: RawPixels
    width: int = field(kw_only=True)
    height: int = field(kw_only=True)
    channels: int = field(default=1, kw_only=True)
    stride: int | None = field(default=None, kw_only=True)


ImageInput: TypeAlias = "PixelImage | np.ndarray[Any, Any] | Image | Tensor"

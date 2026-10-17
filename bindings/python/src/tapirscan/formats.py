"""Format names, presets and validation of format selections."""

from typing import Literal, TypeAlias, cast

from ._format_registry import (
    FORMAT_BITS,
    Format,
    common_formats,
    common_linear_formats,
    linear_formats,
    matrix_formats,
    retail_formats,
)

__all__ = [
    "FORMAT_BITS",
    "Format",
    "FormatSelection",
    "common_formats",
    "common_linear_formats",
    "format_mask",
    "linear_formats",
    "matrix_formats",
    "resolve_formats",
    "retail_formats",
]

FormatSelection: TypeAlias = (
    Format
    | tuple[Format, ...]
    | list[Format]
    | Literal["retail", "common1D", "common", "1D", "2D", "all"]
)


def resolve_formats(formats: FormatSelection | None) -> tuple[Format, ...]:
    """Expand public presets and validate a nonempty explicit format selection."""
    if formats is None:
        return retail_formats
    if isinstance(formats, str):
        if formats in FORMAT_BITS:
            return (cast("Format", formats),)
        presets = {
            "retail": retail_formats,
            "common1D": common_linear_formats,
            "common": common_formats,
            "1D": linear_formats,
            "2D": matrix_formats,
            "all": (*linear_formats, *matrix_formats),
        }
        if formats not in presets:
            msg = f"Unknown format preset: {formats}"
            raise ValueError(msg)
        return presets[formats]
    if not isinstance(formats, (tuple, list)) or not formats:
        msg = "Choose at least one barcode format"
        raise ValueError(msg)
    for name in formats:
        if not isinstance(name, str) or name not in FORMAT_BITS:
            msg = f"Unsupported barcode format: {name}"
            raise ValueError(msg)
    return tuple(dict.fromkeys(formats))


def format_mask(formats: FormatSelection | None) -> int:
    """Validate formats before native code is called."""
    mask = 0
    for name in resolve_formats(formats):
        mask |= FORMAT_BITS[name]
    return mask

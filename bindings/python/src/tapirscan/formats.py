"""Explicit opt-in barcode formats, matching the native ABI."""

from typing import Literal, TypeAlias

Format: TypeAlias = Literal[
    "EAN13",
    "UPCA",
    "EAN8",
    "UPCE",
    "Code128",
    "Code39",
    "ITF",
    "Codabar",
    "Code93",
    "QRCode",
    "DataMatrix",
    "PDF417",
    "Aztec",
    "DataBar",
    "DataBarExpanded",
    "MaxiCode",
]
FORMAT_BITS: dict[str, int] = {
    "EAN13": 1,
    "UPCA": 2,
    "EAN8": 4,
    "UPCE": 8,
    "Code128": 16,
    "Code39": 32,
    "ITF": 64,
    "Codabar": 128,
    "Code93": 256,
    "QRCode": 512,
    "DataMatrix": 1024,
    "PDF417": 2048,
    "Aztec": 4096,
    "DataBar": 8192,
    "DataBarExpanded": 16384,
    "MaxiCode": 131072,
}


FormatSelection: TypeAlias = (
    tuple[Format, ...] | list[Format] | Literal["1D", "2D", "all"]
)
retail_formats: tuple[Format, ...] = ("EAN13", "UPCA", "EAN8", "UPCE")
linear_formats: tuple[Format, ...] = (
    *retail_formats,
    "Code128",
    "Code39",
    "ITF",
    "Codabar",
    "Code93",
    "DataBar",
    "DataBarExpanded",
)
matrix_formats: tuple[Format, ...] = (
    "QRCode",
    "DataMatrix",
    "PDF417",
    "Aztec",
    "MaxiCode",
)


def resolve_formats(formats: FormatSelection | None) -> tuple[Format, ...]:
    """Expand public presets and validate a nonempty explicit format selection."""
    if formats is None:
        return ("EAN13",)
    if isinstance(formats, str):
        presets = {
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

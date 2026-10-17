"""Structured content of decoded text: GS1, URLs, Wi-Fi, contacts, payments and IDs."""

from __future__ import annotations

import ctypes as c
from dataclasses import dataclass
from functools import lru_cache
from typing import TYPE_CHECKING, Literal, NamedTuple, TypeAlias, cast

from ._library import ScannerError, library_path, load

if TYPE_CHECKING:
    import os
    from collections.abc import Iterator
    from pathlib import Path

    from .results import Barcode

ContentKind: TypeAlias = Literal[
    "GS1",
    "URL",
    "Email",
    "Phone",
    "SMS",
    "Geo",
    "WiFi",
    "Contact",
    "Calendar",
    # Experimental kinds: they may change in minor releases.
    "SwissQRBill",
    "EPCPayment",
    "HIBC",
    "VIN",
    "AAMVA",
]


class ContentField(NamedTuple):
    """One named value; names repeat where the payload repeats them."""

    name: str
    value: str


@dataclass(frozen=True)
class Content:
    """Structured content of decoded text, with fields in payload order."""

    kind: ContentKind
    fields: tuple[ContentField, ...]

    def get(self, name: str) -> str | None:
        """Return the first value of a field, or None."""
        return next((f.value for f in self.fields if f.name == name), None)

    def get_all(self, name: str) -> list[str]:
        """Return every value of a field in payload order."""
        return [f.value for f in self.fields if f.name == name]

    def __iter__(self) -> Iterator[ContentField]:
        """Iterate over the fields."""
        return iter(self.fields)


class _Summary(c.Structure):
    _fields_ = [("kind", c.c_uint32), ("field_count", c.c_uint64)]


class _Lengths(c.Structure):
    _fields_ = [("name_length", c.c_uint64), ("value_length", c.c_uint64)]


@lru_cache(maxsize=4)
def _library(path: Path) -> c.CDLL:
    lib = load(path)
    u64 = c.c_uint64
    ptr = c.POINTER
    signatures = {
        "tapirscan_parse_content": (
            [c.c_char_p, u64, c.c_int32, ptr(u64), c.c_void_p],
            c.c_int32,
        ),
        "tapirscan_content_info": ([u64, ptr(_Summary)], c.c_int32),
        "tapirscan_content_field": ([u64, u64, ptr(_Lengths)], c.c_int32),
        "tapirscan_content_copy": (
            [u64, u64, c.c_uint32, c.c_void_p, u64],
            c.c_int32,
        ),
        "tapirscan_content_destroy": ([u64], c.c_int32),
        "tapirscan_content_kind_name": ([c.c_uint32], c.c_char_p),
    }
    for name, (args, result) in signatures.items():
        fn = getattr(lib, name)
        fn.argtypes = args
        fn.restype = result
    return lib


def _check(code: int, detail: str = "") -> None:
    if code:
        raise ScannerError(code, detail or "content parsing failed")


def parse_content(
    source: str | Barcode,
    *,
    gs1: bool | None = None,
    library_dir: str | os.PathLike[str] | None = None,
) -> Content | None:
    """Recognize the structure of decoded text, or a barcode's text and GS1 flag.

    `gs1=True` parses the text as a GS1 element string with GS (0x1D)
    separators; for a barcode it defaults to the barcode's GS1 flag. Returns
    None for text without recognized structure.
    """
    if isinstance(source, str):
        text = source
    else:
        text = source.text
        if gs1 is None:
            gs1 = source.gs1
    lib = _library(library_path(library_dir))
    data = text.encode()
    handle = c.c_uint64(0)
    error = c.create_string_buffer(512)
    _check(
        lib.tapirscan_parse_content(
            data, len(data), 1 if gs1 else 0, c.byref(handle), error
        ),
        error.value.decode(),
    )
    if not handle.value:
        return None
    try:
        summary = _Summary()
        _check(lib.tapirscan_content_info(handle, c.byref(summary)))
        fields = []
        for index in range(summary.field_count):
            lengths = _Lengths()
            _check(lib.tapirscan_content_field(handle, index, c.byref(lengths)))
            parts = []
            for part, length in enumerate((lengths.name_length, lengths.value_length)):
                buffer = c.create_string_buffer(length + 1)
                _check(
                    lib.tapirscan_content_copy(handle, index, part, buffer, length + 1)
                )
                parts.append(buffer.raw[:length].decode())
            fields.append(ContentField(*parts))
        kind = cast(
            "ContentKind", lib.tapirscan_content_kind_name(summary.kind).decode()
        )
        return Content(kind, tuple(fields))
    finally:
        lib.tapirscan_content_destroy(handle)

"""Structured content of decoded text: GS1, URLs, Wi-Fi, contacts, payments and IDs."""

from __future__ import annotations

import ctypes as c
from dataclasses import dataclass
from functools import lru_cache
from typing import TYPE_CHECKING, NamedTuple

from ._content_registry import CONTENT_KINDS, ContentKind
from ._library import Signatures, bind, check, library_path, load

if TYPE_CHECKING:
    import os
    from collections.abc import Iterator
    from pathlib import Path

    from .results import Barcode


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


class _ContentSummary(c.Structure):
    _fields_ = [("kind", c.c_uint32), ("field_count", c.c_uint64)]


class _ContentLengths(c.Structure):
    _fields_ = [("name_length", c.c_uint64), ("value_length", c.c_uint64)]


# Shared with Scanner, which binds them on its own library handle.
CONTENT_SIGNATURES: Signatures = {
    "tapirscan_parse_content": (
        [c.c_char_p, c.c_uint64, c.c_int32, c.POINTER(c.c_uint64), c.c_void_p],
        c.c_int32,
    ),
    "tapirscan_content_info": (
        [c.c_uint64, c.POINTER(_ContentSummary)],
        c.c_int32,
    ),
    "tapirscan_content_field": (
        [c.c_uint64, c.c_uint64, c.POINTER(_ContentLengths)],
        c.c_int32,
    ),
    "tapirscan_content_copy": (
        [c.c_uint64, c.c_uint64, c.c_uint32, c.c_void_p, c.c_uint64],
        c.c_int32,
    ),
    "tapirscan_content_destroy": ([c.c_uint64], c.c_int32),
}


@lru_cache(maxsize=4)
def _library(path: Path) -> c.CDLL:
    lib = load(path)
    bind(lib, CONTENT_SIGNATURES)
    return lib


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
    return _parse(_library(library_path(library_dir)), text, gs1=bool(gs1))


def _parse(lib: c.CDLL, text: str, *, gs1: bool) -> Content | None:
    """Parse text with a library that has `CONTENT_SIGNATURES` bound."""
    data = text.encode()
    handle = c.c_uint64(0)
    error = c.create_string_buffer(512)
    check(
        lib,
        lib.tapirscan_parse_content(
            data, len(data), 1 if gs1 else 0, c.byref(handle), error
        ),
        error.value.decode(),
    )
    if not handle.value:
        return None
    try:
        summary = _ContentSummary()
        check(lib, lib.tapirscan_content_info(handle, c.byref(summary)))
        fields = []
        for index in range(summary.field_count):
            lengths = _ContentLengths()
            check(lib, lib.tapirscan_content_field(handle, index, c.byref(lengths)))
            parts = []
            for part, length in enumerate((lengths.name_length, lengths.value_length)):
                buffer = c.create_string_buffer(length + 1)
                check(
                    lib,
                    lib.tapirscan_content_copy(handle, index, part, buffer, length + 1),
                )
                parts.append(buffer.raw[:length].decode())
            fields.append(ContentField(*parts))
        return Content(CONTENT_KINDS[summary.kind], tuple(fields))
    finally:
        lib.tapirscan_content_destroy(handle)

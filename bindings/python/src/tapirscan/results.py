"""Typed, immutable results shared by module and reusable-scanner calls."""

from __future__ import annotations

import json
from collections.abc import Iterator, Sequence
from dataclasses import dataclass, field
from math import ceil, floor
from typing import Any, Literal, NamedTuple, TypeAlias, cast, overload

from typing_extensions import override

SCHEMA_VERSION = 2

Mode: TypeAlias = Literal["low", "medium", "high", "very-high"]
Layout: TypeAlias = Literal["auto", "HW", "HWC", "CHW"]
ValueRange: TypeAlias = Literal["auto", "0_1", "0_255"]
JSONValue: TypeAlias = (
    "bool | int | float | str | list[JSONValue] | dict[str, JSONValue] | None"
)


class Point(NamedTuple):
    """A source-image coordinate in pixels."""

    x: float
    y: float


class Rect(NamedTuple):
    """An integer rectangle enclosing a barcode polygon."""

    left: int
    top: int
    width: int
    height: int


@dataclass(frozen=True)
class Barcode:
    """An immutable decoded barcode value and its source geometry."""

    text: str
    polygon: tuple[Point, ...]
    format: str
    _support: int = field(repr=False, compare=False)

    @property
    def data(self) -> bytes:
        """Return the decoded text as UTF-8 bytes."""
        return self.text.encode("utf-8")

    @property
    def type(self) -> str:
        """Return the supported barcode symbology."""
        return self.format

    @property
    def rect(self) -> Rect:
        """Return the enclosing integer rectangle."""
        left = floor(min(p.x for p in self.polygon))
        top = floor(min(p.y for p in self.polygon))
        return Rect(
            left,
            top,
            ceil(max(p.x for p in self.polygon)) - left,
            ceil(max(p.y for p in self.polygon)) - top,
        )

    @property
    def quality(self) -> int:
        """Migration alias for support; not comparable to ZBar's quality score."""
        return self._support

    @property
    def orientation(self) -> None:
        """Return None because reading orientation is not provided."""
        return None


@dataclass(frozen=True)
class Proposal:
    """A localized region and its heuristic localization score."""

    polygon: tuple[Point, ...]
    score: float


@dataclass(frozen=True)
class SearchWindow:
    """An attempted search area, which need not contain a barcode."""

    polygon: tuple[Point, ...]
    candidate_index: int
    kind: str


@dataclass(frozen=True)
class Candidate:
    """Decode outcomes and status for one attempted region."""

    candidate_index: int
    polygon: tuple[Point, ...]
    detections: tuple[Barcode, ...]
    unfinished: bool
    error: bool
    error_detail: str | None
    elapsed_ms: float


@dataclass(frozen=True)
class Regions:
    """Requested localization proposals and attempted-region evidence."""

    proposals: tuple[Proposal, ...]
    search_windows: tuple[SearchWindow, ...]
    candidates: tuple[Candidate, ...]
    omitted: int
    work_limited: bool
    additional: tuple[Barcode, ...] = ()


class ImageSize(NamedTuple):
    """Dimensions of the supplied scan image, in pixels."""

    width: int
    height: int


@dataclass(frozen=True)
class BarcodeEvidence:
    """Diagnostic evidence for one decoded barcode, in result order."""

    support: int
    axis: int
    candidate_indices: tuple[int, ...]


@dataclass(frozen=True)
class Diagnostics:
    """Explicitly requested search evidence and native diagnostic JSON."""

    regions: Regions | None
    barcodes: tuple[BarcodeEvidence, ...]
    localization_limited: bool
    _json: bytes = field(repr=False, compare=False)

    def to_dict(self) -> dict[str, JSONValue]:
        """Return an independent copy of the native diagnostic payload."""
        return cast("dict[str, JSONValue]", json.loads(self._json))


@dataclass(frozen=True)
class ScanResult(Sequence[Barcode]):
    """Iterate/index barcodes directly; access evidence through attributes.

    Empty results are false even when undecoded regions exist. ``debug is None``
    means diagnostic evidence was not requested.
    """

    barcodes: tuple[Barcode, ...]
    mode: Mode
    multiple: bool
    elapsed_ms: float
    unfinished: bool
    image: ImageSize
    debug: Diagnostics | None
    _json: bytes = field(repr=False, compare=False)

    @override
    def __len__(self) -> int:
        """Return the number of decoded barcodes."""
        return len(self.barcodes)

    @override
    def __iter__(self) -> Iterator[Barcode]:
        """Iterate the decoded barcodes in scanner order."""
        return iter(self.barcodes)

    @overload
    def __getitem__(self, index: int) -> Barcode: ...

    @overload
    def __getitem__(self, index: slice) -> tuple[Barcode, ...]: ...

    @override
    def __getitem__(self, index: int | slice) -> Barcode | tuple[Barcode, ...]:
        """Select a barcode or a slice of barcodes."""
        return self.barcodes[index]

    @property
    def values(self) -> list[str]:
        """Return an independent list of decoded strings."""
        return [barcode.text for barcode in self]

    @property
    def best(self) -> Barcode | None:
        """Return the highest-support barcode, or None for an empty result."""
        return max(self.barcodes, key=lambda b: b.quality) if self.barcodes else None

    def to_dict(self) -> dict[str, JSONValue]:
        """Independent schema-2 JSON data, including all requested diagnostics."""
        return cast("dict[str, JSONValue]", json.loads(self._json))


def _polygon(points: list[list[float]]) -> tuple[Point, ...]:
    return tuple(Point(x, y) for x, y in points)


def _barcode(value: dict[str, Any]) -> Barcode:
    return Barcode(
        value["text"],
        _polygon(value["polygon"]),
        value.get("format", "EAN13"),
        value["support"],
    )


def _from_json(raw: bytes, width: int, height: int, *, debug: bool) -> ScanResult:
    # Dynamic values are confined to this trusted, versioned native ABI boundary.
    value: dict[str, Any] = json.loads(raw)
    if value["schemaVersion"] != SCHEMA_VERSION or value["mode"] not in (
        "low",
        "medium",
        "high",
        "very-high",
    ):
        msg = "Unsupported native result schema or mode"
        raise RuntimeError(msg)
    frame = value["scan"]
    regions = None
    if "localization" in value or "regions" in frame:
        loc = value.get(
            "localization", {"proposals": [], "omitted": 0, "workLimited": False}
        )
        regions = Regions(
            tuple(
                Proposal(_polygon(p["polygon"]), p["score"]) for p in loc["proposals"]
            ),
            tuple(
                SearchWindow(_polygon(w["polygon"]), w["candidateIndex"], w["kind"])
                for w in value.get("searchWindows", [])
            ),
            tuple(
                Candidate(
                    c["candidate_index"],
                    _polygon(c["coverage"]),
                    tuple(_barcode(d) for d in c["detections"]),
                    c["unfinished"],
                    c["error"],
                    c["error_detail"],
                    c["ms"],
                )
                for c in frame.get("candidates", [])
            ),
            loc["omitted"],
            loc["workLimited"],
            tuple(_barcode(b) for b in frame.get("regions", [])),
        )
    return ScanResult(
        tuple(_barcode(b) for b in frame["barcodes"]),
        value["mode"],
        value["multiple"],
        value["elapsedMs"],
        frame["unfinished"],
        ImageSize(width, height),
        Diagnostics(
            regions,
            tuple(
                BarcodeEvidence(
                    b["support"],
                    b.get("axis", 0),
                    tuple(b.get("candidate_indices", ())),
                )
                for b in frame["barcodes"]
            ),
            value["localizationLimited"],
            raw,
        )
        if debug
        else None,
        raw,
    )

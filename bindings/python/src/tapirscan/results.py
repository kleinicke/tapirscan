"""Typed, immutable results shared by module and reusable-scanner calls."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from math import ceil, floor
from typing import TYPE_CHECKING, Any, Literal, NamedTuple, TypeAlias, cast

from typing_extensions import override

if TYPE_CHECKING:
    from .formats import Format

SCHEMA_VERSION = 2

EanAddOnPolicy: TypeAlias = Literal["ignore", "read", "require"]

ColorOrder: TypeAlias = Literal["RGB", "BGR"]

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
class StructuredAppend:
    """One symbol in a multipart payload; index is one-based, no auto assembly."""

    index: int
    count: int
    id: str | None = None
    parity: int | None = None


@dataclass(frozen=True)
class Barcode:
    """An immutable decoded barcode value and its source geometry."""

    text: str
    polygon: tuple[Point, ...]
    format: Format
    support: int = 0
    gs1: bool | None = None
    reader_initialization: bool | None = None
    structured_append: StructuredAppend | None = None
    ean_add_on: str | None = None
    payload_bytes: bytes | None = None

    def as_dict(self) -> dict[str, JSONValue]:
        """Return JSON-compatible public fields, with payload bytes as integers."""
        return {
            "text": self.text,
            "format": self.format,
            "polygon": [[p.x, p.y] for p in self.polygon],
            "rect": dict(self.rect._asdict()),
            "support": self.support,
            "gs1": self.gs1,
            "reader_initialization": self.reader_initialization,
            "structured_append": (
                {
                    "index": self.structured_append.index,
                    "count": self.structured_append.count,
                    "id": self.structured_append.id,
                    "parity": self.structured_append.parity,
                }
                if self.structured_append
                else None
            ),
            "ean_add_on": self.ean_add_on,
            "payload_bytes": list(self.payload_bytes)
            if self.payload_bytes is not None
            else None,
        }

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
class UndecodedRegion:
    """Source geometry with no accepted decode; format is a reader hint."""

    polygon: tuple[Point, ...]
    format: str


@dataclass(frozen=True)
class Regions:
    """Requested localization proposals and attempted-region evidence."""

    proposals: tuple[Proposal, ...] | None
    search_windows: tuple[SearchWindow, ...] | None
    candidates: tuple[Candidate, ...]
    omitted: int
    work_limited: bool
    undecoded: tuple[UndecodedRegion, ...] = ()


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
    _json: bytes = field(repr=False, compare=False)

    def to_raw_dict(self) -> dict[str, JSONValue]:
        """Return an independent copy of the native diagnostic payload."""
        return cast("dict[str, JSONValue]", json.loads(self._json))


@dataclass(frozen=True)
class ScanResult:
    """Decoded values and their locations; no diagnostic collection."""

    barcodes: tuple[Barcode, ...]

    @property
    def best(self) -> Barcode | None:
        """Highest-support read, first on ties, or None; support is not confidence."""
        winner: Barcode | None = None
        for barcode in self.barcodes:
            if winner is None or barcode.support > winner.support:
                winner = barcode
        return winner

    @property
    def values(self) -> list[str]:
        """Return an independent list of decoded strings."""
        return [barcode.text for barcode in self.barcodes]

    def as_dict(self) -> dict[str, JSONValue]:
        """Return an independent JSON-compatible result."""
        selected = self.best
        return {
            "barcodes": [barcode.as_dict() for barcode in self.barcodes],
            "values": list(self.values),
            "best": selected.as_dict() if selected is not None else None,
        }


@dataclass(frozen=True)
class InspectionResult(ScanResult):
    """Inspection report with decoded barcodes and optional engine evidence."""

    mode: Mode
    elapsed_ms: float
    undecoded: tuple[UndecodedRegion, ...]
    image: ImageSize
    diagnostics: Diagnostics

    @override
    def as_dict(self) -> dict[str, JSONValue]:
        """Return independent JSON-compatible public results, excluding diagnostics.

        Use diagnostics.to_raw_dict() separately when engine evidence is needed.
        """
        return {
            "barcodes": [barcode.as_dict() for barcode in self.barcodes],
            "values": list(self.values),
            "best": self.best.as_dict() if self.best is not None else None,
            "image": dict(self.image._asdict()),
            "mode": self.mode,
            "elapsed_ms": self.elapsed_ms,
            "undecoded": [
                {"format": r.format, "polygon": [[p.x, p.y] for p in r.polygon]}
                for r in self.undecoded
            ],
        }


def _polygon(points: list[list[float]]) -> tuple[Point, ...]:
    return tuple(Point(x, y) for x, y in points)


def _barcode(value: dict[str, Any]) -> Barcode:
    append = value.get("structuredAppend")
    return Barcode(
        value["text"],
        _polygon(value["polygon"]),
        value.get("format", "EAN13"),
        value.get("support", 0),
        value.get("gs1"),
        value.get("readerInitialization"),
        StructuredAppend(**append) if append is not None else None,
        value.get("eanAddOn"),
        bytes(value["bytes"]) if "bytes" in value else None,
    )


def _diagnostics(raw: bytes, undecoded: tuple[UndecodedRegion, ...]) -> Diagnostics:
    """Build diagnostic evidence from the engine report.

    Public result fields come from typed native accessors; this report only feeds
    `diagnostics`, whose fields may change between releases.
    """
    value: dict[str, Any] = json.loads(raw)
    if value["schemaVersion"] != SCHEMA_VERSION:
        msg = "Unsupported native diagnostic schema"
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
            )
            if "localization" in value
            else None,
            tuple(
                SearchWindow(_polygon(w["polygon"]), w["candidateIndex"], w["kind"])
                for w in value.get("searchWindows", [])
            )
            if "searchWindows" in value
            else None,
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
            undecoded,
        )
    return Diagnostics(
        regions,
        tuple(
            BarcodeEvidence(
                b["support"],
                b.get("axis", 0),
                tuple(b.get("candidate_indices", ())),
            )
            for b in frame["barcodes"]
        ),
        raw,
    )

"""Preview acceptance workflows, independent of camera and native scanner lifetime."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal

from .formats import FORMAT_BITS, Format

_MAX_QUANTITY = 9_007_199_254_740_991


@dataclass(frozen=True)
class ScanEntry:
    """Accepted units of a format/text pair, not a physical-object count."""

    format: Format
    text: str
    quantity: int


class ScanSession:
    """Preview single/list workflow. Confine each session to one thread.

    Repeated identities are ignored until reset unless repeat=True is explicit.
    The caller owns camera control and persistence. Finished sessions are locked.
    """

    def __init__(self, *, mode: Literal["single", "list"] = "single") -> None:
        """Create an empty single or list workflow."""
        if mode not in ("single", "list"):
            message = "session mode must be 'single' or 'list'"
            raise ValueError(message)
        self._mode = mode
        self._finished = False
        self._entries: dict[tuple[Format, str], ScanEntry] = {}
        self._history: list[tuple[Format, str]] = []

    @property
    def mode(self) -> Literal["single", "list"]:
        """Workflow selected at construction."""
        return self._mode

    @property
    def entries(self) -> tuple[ScanEntry, ...]:
        """Immutable snapshot in first-acceptance order, independent of later edits."""
        return tuple(self._entries.values())

    @property
    def finished(self) -> bool:
        """True after finish(), or the first acceptance in single mode."""
        return self._finished

    @property
    def can_undo(self) -> bool:
        """Whether the latest accepted unit can be undone."""
        return not self._finished and bool(self._history)

    def accept(
        self, barcode_format: Format, text: str, *, repeat: bool = False
    ) -> bool:
        """Accept one unit; repeat=True must represent an intentional user action.

        Pass barcode.format and barcode.text from a scan result. False means
        a duplicate or finished session. Empty decoded text is preserved.
        """
        if self._finished:
            return False
        if not isinstance(barcode_format, str) or barcode_format not in FORMAT_BITS:
            message = "unsupported barcode format"
            raise ValueError(message)
        if not isinstance(text, str) or not isinstance(repeat, bool):
            message = "text must be a string and repeat must be a boolean"
            raise TypeError(message)
        key = (barcode_format, text)
        previous = self._entries.get(key)
        if previous is not None and not repeat:
            return False
        quantity = (previous.quantity if previous else 0) + 1
        if quantity > _MAX_QUANTITY:
            message = "quantity exceeds the cross-language safe integer range"
            raise OverflowError(message)
        self._entries[key] = ScanEntry(barcode_format, text, quantity)
        self._history.append(key)
        if self._mode == "single":
            self._finished = True
        return True

    def undo(self) -> bool:
        """Reverse the last accepted unit; false when empty or finished."""
        if not self.can_undo:
            return False
        key = self._history.pop()
        entry = self._entries[key]
        if entry.quantity == 1:
            del self._entries[key]
        else:
            self._entries[key] = ScanEntry(entry.format, entry.text, entry.quantity - 1)
        return True

    def finish(self) -> tuple[ScanEntry, ...]:
        """Lock acceptance and return a snapshot. Does not stop a camera."""
        self._finished = True
        return self.entries

    def reset(self) -> None:
        """Begin an empty session in the same mode; old snapshots stay unchanged."""
        self._entries.clear()
        self._history.clear()
        self._finished = False

"""Provide migration aliases returning the unified ScanResult."""

from . import scan as decode
from .results import Barcode as Decoded
from .results import Point, Rect

__all__ = ["Decoded", "Point", "Rect", "decode"]

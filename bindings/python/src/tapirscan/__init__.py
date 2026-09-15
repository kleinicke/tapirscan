"""Barcode scanning with bundled native libraries and optional image adapters."""

from __future__ import annotations

import ctypes as c
import os
import sys
from contextlib import suppress
from pathlib import Path
from threading import RLock
from typing import cast

from typing_extensions import Self

from ._images import image_bytes
from .formats import (
    Format,
    FormatSelection,
    format_mask,
    linear_formats,
    matrix_formats,
    resolve_formats,
    retail_formats,
)
from .inputs import ImageInput
from .results import (
    Barcode,
    Diagnostics,
    ImageSize,
    Layout,
    Mode,
    Point,
    Rect,
    ScanResult,
    ValueRange,
    _from_json,
)

__all__ = [
    "Barcode",
    "Diagnostics",
    "Format",
    "FormatSelection",
    "ImageInput",
    "ImageSize",
    "Layout",
    "Mode",
    "Point",
    "Rect",
    "ScanResult",
    "Scanner",
    "ScannerError",
    "ValueRange",
    "linear_formats",
    "matrix_formats",
    "retail_formats",
    "scan",
]
ABI_VERSION = 3
MAX_IMAGE_BYTES = 128 * 1024 * 1024


def _debug_option(*, multiple: bool, debug: bool, include_regions: bool | None) -> bool:
    if type(multiple) is not bool or type(debug) is not bool:
        msg = "multiple and debug must be booleans"
        raise ValueError(msg)
    if include_regions is not None:
        if type(include_regions) is not bool or (debug and not include_regions):
            msg = "include_regions must be boolean and agree with debug"
            raise ValueError(msg)
        return include_regions
    return debug


class ScannerError(RuntimeError):
    """A native scanner failure with its numeric ABI status code."""

    def __init__(self, code: int) -> None:
        """Initialize the scanner state or native error code."""
        self.code = code
        super().__init__(f"Barcode scanner error {code}")


def _check(code: int) -> None:
    if code:
        raise ScannerError(code)


class _Info(c.Structure):
    _fields_ = [
        ("json_length", c.c_uint64),
        ("barcode_count", c.c_uint64),
        ("unfinished", c.c_uint32),
        ("localization_limited", c.c_uint32),
    ]


def _integer(
    value: object, name: str, minimum: int = 0, maximum: int = MAX_IMAGE_BYTES
) -> int:
    if (
        isinstance(value, bool)
        or not isinstance(value, int)
        or not minimum <= value <= maximum
    ):
        msg = f"Invalid {name}"
        raise ValueError(msg)
    return value


class Scanner:
    """A reusable scanner. Use a context manager or call close().

    mode selects a separate compiled library. library_dir may also be supplied
    through TAPIRSCAN_LIBRARY_DIR; otherwise bundled libraries are used.
    Installed packages never search the cwd.
    Results are immutable typed objects and remain valid after close/next scan.
    """

    def __init__(
        self,
        mode: Mode = "medium",
        *,
        formats: FormatSelection | None = None,
        library_dir: str | os.PathLike[str] | None = None,
    ) -> None:
        """Initialize the scanner state or native error code."""
        if mode not in ("low", "medium", "high", "very-high"):
            msg = "mode must be low, medium, high or very-high"
            raise ValueError(msg)
        self.formats = resolve_formats(formats)
        directory = (
            library_dir
            if library_dir is not None
            else os.environ.get("TAPIRSCAN_LIBRARY_DIR")
        )
        if directory is None:
            directory = Path(__file__).resolve().parent / "_native"
            if not directory.is_dir():
                msg = (
                    "This installation has no bundled native libraries. "
                    "Install a platform wheel, or set library_dir / "
                    "TAPIRSCAN_LIBRARY_DIR for a source checkout."
                )
                raise ValueError(msg)
        prefix, suffix = (
            ("", ".dll")
            if sys.platform == "win32"
            else ("lib", ".dylib" if sys.platform == "darwin" else ".so")
        )
        path = (
            Path(directory).expanduser().resolve()
            / f"{prefix}tapirscan_{mode.replace('-', '_')}{suffix}"
        )
        self._lock = RLock()
        self._handle = c.c_uint64(0)
        self._lib = c.CDLL(str(path))
        self.mode = mode
        u64 = c.c_uint64
        u32 = c.c_uint32
        ptr = c.POINTER
        signatures = {
            "barcode_abi_version": ([], u32),
            "barcode_mode": ([], u32),
            "tapirscan_create": ([ptr(u64)], c.c_int32),
            "tapirscan_destroy": ([u64], c.c_int32),
            "barcode_scan_formats": (
                [u64, c.c_void_p, u64, u64, u64, u32, u64, u32, u32, ptr(u64)],
                c.c_int32,
            ),
            "barcode_result_info": ([u64, ptr(_Info)], c.c_int32),
            "barcode_result_copy_json": ([u64, c.c_void_p, u64], c.c_int32),
            "barcode_result_destroy": ([u64], c.c_int32),
        }
        for name, (args, result) in signatures.items():
            fn = getattr(self._lib, name)
            fn.argtypes = args
            fn.restype = result
        if self._lib.barcode_abi_version() != ABI_VERSION:
            msg = "Unsupported scanner ABI"
            raise RuntimeError(msg)
        if self._lib.barcode_mode() != (
            ("low", "medium", "high", "very-high").index(mode)
        ):
            msg = "Native library mode mismatch"
            raise RuntimeError(msg)
        _check(self._lib.tapirscan_create(c.byref(self._handle)))

    def scan(
        self,
        image: ImageInput,
        width: int | None = None,
        height: int | None = None,
        *,
        channels: int = 1,
        stride: int | None = None,
        multiple: bool = True,
        debug: bool = False,
        include_regions: bool | None = None,
        formats: FormatSelection | None = None,
        layout: Layout = "auto",
        value_range: ValueRange = "auto",
    ) -> ScanResult:
        """Scan an image; use .values, .best, .image and optional .debug evidence."""
        mask = format_mask(self.formats if formats is None else formats)
        pixels = image
        debug = _debug_option(
            multiple=multiple, debug=debug, include_regions=include_regions
        )
        if width is None and height is None:
            if channels != 1 or stride is not None:
                msg = (
                    "channels/stride apply only to raw buffers with explicit dimensions"
                )
                raise ValueError(msg)

            pixels, width, height, channels = image_bytes(
                pixels, layout=layout, value_range=value_range
            )
        elif layout != "auto" or value_range != "auto":
            msg = "layout/value_range apply only to image objects"
            raise ValueError(msg)
        width = _integer(width, "width", 3)
        height = _integer(height, "height", 3)
        if isinstance(channels, bool) or channels not in (1, 3, 4):
            msg = "channels must be 1, 3 or 4"
            raise ValueError(msg)
        channels = _integer(channels, "channels", 1, 4)
        row = width * channels
        stride = row if stride is None else _integer(stride, "stride", row)
        required = (height - 1) * stride + row
        if required > MAX_IMAGE_BYTES:
            msg = "Image exceeds 128 MiB limit"
            raise ValueError(msg)
        view = memoryview(cast("bytes", pixels))
        if not view.c_contiguous or view.itemsize != 1:
            msg = "pixels must be a contiguous byte buffer"
            raise ValueError(msg)
        view = view.cast("B")
        if view.nbytes < required:
            msg = "Image buffer is too short"
            raise ValueError(msg)
        # Snapshot input before releasing the GIL in the native call.
        data = (c.c_uint8 * required).from_buffer_copy(view[:required])
        with self._lock:
            if not self._handle.value:
                msg = "Scanner is closed"
                raise RuntimeError(msg)
            result = c.c_uint64()
            flags = (0 if multiple else 1) | (2 if debug else 0)
            _check(
                self._lib.barcode_scan_formats(
                    self._handle,
                    data,
                    required,
                    width,
                    height,
                    channels,
                    stride,
                    flags,
                    mask,
                    c.byref(result),
                )
            )
            try:
                info = _Info()
                _check(self._lib.barcode_result_info(result, c.byref(info)))
                output = c.create_string_buffer(info.json_length + 1)
                _check(self._lib.barcode_result_copy_json(result, output, len(output)))
                return _from_json(
                    output.raw[: info.json_length], width, height, debug=debug
                )
            finally:
                _check(self._lib.barcode_result_destroy(result))

    def close(self) -> None:
        """Release the native scanner handle; repeated calls are safe."""
        with self._lock:
            if self._handle.value:
                handle = self._handle.value
                self._handle.value = 0
                _check(self._lib.tapirscan_destroy(handle))

    def __enter__(self) -> Self:
        """Return this scanner if it is still open."""
        if not self._handle.value:
            msg = "Scanner is closed"
            raise RuntimeError(msg)
        return self

    def __exit__(self, *exc: object) -> None:
        """Close this scanner when leaving its context."""
        self.close()

    def __del__(self) -> None:
        """Attempt cleanup without propagating finalizer failures."""
        # Finalizers must not propagate errors from partial initialization or shutdown.
        with suppress(Exception):
            self.close()


def scan(
    image: ImageInput,
    width: int | None = None,
    height: int | None = None,
    *,
    mode: Mode = "medium",
    library_dir: str | os.PathLike[str] | None = None,
    channels: int = 1,
    stride: int | None = None,
    multiple: bool = True,
    debug: bool = False,
    include_regions: bool | None = None,
    formats: FormatSelection | None = None,
    layout: Layout = "auto",
    value_range: ValueRange = "auto",
) -> ScanResult:
    """Scan one image. Use Scanner.scan with the same options for repeated frames."""
    with Scanner(mode, library_dir=library_dir) as scanner:
        return scanner.scan(
            image,
            width,
            height,
            channels=channels,
            stride=stride,
            multiple=multiple,
            debug=debug,
            include_regions=include_regions,
            formats=formats,
            layout=layout,
            value_range=value_range,
        )

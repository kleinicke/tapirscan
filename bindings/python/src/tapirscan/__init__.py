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

from ._images import _size, image_bytes
from .formats import (
    FORMAT_BITS,
    Format,
    FormatSelection,
    common_formats,
    common_linear_formats,
    format_mask,
    linear_formats,
    matrix_formats,
    resolve_formats,
    retail_formats,
)
from .inputs import ImageInput, PixelImage
from .results import (
    Barcode,
    ColorOrder,
    Diagnostics,
    EanAddOnPolicy,
    ImageSize,
    InspectionResult,
    Layout,
    Mode,
    Point,
    Rect,
    ScanResult,
    StructuredAppend,
    UndecodedRegion,
    ValueRange,
    _diagnostics,
)

__all__ = [
    "Barcode",
    "ColorOrder",
    "Diagnostics",
    "EanAddOnPolicy",
    "Format",
    "FormatSelection",
    "ImageInput",
    "ImageSize",
    "InspectionResult",
    "Layout",
    "Mode",
    "PixelImage",
    "Point",
    "Rect",
    "ScanResult",
    "Scanner",
    "ScannerError",
    "StructuredAppend",
    "UndecodedRegion",
    "ValueRange",
    "common_formats",
    "common_linear_formats",
    "inspect",
    "linear_formats",
    "matrix_formats",
    "retail_formats",
    "scan",
]
ABI_VERSION = 6
MAX_IMAGE_BYTES = 128 * 1024 * 1024
_MODES = ("low", "medium", "high", "very-high")
_ADDON_POLICIES = ("ignore", "read", "require")


_ERROR_CODES = {1: "invalid_input", 2: "disposed", 3: "engine", 4: "engine"}


class ScannerError(RuntimeError):
    """A native scanner failure; `code` matches the JavaScript error codes."""

    def __init__(self, status: int, message: str) -> None:
        """Keep the native status and name its error code."""
        self.status = status
        self.code = _ERROR_CODES.get(status, "engine")
        super().__init__(f"{message} (status {status})")


# Native ABI 6 structures from bindings/c/include/tapirscan.h.
class _ScannerOptions(c.Structure):
    _fields_ = [
        ("mode", c.c_uint32),
        ("formats", c.c_uint32),
        ("ean_add_on_policy", c.c_uint32),
    ]


class _Image(c.Structure):
    _fields_ = [
        ("data", c.c_void_p),
        ("length", c.c_uint64),
        ("width", c.c_uint64),
        ("height", c.c_uint64),
        ("channels", c.c_uint32),
        ("stride", c.c_uint64),
    ]


class _ScanOptions(c.Structure):
    _fields_ = [
        ("formats", c.c_uint32),
    ]


class _Point(c.Structure):
    _fields_ = [("x", c.c_double), ("y", c.c_double)]


class _Summary(c.Structure):
    _fields_ = [
        ("barcode_count", c.c_uint64),
        ("undecoded_count", c.c_uint64),
        ("width", c.c_uint64),
        ("height", c.c_uint64),
        ("elapsed_ms", c.c_double),
        ("mode", c.c_uint32),
    ]


class _Barcode(c.Structure):
    _fields_ = [
        ("polygon", _Point * 4),
        ("support", c.c_uint64),
        ("format", c.c_uint32),
        ("gs1", c.c_int32),
        ("reader_initialization", c.c_int32),
        ("structured_append_parity", c.c_int32),
        ("structured_append_index", c.c_uint64),
        ("structured_append_count", c.c_uint64),
        ("text_length", c.c_uint64),
        ("payload_bytes_length", c.c_uint64),
        ("ean_add_on_length", c.c_uint64),
        ("structured_append_id_length", c.c_uint64),
    ]


class _Region(c.Structure):
    _fields_ = [("polygon", _Point * 4), ("format", c.c_uint32)]


_ABSENT = 2**64 - 1
_FORMAT_NAMES: dict[int, Format] = {
    bit: cast("Format", name) for name, bit in FORMAT_BITS.items()
}
_TEXT, _PAYLOAD_BYTES, _EAN_ADD_ON, _STRUCTURED_APPEND_ID = range(4)


def _points(polygon: c.Array[_Point]) -> tuple[Point, ...]:
    return tuple(Point(p.x, p.y) for p in polygon)


def _flag(value: int) -> bool | None:
    return None if value < 0 else bool(value)


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


def _snapshot(
    image: ImageInput, layout: Layout, value_range: ValueRange, color_order: ColorOrder
) -> tuple[c.Array[c.c_uint8], int, int, int, int]:
    """Validate image storage and own its bytes before releasing the GIL."""
    if isinstance(image, PixelImage):
        if layout != "auto" or value_range != "auto" or color_order != "RGB":
            msg = (
                "layout/value_range/color_order apply only to NumPy arrays and tensors"
            )
            raise ValueError(msg)
        pixels, width, height = image.data, image.width, image.height
        channels, stride = image.channels, image.stride
    else:
        pixels, width, height, channels = image_bytes(
            image, layout=layout, value_range=value_range, color_order=color_order
        )
        stride = None
    width = _integer(width, "width", 3)
    height = _integer(height, "height", 3)
    if isinstance(channels, bool) or channels not in (1, 3, 4):
        msg = "channels must be 1, 3 or 4"
        raise ValueError(msg)
    channels = _integer(channels, "channels", 1, 4)
    _size(width, height, channels)
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
    return data, width, height, channels, stride


class Scanner:
    """A reusable scanner. Use a context manager or call close().

    library_dir may also be supplied through TAPIRSCAN_LIBRARY_DIR; otherwise the
    bundled library is used.
    Installed packages never search the cwd.
    Results are immutable typed objects and remain valid after close/next scan.
    """

    def __init__(
        self,
        mode: Mode = "medium",
        *,
        formats: FormatSelection | None = None,
        ean_add_on_policy: EanAddOnPolicy = "ignore",
        library_dir: str | os.PathLike[str] | None = None,
    ) -> None:
        """Initialize the scanner state or native error code."""
        if mode not in _MODES:
            msg = "mode must be low, medium, high or very-high"
            raise ValueError(msg)
        if ean_add_on_policy not in _ADDON_POLICIES:
            msg = 'ean_add_on_policy must be "ignore", "read" or "require"'
            raise ValueError(msg)
        self._ean_add_on_policy = ean_add_on_policy
        self._formats = resolve_formats(formats)
        directory = (
            library_dir
            if library_dir is not None
            else os.environ.get("TAPIRSCAN_LIBRARY_DIR")
        )
        if directory is None:
            directory = Path(__file__).resolve().parent / "_native"
            if not directory.is_dir():
                msg = (
                    "This installation has no bundled native library. "
                    "Install a platform wheel, or set library_dir / "
                    "TAPIRSCAN_LIBRARY_DIR for a source checkout."
                )
                raise ValueError(msg)
        prefix, suffix = (
            ("", ".dll")
            if sys.platform == "win32"
            else ("lib", ".dylib" if sys.platform == "darwin" else ".so")
        )
        path = Path(directory).expanduser().resolve() / f"{prefix}tapirscan{suffix}"
        self._lock = RLock()
        self._handle = c.c_uint64(0)
        self._lib = c.CDLL(str(path))
        self._mode = mode
        u64 = c.c_uint64
        u32 = c.c_uint32
        ptr = c.POINTER
        # Check the version before resolving ABI-specific symbols.
        self._lib.tapirscan_abi_version.argtypes = []
        self._lib.tapirscan_abi_version.restype = u32
        actual_abi = self._lib.tapirscan_abi_version()
        if actual_abi != ABI_VERSION:
            msg = (
                f"Native library ABI mismatch: expected {ABI_VERSION}, "
                f"got {actual_abi} from {path}. "
                "Rebuild the native library or reinstall a matching wheel."
            )
            raise RuntimeError(msg)
        signatures = {
            "tapirscan_status_message": ([c.c_int32], c.c_char_p),
            "tapirscan_scanner_create": (
                [ptr(_ScannerOptions), ptr(u64), c.c_void_p],
                c.c_int32,
            ),
            "tapirscan_scanner_destroy": ([u64], c.c_int32),
            "tapirscan_scan": (
                [u64, ptr(_Image), ptr(_ScanOptions), ptr(u64), c.c_void_p],
                c.c_int32,
            ),
            "tapirscan_inspect": (
                [u64, ptr(_Image), ptr(_ScanOptions), ptr(u64), c.c_void_p],
                c.c_int32,
            ),
            "tapirscan_result_count": ([u64, ptr(u64)], c.c_int32),
            "tapirscan_result_info": ([u64, ptr(_Summary)], c.c_int32),
            "tapirscan_result_barcode": ([u64, u64, ptr(_Barcode)], c.c_int32),
            "tapirscan_result_undecoded": ([u64, u64, ptr(_Region)], c.c_int32),
            "tapirscan_result_copy": (
                [u64, u64, c.c_uint32, c.c_void_p, u64],
                c.c_int32,
            ),
            "tapirscan_result_json_length": ([u64, ptr(u64)], c.c_int32),
            "tapirscan_result_copy_json": ([u64, c.c_void_p, u64], c.c_int32),
            "tapirscan_result_destroy": ([u64], c.c_int32),
        }
        for name, (args, result) in signatures.items():
            fn = getattr(self._lib, name)
            fn.argtypes = args
            fn.restype = result
        options = _ScannerOptions(
            _MODES.index(mode),
            format_mask(self._formats),
            _ADDON_POLICIES.index(ean_add_on_policy),
        )
        error = c.create_string_buffer(512)
        self._check(
            self._lib.tapirscan_scanner_create(
                c.byref(options), c.byref(self._handle), error
            ),
            error.value.decode(),
        )

    def _check(self, code: int, detail: str = "") -> None:
        if code:
            message = detail or self._lib.tapirscan_status_message(code).decode()
            raise ScannerError(code, message)

    @property
    def mode(self) -> Mode:
        """The compiled effort mode selected at creation."""
        return self._mode

    @property
    def ean_add_on_policy(self) -> EanAddOnPolicy:
        """The EAN/UPC supplement policy selected at creation."""
        return self._ean_add_on_policy

    @property
    def formats(self) -> tuple[Format, ...]:
        """Default formats; individual scans may override this selection."""
        return self._formats

    def scan(
        self,
        image: ImageInput,
        *,
        formats: FormatSelection | None = None,
        layout: Layout = "auto",
        value_range: ValueRange = "auto",
        color_order: ColorOrder = "RGB",
    ) -> ScanResult:
        """Return decoded barcodes with source-image positions."""
        return self._run(
            image,
            inspect=False,
            formats=formats,
            layout=layout,
            value_range=value_range,
            color_order=color_order,
        )

    def inspect(
        self,
        image: ImageInput,
        *,
        formats: FormatSelection | None = None,
        layout: Layout = "auto",
        value_range: ValueRange = "auto",
        color_order: ColorOrder = "RGB",
    ) -> InspectionResult:
        """Inspect barcodes, unread regions, timing and engine diagnostics."""
        result = self._run(
            image,
            inspect=True,
            formats=formats,
            layout=layout,
            value_range=value_range,
            color_order=color_order,
        )
        return cast("InspectionResult", result)

    def _run(
        self,
        image: ImageInput,
        *,
        inspect: bool,
        formats: FormatSelection | None,
        layout: Layout,
        value_range: ValueRange,
        color_order: ColorOrder,
    ) -> ScanResult:
        mask = format_mask(self.formats if formats is None else formats)
        with self._lock:
            if not self._handle.value:
                msg = "Scanner is closed"
                raise RuntimeError(msg)
        data, width, height, channels, stride = _snapshot(
            image, layout, value_range, color_order
        )
        pixels = _Image(c.addressof(data), len(data), width, height, channels, stride)
        options = _ScanOptions(mask)
        with self._lock:
            if not self._handle.value:
                msg = "Scanner is closed"
                raise RuntimeError(msg)
            result = c.c_uint64()
            error = c.create_string_buffer(512)
            self._check(
                (self._lib.tapirscan_inspect if inspect else self._lib.tapirscan_scan)(
                    self._handle,
                    c.byref(pixels),
                    c.byref(options),
                    c.byref(result),
                    error,
                ),
                error.value.decode(),
            )
            try:
                return self._read(result.value, inspect=inspect)
            finally:
                self._check(self._lib.tapirscan_result_destroy(result))

    def _read(self, result: int, *, inspect: bool) -> ScanResult:
        """Build public results from typed accessors; JSON only feeds diagnostics."""
        count = c.c_uint64()
        self._check(self._lib.tapirscan_result_count(result, c.byref(count)))
        barcodes = tuple(self._barcode(result, i) for i in range(count.value))
        if not inspect:
            return ScanResult(barcodes)
        info = _Summary()
        self._check(self._lib.tapirscan_result_info(result, c.byref(info)))
        undecoded = tuple(
            self._undecoded(result, i) for i in range(info.undecoded_count)
        )
        length = c.c_uint64()
        self._check(self._lib.tapirscan_result_json_length(result, c.byref(length)))
        output = c.create_string_buffer(length.value + 1)
        self._check(self._lib.tapirscan_result_copy_json(result, output, len(output)))
        return InspectionResult(
            barcodes,
            cast("Mode", _MODES[info.mode]),
            info.elapsed_ms,
            undecoded,
            ImageSize(info.width, info.height),
            _diagnostics(output.raw[: length.value], undecoded),
        )

    def _undecoded(self, result: int, index: int) -> UndecodedRegion:
        region = _Region()
        self._check(
            self._lib.tapirscan_result_undecoded(result, index, c.byref(region))
        )
        name = _FORMAT_NAMES.get(region.format, "Unknown")
        return UndecodedRegion(_points(region.polygon), name)

    def _barcode(self, result: int, index: int) -> Barcode:
        native = _Barcode()
        self._check(self._lib.tapirscan_result_barcode(result, index, c.byref(native)))

        def field(kind: int, length: int) -> bytes | None:
            if length == _ABSENT:
                return None
            buffer = c.create_string_buffer(length + 1)
            self._check(
                self._lib.tapirscan_result_copy(result, index, kind, buffer, length + 1)
            )
            return buffer.raw[:length]

        text = field(_TEXT, native.text_length) or b""
        add_on = field(_EAN_ADD_ON, native.ean_add_on_length)
        append_id = field(_STRUCTURED_APPEND_ID, native.structured_append_id_length)
        parity = native.structured_append_parity
        append = (
            StructuredAppend(
                native.structured_append_index,
                native.structured_append_count,
                append_id.decode() if append_id is not None else None,
                parity if parity >= 0 else None,
            )
            if native.structured_append_count
            else None
        )
        return Barcode(
            text.decode(),
            _points(native.polygon),
            _FORMAT_NAMES[native.format],
            native.support,
            _flag(native.gs1),
            _flag(native.reader_initialization),
            append,
            add_on.decode() if add_on is not None else None,
            field(_PAYLOAD_BYTES, native.payload_bytes_length),
        )

    def close(self) -> None:
        """Release the native scanner handle; repeated calls are safe."""
        with self._lock:
            if self._handle.value:
                handle = self._handle.value
                self._handle.value = 0
                self._check(self._lib.tapirscan_scanner_destroy(handle))

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
    *,
    mode: Mode = "medium",
    ean_add_on_policy: EanAddOnPolicy = "ignore",
    library_dir: str | os.PathLike[str] | None = None,
    formats: FormatSelection | None = None,
    layout: Layout = "auto",
    value_range: ValueRange = "auto",
    color_order: ColorOrder = "RGB",
) -> ScanResult:
    """Scan one image; create a Scanner to reuse its mode and supplement policy."""
    with Scanner(
        mode,
        formats=formats,
        ean_add_on_policy=ean_add_on_policy,
        library_dir=library_dir,
    ) as scanner:
        return scanner.scan(
            image,
            layout=layout,
            value_range=value_range,
            color_order=color_order,
        )


def inspect(
    image: ImageInput,
    *,
    mode: Mode = "medium",
    ean_add_on_policy: EanAddOnPolicy = "ignore",
    library_dir: str | os.PathLike[str] | None = None,
    formats: FormatSelection | None = None,
    layout: Layout = "auto",
    value_range: ValueRange = "auto",
    color_order: ColorOrder = "RGB",
) -> InspectionResult:
    """Inspect one image, including timing and engine diagnostics."""
    with Scanner(
        mode,
        formats=formats,
        ean_add_on_policy=ean_add_on_policy,
        library_dir=library_dir,
    ) as scanner:
        return scanner.inspect(
            image,
            layout=layout,
            value_range=value_range,
            color_order=color_order,
        )

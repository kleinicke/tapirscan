"""Barcode scanning with bundled native libraries and optional image adapters."""

from __future__ import annotations

import ctypes as c
import threading
from collections import deque
from concurrent.futures import Future, ThreadPoolExecutor
from contextlib import suppress
from threading import RLock
from typing import TYPE_CHECKING, cast

from typing_extensions import Self

if TYPE_CHECKING:
    import os
    from collections.abc import Iterable

from ._images import _cpu_count, _size, image_bytes
from ._library import ABI_VERSION as ABI_VERSION
from ._library import ScannerError, library_path, load
from .content import Content, ContentField, ContentKind, parse_content
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
    "Content",
    "ContentField",
    "ContentKind",
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
    "parse_content",
    "retail_formats",
    "scan",
    "scan_many",
]
MAX_IMAGE_BYTES = 1024 * 1024 * 1024
_MODES = ("low", "medium", "high", "very-high")
_ADDON_POLICIES = ("ignore", "read", "require")


# Native ABI 7 structures from bindings/c/include/tapirscan.h.
class _ScannerOptions(c.Structure):
    _fields_ = [
        ("mode", c.c_uint32),
        ("formats", c.c_uint32),
        ("ean_add_on_policy", c.c_uint32),
        ("parallel", c.c_uint32),
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
) -> tuple[memoryview, int, int, int, int, bytes | None]:
    """Validate image storage; return the exact view and any immutable owner.

    A ``bytes`` object, such as a Pillow conversion or raw ``PixelImage`` bytes, can
    be scanned in place: it cannot change while the GIL is released.
    """
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
        msg = "Image exceeds 1 GiB limit"
        raise ValueError(msg)
    view = memoryview(cast("bytes", pixels))
    if not view.c_contiguous or view.itemsize != 1:
        msg = "pixels must be a contiguous byte buffer"
        raise ValueError(msg)
    view = view.cast("B")
    if view.nbytes < required:
        msg = "Image buffer is too short"
        raise ValueError(msg)
    # Under its lock, the scanner copies this view into its own buffer before
    # releasing the GIL, unless `immutable` lets it scan the bytes in place.
    immutable = pixels if type(pixels) is bytes else None
    return view[:required], width, height, channels, stride, immutable


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
        parallel: bool | None = None,
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
        self._parallel = parallel
        self._formats = resolve_formats(formats)
        path = library_path(library_dir)
        self._lock = RLock()
        # Reused snapshot of caller pixels; grows to the largest image until close().
        self._staging: c.Array[c.c_uint8] | None = None
        self._handle = c.c_uint64(0)
        self._lib = load(path)
        self._mode = mode
        u64 = c.c_uint64
        ptr = c.POINTER
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
            2 if parallel is None else int(bool(parallel)),
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
    def parallel(self) -> bool | None:
        """Requested threading; None follows the process default."""
        return self._parallel

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
        data, width, height, channels, stride, immutable = _snapshot(
            image, layout, value_range, color_order
        )
        options = _ScanOptions(mask)
        with self._lock:
            if not self._handle.value:
                msg = "Scanner is closed"
                raise RuntimeError(msg)
            length = data.nbytes
            if immutable is not None:
                # `immutable` stays referenced until the call returns.
                address = c.cast(c.c_char_p(immutable), c.c_void_p).value or 0
            else:
                # Snapshot input before releasing the GIL in the native call. A
                # reused buffer avoids faulting in fresh memory for every image.
                if self._staging is None or len(self._staging) < length:
                    self._staging = (c.c_uint8 * length)()
                memoryview(self._staging).cast("B")[:length] = data
                address = c.addressof(self._staging)
            pixels = _Image(address, length, width, height, channels, stride)
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
                self._staging = None
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
    parallel: bool | None = None,
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
        parallel=parallel,
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
    parallel: bool | None = None,
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
        parallel=parallel,
        library_dir=library_dir,
    ) as scanner:
        return scanner.inspect(
            image,
            layout=layout,
            value_range=value_range,
            color_order=color_order,
        )


def scan_many(
    images: Iterable[ImageInput],
    *,
    mode: Mode = "medium",
    ean_add_on_policy: EanAddOnPolicy = "ignore",
    parallel: bool = False,
    library_dir: str | os.PathLike[str] | None = None,
    formats: FormatSelection | None = None,
    layout: Layout = "auto",
    value_range: ValueRange = "auto",
    color_order: ColorOrder = "RGB",
    workers: int | None = None,
) -> list[ScanResult]:
    """Scan several images on worker threads; results keep the input order.

    Each worker thread uses its own Scanner, and scanning releases the GIL, so
    throughput scales with CPU cores. Because the workers already keep every core
    busy, each scanner runs its formats on one thread unless ``parallel`` is true.
    ``workers`` defaults to the CPUs this process
    may use. Images are consumed lazily, at most two per worker ahead of the
    scans, so a yielded array must not be modified until ``scan_many`` returns;
    yield a new array per image. After an image fails, no further images are
    consumed; the earliest failure is raised once the started scans finish.
    """
    if workers is not None and (isinstance(workers, bool) or workers < 1):
        msg = "workers must be a positive integer or None"
        raise ValueError(msg)
    count = workers or _cpu_count()
    local = threading.local()
    created: list[Scanner] = []
    created_lock = RLock()

    def scan_one(image: ImageInput) -> ScanResult:
        scanner = cast("Scanner | None", getattr(local, "scanner", None))
        if scanner is None:
            scanner = Scanner(
                mode,
                formats=formats,
                ean_add_on_policy=ean_add_on_policy,
                parallel=parallel,
                library_dir=library_dir,
            )
            local.scanner = scanner
            with created_lock:
                created.append(scanner)
        return scanner.scan(
            image, layout=layout, value_range=value_range, color_order=color_order
        )

    try:
        with ThreadPoolExecutor(count, thread_name_prefix="tapirscan") as pool:
            # A bounded window: a generator of decoded images is consumed only as
            # fast as it is scanned, instead of being held in memory all at once.
            window: deque[Future[ScanResult]] = deque()
            done: list[Future[ScanResult]] = []
            for image in images:
                if len(window) >= 2 * count:
                    oldest = window.popleft()
                    done.append(oldest)
                    if oldest.exception() is not None:
                        break
                window.append(pool.submit(scan_one, image))
            done.extend(window)
            return [future.result() for future in done]
    finally:
        for scanner in created:
            scanner.close()

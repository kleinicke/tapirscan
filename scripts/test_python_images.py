#!/usr/bin/env python3
"""Image adapters and unified typed API against the actual native scanner."""

import ctypes
import json
import os
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path
from typing import Any
from unittest.mock import PropertyMock, patch

import numpy as np
import torch
from fixture_data import TEXT, fixtures
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(
    0, os.environ.get("BARCODE_PYTHON_PACKAGE", str(ROOT / "bindings/python/src"))
)
import tapirscan as barcode
from tapirscan import Barcode, PixelImage, Scanner, StructuredAppend
from tapirscan._images import image_bytes
from tapirscan.results import _diagnostics

decode = barcode.inspect

LIBS = ROOT / "build/native"
_, RAW, W, H, *_ = next(fixtures())
GRAY = np.frombuffer(RAW, dtype=np.uint8).reshape(H, W).copy()


class _TypedResult:
    """A fake native library serving one hand-made barcode through the typed ABI."""

    def __init__(
        self,
        fields: tuple[bytes | None, ...],
        gs1: int,
        initialization: int,
        parity: int,
    ) -> None:
        """Keep the field bytes (None = absent) and the flag values."""
        self.fields, self.flags = fields, (gs1, initialization, parity)

    def tapirscan_result_barcode(self, _result: int, _index: int, out: Any) -> int:  # noqa: ANN401
        """Fill the struct behind a ctypes byref() argument."""
        native = out._obj  # noqa: SLF001
        native.format = barcode.formats.FORMAT_BITS["QRCode"]
        gs1, initialization, parity = self.flags
        native.gs1, native.reader_initialization = gs1, initialization
        native.structured_append_parity = parity
        native.structured_append_index, native.structured_append_count = 2, 3
        absent = 2**64 - 1
        (
            native.text_length,
            native.payload_bytes_length,
            native.ean_add_on_length,
            native.structured_append_id_length,
        ) = [absent if f is None else len(f) for f in self.fields]
        return 0

    def tapirscan_result_copy(
        self,
        _result: int,
        _index: int,
        kind: int,
        out: Any,  # noqa: ANN401
        size: int,
    ) -> int:
        """Copy one field plus its NUL terminator."""
        data = (self.fields[kind] or b"") + b"\x00"
        ctypes.memmove(out, data, min(size, len(data)))
        return 0


class Images(unittest.TestCase):
    """Verify supported image inputs and immutable scan results."""

    def check_image(
        self, image: barcode.ImageInput, *, value_range: barcode.ValueRange = "auto"
    ) -> None:
        """Verify an image produces the expected typed barcode in both modes."""
        for mode in ("low", "medium", "high", "very-high"):
            reads = decode(image, mode=mode, library_dir=LIBS, value_range=value_range)
            self.assertEqual(reads.values, [TEXT])
            self.assertIsInstance(reads.barcodes[0], Barcode)
            self.assertEqual(reads.barcodes[0].format, "EAN13")
            self.assertGreater(reads.barcodes[0].rect.width, 0)
            self.assertEqual(len(reads.barcodes[0].polygon), 4)

    def test_old_native_abi(self) -> None:
        """Reject older libraries at initialization with actionable version details."""
        with patch("tapirscan.c.CDLL") as load:
            load.return_value.tapirscan_abi_version.return_value = 4
            with self.assertRaisesRegex(RuntimeError, "expected 6, got 4.*Rebuild"):
                Scanner(library_dir=LIBS)
            load.return_value.tapirscan_scanner_create.assert_not_called()
            load.return_value.tapirscan_result_json_length.assert_not_called()

    def test_supplement_policy(self) -> None:
        """Policy is opt-in, creation-only and forwarded by one-shot scanning."""
        with Scanner(library_dir=LIBS) as scanner:
            self.assertEqual(scanner.ean_add_on_policy, "ignore")
        self.assertEqual(
            decode(
                PixelImage(RAW, width=W, height=H),
                library_dir=LIBS,
                ean_add_on_policy="require",
            ).values,
            [],
        )
        with self.assertRaisesRegex(ValueError, "ean_add_on_policy"):
            Scanner(ean_add_on_policy="Read")  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]

    def test_raw_and_pillow(self) -> None:
        """Verify raw and pillow."""
        self.check_image(PixelImage(RAW, width=W, height=H))
        for mode in ("L", "1", "RGB", "RGBA", "P", "CMYK", "I", "F"):
            with self.subTest(mode=mode):
                self.check_image(Image.fromarray(GRAY).convert(mode))

    def test_pillow_precision(self) -> None:
        """High precision storage must not silently clip or accept nonfinite pixels."""
        for dtype in (np.uint16, np.int32, np.float32):
            pixels = GRAY.astype(dtype)
            self.assertEqual(image_bytes(Image.fromarray(pixels))[0], RAW)
            for value in (256, 65535):
                pixels[0, 0] = value
                with self.assertRaisesRegex(ValueError, "rescale explicitly"):
                    image_bytes(Image.fromarray(pixels))
        for value in (float("nan"), float("inf"), -1.0):
            pixels = GRAY.astype(np.float32)
            pixels[1, 1] = value
            with self.assertRaises(ValueError):
                image_bytes(Image.fromarray(pixels))

    def test_format_override_and_configuration(self) -> None:
        """Single identifiers work and per-call selections do not alter defaults."""
        image = PixelImage(RAW, width=W, height=H)
        with Scanner("low", formats="EAN13", library_dir=LIBS) as scanner:
            self.assertEqual(scanner.inspect(image).values, [TEXT])
            self.assertEqual(scanner.inspect(image, formats="QRCode").values, [])
            self.assertEqual(scanner.inspect(image).values, [TEXT])
            with self.assertRaises(AttributeError):
                scanner.mode = "high"  # ty: ignore[invalid-assignment]
            with self.assertRaises(AttributeError):
                scanner.formats = ("QRCode",)  # ty: ignore[invalid-assignment]
        self.assertEqual(
            decode(image, formats="EAN13", library_dir=LIBS).values, [TEXT]
        )

    def test_retail_and_common_presets(self) -> None:
        """Presets expose their exact selection and decode through the public API."""
        self.assertEqual(barcode.retail_formats, ("EAN13", "UPCA", "EAN8", "UPCE"))
        with Scanner(library_dir=LIBS) as scanner:
            self.assertEqual(scanner.formats, barcode.retail_formats)
        self.assertEqual(
            barcode.common_linear_formats,
            (*barcode.retail_formats, "Code128", "Code39", "ITF"),
        )
        self.assertEqual(
            barcode.common_formats,
            (*barcode.common_linear_formats, "QRCode", "DataMatrix"),
        )
        for preset, expected in (
            ("retail", barcode.retail_formats),
            ("common1D", barcode.common_linear_formats),
            ("common", barcode.common_formats),
        ):
            with Scanner(formats=preset, library_dir=LIBS) as scanner:
                self.assertEqual(scanner.formats, expected)
                self.assertEqual(
                    scanner.inspect(PixelImage(RAW, width=W, height=H)).values, [TEXT]
                )

    def test_typed_results_match_engine_report(self) -> None:
        """Public fields come from typed accessors and agree with the engine report."""
        report = decode(PixelImage(RAW, width=W, height=H), library_dir=LIBS)
        raw = report.diagnostics.to_raw_dict()
        engine = raw["scan"]["barcodes"]
        self.assertEqual([b.text for b in report.barcodes], [b["text"] for b in engine])
        self.assertEqual(
            [b.polygon for b in report.barcodes],
            [tuple(map(tuple, b["polygon"])) for b in engine],
        )
        self.assertEqual((report.mode, report.image), (raw["mode"], (W, H)))
        self.assertGreaterEqual(report.elapsed_ms, 0)
        exported = report.as_dict()
        self.assertEqual(exported["best"], report.barcodes[0].as_dict())
        self.assertEqual(exported["values"], [TEXT])
        with self.assertRaises(FrozenInstanceError):
            report.barcodes[0].text = "changed"  # ty: ignore[invalid-assignment]

    def test_search_window_evidence(self) -> None:
        """Preserve absent, empty and populated search evidence."""
        raw = decode(
            PixelImage(RAW, width=W, height=H), library_dir=LIBS
        ).diagnostics.to_raw_dict()
        window = {
            "kind": "full_frame_search",
            "polygon": [[0, 0], [W, 0], [W, H], [0, H]],
            "candidateIndex": 7,
        }
        for windows in (None, [], [window]):
            with self.subTest(windows=windows):
                if windows is None:
                    raw.pop("searchWindows", None)
                else:
                    raw["searchWindows"] = windows
                regions = _diagnostics(json.dumps(raw).encode(), ()).regions
                if regions is None:
                    self.fail("Region evidence was requested")
                if windows is None:
                    self.assertIsNone(regions.search_windows)
                else:
                    if regions.search_windows is None:
                        self.fail("Search evidence was dropped")
                    self.assertEqual(
                        [
                            {
                                "kind": item.kind,
                                "polygon": [list(point) for point in item.polygon],
                                "candidateIndex": item.candidate_index,
                            }
                            for item in regions.search_windows
                        ],
                        windows,
                    )

    def test_numpy(self) -> None:
        """Verify numpy."""

        class ArraySubclass(np.ndarray[tuple[int, ...], np.dtype[np.uint8]]):
            pass

        for a in (
            GRAY.view(ArraySubclass),
            GRAY,
            np.asfortranarray(GRAY),
            GRAY.T,
            GRAY[:, ::-1],
            np.repeat(GRAY[:, :, None], 3, axis=2),
        ):
            self.check_image(a)
        self.check_image(GRAY.astype(np.float32), value_range="0_255")
        # RGB channels are preserved for native luminance conversion.
        color = np.zeros((H, W, 3), dtype=np.uint8)
        color[:, :, 0] = GRAY
        self.assertEqual(image_bytes(color)[0], color.tobytes())
        self.assertEqual(image_bytes(color), image_bytes(torch.from_numpy(color)))

    def test_numpy_ranges_and_layouts(self) -> None:
        """NumPy and tensor adapters agree and reject lossy implicit conversion."""
        for pixels in (
            GRAY.astype(np.float32) / 255,
            GRAY.astype(bool),
            np.repeat(GRAY[None], 3, axis=0),
            np.repeat(GRAY[:, :, None], 4, axis=2),
        ):
            self.assertEqual(image_bytes(pixels), image_bytes(torch.from_numpy(pixels)))
            self.check_image(pixels)
        for pixels in (
            np.full((5, 6), 65535, dtype=np.uint16),
            np.full((5, 6), -1.0),
            np.full((5, 6), np.nan),
            np.zeros((3, 10, 4)),
        ):
            with self.assertRaises(ValueError):
                image_bytes(pixels)
        self.assertEqual(
            image_bytes(np.ones((5, 6)), value_range="0_255")[0], bytes([1]) * 30
        )
        image_bytes(np.zeros((3, 10, 4)), layout="CHW")
        with Scanner(formats="1D", library_dir=LIBS) as scanner:
            self.assertEqual(scanner.inspect(GRAY).values, [TEXT])
            self.assertEqual(scanner.inspect(GRAY, formats="2D").values, [])

    def test_tensor_layouts_and_dtypes(self) -> None:
        """Verify tensor layouts and dtypes."""
        gray = torch.from_numpy(GRAY)
        images = [
            gray,
            gray.T,
            gray.unsqueeze(0),
            gray.unsqueeze(-1),
            gray.expand(3, -1, -1),
            gray.expand(4, -1, -1).permute(1, 2, 0),
            gray[None, None],
            gray.bool(),
            gray.to(torch.int16),
        ]
        for dtype in (torch.float16, torch.float32, torch.float64, torch.bfloat16):
            images.append(gray.to(dtype) / 255)
            self.check_image(gray.to(dtype), value_range="0_255")
        for image in images:
            with self.subTest(shape=image.shape, dtype=image.dtype):
                self.check_image(image)
        self.assertEqual(
            image_bytes(torch.ones(5, 6), value_range="0_255")[0], bytes([1]) * 30
        )
        self.assertEqual(image_bytes(torch.ones(5, 6))[0], bytes([255]) * 30)
        image_bytes(torch.zeros(3, 10, 4), layout="CHW")

    def test_autograd(self) -> None:
        """Verify autograd."""
        leaf = torch.from_numpy(GRAY).float().requires_grad_()
        image = leaf / 255
        before = image.detach().clone()
        self.check_image(image)
        self.assertTrue(torch.equal(before, image))
        self.assertTrue(image.requires_grad)
        image.sum().backward()
        torch.testing.assert_close(leaf.grad, torch.full_like(leaf, 1 / 255))

    def test_options_and_regions(self) -> None:
        """Verify options and regions."""
        _, raw, w, h, *_ = list(fixtures())[2]
        image = Image.frombytes("L", (w, h), raw)
        self.assertEqual(len(decode(image, library_dir=LIBS).barcodes), 2)
        self.assertIsNotNone(decode(image, library_dir=LIBS).best)
        with Scanner(library_dir=LIBS) as scanner:
            result = scanner.inspect(
                torch.from_numpy(np.asarray(image).copy()),
            )
        if result.diagnostics is None or result.diagnostics.regions is None:
            self.fail("Region evidence was requested")
        # Medium searches the full frame only on unread barcode evidence.
        self.assertIsNotNone(result.diagnostics.regions.search_windows)

    def test_scan_returns_only_barcodes(self) -> None:
        """Ordinary results own values and geometry without inspection evidence."""
        image = PixelImage(RAW, width=W, height=H)
        with Scanner(library_dir=LIBS) as scanner:
            result = scanner.scan(image)
            self.assertIs(type(result), barcode.ScanResult)
            self.assertEqual(result.values, [TEXT])
            self.assertFalse(hasattr(result, "diagnostics"))
            report = scanner.inspect(image)
            self.assertEqual(result.barcodes, report.barcodes)
            self.assertEqual(result.best, report.best)
            self.assertEqual(
                scanner.scan(
                    PixelImage(bytes([255]) * len(RAW), width=W, height=H)
                ).values,
                [],
            )
            with self.assertRaises(TypeError):
                scanner.scan(image, debug=True)  # ty: ignore[unknown-argument]
        self.assertEqual([b.text for b in result.barcodes], [TEXT])

    def test_unified_result(self) -> None:
        """Verify unified result."""
        result = barcode.inspect(PixelImage(RAW, width=W, height=H), library_dir=LIBS)
        self.assertEqual(result.values, [TEXT])
        self.assertEqual(result.barcodes[0].text, TEXT)
        self.assertGreater(result.barcodes[0].support, 0)
        self.assertIsNone(result.barcodes[0].structured_append)
        self.assertEqual(result.best, result.barcodes[0])
        self.assertIsNotNone(result.diagnostics)
        self.assertEqual(result.image, (W, H))
        details = barcode.inspect(
            PixelImage(RAW, width=W, height=H),
            library_dir=LIBS,
        )
        if details.diagnostics is None:
            self.fail("Diagnostics were requested")
        regions = details.diagnostics.regions
        if regions is None:
            self.fail("Region evidence was requested")
        self.assertTrue(regions.proposals)
        self.assertTrue(regions.candidates)
        if regions.search_windows is None:
            self.fail("Search windows were requested")
        self.assertEqual(
            [
                {
                    "kind": window.kind,
                    "polygon": [list(point) for point in window.polygon],
                    "candidateIndex": window.candidate_index,
                }
                for window in regions.search_windows
            ],
            details.diagnostics.to_raw_dict()["searchWindows"],
        )
        self.assertTrue(any(c.detections for c in regions.candidates))
        for candidate in regions.candidates:
            self.assertEqual(len(candidate.polygon), 4)
            for detection in candidate.detections:
                self.assertEqual(detection.text, TEXT)

        with self.assertRaises(FrozenInstanceError):
            result.barcodes[0].text = "changed"  # ty: ignore[invalid-assignment]
        exported = result.diagnostics.to_raw_dict()
        exported["scan"]["barcodes"].clear()
        self.assertEqual(result.values, [TEXT])
        blank = barcode.inspect(
            PixelImage(bytes([255]) * (W * H), width=W, height=H), library_dir=LIBS
        )
        self.assertEqual(blank.barcodes, ())
        self.assertEqual(blank.values, [])
        self.assertIsNone(blank.best)

    def test_predictable_float_range(self) -> None:
        """A stray bright pixel must not silently change the scale of the image."""
        image = np.full((5, 6), 0.5, dtype=np.float32)
        for convert in (np.asarray, torch.from_numpy):
            self.assertEqual(image_bytes(convert(image))[0], bytes([128]) * 30)
            bright = image.copy()
            bright[0, 0] = 1.01
            with self.assertRaisesRegex(ValueError, "selected range"):
                image_bytes(convert(bright))
            self.assertEqual(image_bytes(convert(bright), value_range="0_255")[0][1], 0)

    def test_optional_color_order(self) -> None:
        """BGR/BGRA views preserve alpha, layouts, caller data and tensor gradients."""
        for channels, order in ((3, [2, 1, 0]), (4, [2, 1, 0, 3])):
            rgb = np.zeros((H, W, channels), dtype=np.uint8)
            rgb[:, :, 0] = GRAY
            rgb[:, :, 1] = 17
            rgb[:, :, 2] = 201
            rgb[:, :, 3:] = 33
            bgr = rgb[:, :, order]
            original = bgr.copy()
            for pixels in (bgr, torch.from_numpy(bgr)):
                self.assertEqual(
                    image_bytes(pixels, color_order="BGR"), image_bytes(rgb)
                )
            self.assertTrue(np.array_equal(bgr, original))
            tensor = torch.from_numpy(bgr).permute(2, 0, 1).float().requires_grad_()
            self.assertEqual(
                image_bytes(
                    tensor, color_order="BGR", layout="CHW", value_range="0_255"
                ),
                image_bytes(rgb),
            )
            self.assertTrue(tensor.requires_grad)
        with Scanner(library_dir=LIBS) as scanner:
            self.assertEqual(scanner.inspect(GRAY, color_order="BGR").values, [TEXT])
        self.assertEqual(
            barcode.inspect(GRAY, color_order="BGR", library_dir=LIBS).values, [TEXT]
        )
        with self.assertRaisesRegex(ValueError, "only"):
            image_bytes(Image.fromarray(GRAY), color_order="BGR")
        with self.assertRaisesRegex(ValueError, "color_order"):
            image_bytes(GRAY, color_order="typo")  # ty: ignore[invalid-argument-type]

    def test_public_serialization(self) -> None:
        """JSON export contains application fields and owns its nested collections."""
        with Scanner(library_dir=LIBS) as scanner:
            result = scanner.inspect(
                GRAY,
            )
        exported = result.as_dict()
        self.assertEqual(json.loads(json.dumps(exported)), exported)
        self.assertNotIn("_json", exported)
        self.assertNotIn("scan", exported)
        self.assertNotIn("debug", exported)
        self.assertEqual(exported["image"], {"width": W, "height": H})
        self.assertEqual(exported["best"], result.barcodes[0].as_dict())
        exported.clear()
        self.assertEqual(result.as_dict()["values"], [TEXT])

    def test_scan_never_serializes_json(self) -> None:
        """Ordinary scans read typed results; only inspect fetches the engine JSON."""
        refuse = AssertionError("scan() requested JSON")
        with Scanner(library_dir=LIBS) as scanner:
            lib = scanner._lib  # noqa: SLF001
            with (
                patch.object(lib, "tapirscan_result_json_length", side_effect=refuse),
                patch.object(lib, "tapirscan_result_copy_json", side_effect=refuse),
            ):
                self.assertEqual(
                    scanner.scan(PixelImage(RAW, width=W, height=H)).values, [TEXT]
                )

    def test_typed_barcode_fields_keep_absence_and_bytes(self) -> None:
        """Absent and empty fields stay distinct; embedded NUL bytes survive."""
        cases = [
            # text, payload, add-on, append ID; gs1, initialization, parity
            ((b"a\x00b", None, b"", None), (-1, 0, -1)),
            ((b"", b"\x00\xff", None, b"group"), (1, -1, 7)),
        ]
        for fields, (gs1, initialization, parity) in cases:
            reader = Scanner.__new__(Scanner)
            reader._lib = _TypedResult(fields, gs1, initialization, parity)  # noqa: SLF001  # ty: ignore[invalid-assignment]
            read = reader._barcode(1, 0)  # noqa: SLF001
            text, payload, add_on, append_id = fields
            with self.subTest(text=text):
                self.assertEqual(read.text, (text or b"").decode())
                self.assertEqual(read.payload_bytes, payload)
                self.assertEqual(
                    read.ean_add_on, None if add_on is None else add_on.decode()
                )
                self.assertEqual(read.gs1, None if gs1 < 0 else bool(gs1))
                self.assertEqual(
                    read.reader_initialization,
                    None if initialization < 0 else bool(initialization),
                )
                self.assertEqual(
                    read.structured_append,
                    StructuredAppend(
                        2,
                        3,
                        None if append_id is None else append_id.decode(),
                        None if parity < 0 else parity,
                    ),
                )

    def test_pillow_16_bit_modes(self) -> None:
        """Every documented I;16 variant scans as intensities in [0,255]."""
        values = np.frombuffer(RAW, dtype=np.uint8).astype(np.uint16)
        for mode, dtype in (("I;16", "<u2"), ("I;16L", "<u2"), ("I;16B", ">u2")):
            with self.subTest(mode=mode):
                image = Image.frombytes(mode, (W, H), values.astype(dtype).tobytes())
                self.assertEqual(decode(image, library_dir=LIBS).values, [TEXT])

    def test_undecoded_region_types(self) -> None:
        """Undecoded regions are engine geometry, shared with the diagnostics."""
        square = tuple(
            barcode.Point(x, y) for x, y in ((0, 0), (10, 0), (10, 10), (0, 10))
        )
        region = barcode.UndecodedRegion(square, "DataMatrix")
        self.assertNotIsInstance(region, Barcode)
        self.assertFalse(hasattr(region, "text"))
        self.assertFalse(hasattr(region, "payload_bytes"))
        with self.assertRaises(FrozenInstanceError):
            region.format = "Unknown"  # ty: ignore[invalid-assignment]
        with Scanner("high", formats="all", library_dir=LIBS) as scanner:
            report = scanner.inspect(PixelImage(RAW, width=W, height=H))
        regions = report.diagnostics.regions
        if regions is None:
            self.fail("Missing requested region evidence")
        self.assertEqual(regions.undecoded, report.undecoded)

    def test_oversized_images_fail_before_conversion(self) -> None:
        """Check dimensions before copying buffers or converting optional inputs."""
        width, height = 8192, 4097
        # Broadcast views represent an oversized image without allocating its pixels.
        array = np.broadcast_to(np.zeros(1, dtype=np.uint8), (height, width))
        tensor = torch.zeros(1, dtype=torch.uint8).expand(height, width)
        with Scanner("low", library_dir=LIBS) as scanner:
            for image in (PixelImage(b"", width=width, height=height), array, tensor):
                with (
                    self.subTest(input_type=type(image).__name__),
                    self.assertRaisesRegex(ValueError, "32 megapixel"),
                ):
                    scanner.inspect(image)
            with (
                Image.new("L", (3, 3)) as image,
                patch.object(
                    Image.Image,
                    "size",
                    new_callable=PropertyMock,
                    return_value=(width, height),
                ),
                patch.object(Image.Image, "convert") as convert,
            ):
                with self.assertRaisesRegex(ValueError, "32 megapixel"):
                    scanner.inspect(image)
                convert.assert_not_called()
            # The exact pixel-count boundary reaches storage validation.
            with self.assertRaisesRegex(ValueError, "buffer is too short"):
                scanner.inspect(PixelImage(b"", width=width, height=height - 1))

    def test_native_error_messages(self) -> None:
        """Errors carry a shared code name, the native status and its message."""
        with Scanner(library_dir=LIBS) as scanner:
            for status, code, message in (
                (1, "invalid_input", "Invalid scanner arguments"),
                (2, "disposed", "handle"),
                (3, "engine", "buffer is too small"),
                (4, "engine", "Internal scanner"),
                (99, "engine", "Unknown scanner status"),
            ):
                with self.subTest(status=status):
                    with self.assertRaises(barcode.ScannerError) as raised:
                        scanner._check(status)  # noqa: SLF001
                    self.assertIsInstance(raised.exception, RuntimeError)
                    self.assertEqual(raised.exception.code, code)
                    self.assertEqual(raised.exception.status, status)
                    self.assertIn(message, str(raised.exception))
                    self.assertIn(f"status {status}", str(raised.exception))

    def test_invalid(self) -> None:
        """Verify invalid."""
        for image in (
            torch.zeros(2, 1, 10, 10),
            torch.zeros(3, 10, 4),
            torch.zeros(5, 6, dtype=torch.complex64),
            torch.zeros(5, 6, device="meta"),
            torch.full((5, 6), float("nan")),
            torch.full((5, 6), -1.0),
            torch.full((5, 6), 256.0),
            torch.zeros(5, 6).to_sparse(),
        ):
            with self.subTest(image_type=type(image)), self.assertRaises(ValueError):
                image_bytes(image)

    def test_pixel_storage_and_removed_options(self) -> None:
        """Raw storage is explicit; old signatures and aliases fail clearly."""
        with Scanner(library_dir=LIBS) as scanner:
            for image in (
                PixelImage(RAW[:-1], width=W, height=H),
                PixelImage(RAW, width=True, height=H),
                PixelImage(RAW, width=W, height=H, channels=2),
                PixelImage(RAW, width=W, height=H, stride=W - 1),
                PixelImage(memoryview(RAW)[::2], width=W, height=H),
                PixelImage(RAW, width=W, height=H, stride=128 * 1024 * 1024),
            ):
                with self.assertRaises(ValueError):
                    scanner.inspect(image)
            image = PixelImage(RAW, width=W, height=H)
            for options in (
                {"multiple": False},
                {"include_regions": True},
                {"debug": 1},
            ):
                with self.assertRaises(TypeError):
                    scanner.inspect(image, **options)  # ty: ignore[invalid-argument-type]
            with self.assertRaises(TypeError):
                scanner.inspect((RAW, W, H))  # ty: ignore[invalid-argument-type]
            for options in ({"layout": "HW"}, {"value_range": "0_255"}):
                with self.assertRaises(ValueError):
                    scanner.inspect(image, **options)  # ty: ignore[invalid-argument-type]
            for name in ("data", "type", "quality", "orientation"):
                self.assertFalse(hasattr(scanner.inspect(image).barcodes[0], name))
            scanner.close()
            # Closure is checked before expensive conversion or validation.
            with self.assertRaisesRegex(RuntimeError, "closed"):
                scanner.inspect(PixelImage(b"", width=W, height=H))

    @unittest.skipUnless(torch.cuda.is_available(), "CUDA hardware unavailable")
    def test_cuda(self) -> None:
        """Verify cuda."""
        self.check_image(
            torch.from_numpy(GRAY).to("cuda").float().requires_grad_() / 255
        )

    @unittest.skipUnless(torch.backends.mps.is_available(), "MPS hardware unavailable")
    def test_mps(self) -> None:
        """Verify MPS when the runtime can actually allocate device memory."""
        try:
            image = torch.from_numpy(GRAY).to("mps").float().requires_grad_() / 255
        except RuntimeError as reason:
            self.skipTest(f"MPS allocation unavailable on this host: {reason}")
        self.check_image(image)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Image adapters and unified typed API against the actual native scanner."""

import json
import os
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path
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
from tapirscan import Barcode, PixelImage, Scanner
from tapirscan._images import image_bytes
from tapirscan.results import _from_json

decode = barcode.scan

LIBS = ROOT / "build/native"
_, RAW, W, H, *_ = next(fixtures())
GRAY = np.frombuffer(RAW, dtype=np.uint8).reshape(H, W).copy()


class Images(unittest.TestCase):
    """Verify supported image inputs and immutable scan results."""

    def check_image(
        self, image: barcode.ImageInput, *, value_range: barcode.ValueRange = "auto"
    ) -> None:
        """Verify an image produces the expected typed barcode in both modes."""
        for mode in ("low", "medium", "high", "very-high"):
            reads = decode(image, mode=mode, library_dir=LIBS, value_range=value_range)
            self.assertEqual(reads.values, [TEXT])
            self.assertIsInstance(reads[0], Barcode)
            self.assertEqual(reads[0].format, "EAN13")
            self.assertGreater(reads[0].rect.width, 0)
            self.assertEqual(len(reads[0].polygon), 4)

    def test_old_native_abi(self) -> None:
        """Reject older libraries at initialization with actionable version details."""
        with patch("tapirscan.c.CDLL") as load:
            load.return_value.barcode_abi_version.return_value = 3
            with self.assertRaisesRegex(RuntimeError, "expected 4, got 3.*Rebuild"):
                Scanner(library_dir=LIBS)
            load.return_value.tapirscan_create.assert_not_called()

    def test_finish_candidates(self) -> None:
        """Continuation is opt-in and validates the selected reader."""
        image = PixelImage(RAW, width=W, height=H)
        for mode in ("low", "medium", "high", "very-high"):
            with Scanner(mode, library_dir=LIBS) as scanner:
                self.assertEqual(
                    scanner.scan(image).values,
                    scanner.scan(image, extended_budget=False).values,
                )
                self.assertEqual(
                    scanner.scan(image, extended_budget=True).values, [TEXT]
                )
                with self.assertRaisesRegex(TypeError, "a boolean"):
                    scanner.scan(image, extended_budget=1)  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]
                self.assertEqual(
                    scanner.scan(image, formats="QRCode", extended_budget=True).values,
                    [],
                )
        self.assertEqual(
            decode(image, library_dir=LIBS, extended_budget=True).values,
            [TEXT],
        )

    def test_missing_continuation_capability(self) -> None:
        """Older custom native builds fail clearly only when continuation is used."""
        with (
            Scanner(library_dir=LIBS) as scanner,
            patch.object(scanner._lib, "barcode_capabilities", return_value=0),  # noqa: SLF001
        ):
            image = PixelImage(RAW, width=W, height=H)
            self.assertEqual(scanner.scan(image).values, [TEXT])
            with self.assertRaisesRegex(
                RuntimeError, "does not support extended budget"
            ):
                scanner.scan(image, extended_budget=True)

    def test_supplement_policy(self) -> None:
        """Policy is opt-in, creation-only and forwarded by one-shot scanning."""
        with Scanner(library_dir=LIBS) as scanner:
            self.assertEqual(scanner.ean_add_on_policy, "Ignore")
        self.assertEqual(
            decode(
                PixelImage(RAW, width=W, height=H),
                library_dir=LIBS,
                ean_add_on_policy="Require",
            ).values,
            [],
        )
        with self.assertRaisesRegex(ValueError, "ean_add_on_policy"):
            Scanner(ean_add_on_policy="read")  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]

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
            self.assertEqual(scanner.scan(image).values, [TEXT])
            self.assertEqual(scanner.scan(image, formats="QRCode").values, [])
            self.assertEqual(scanner.scan(image).values, [TEXT])
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
                    scanner.scan(PixelImage(RAW, width=W, height=H)).values, [TEXT]
                )

    def test_metadata_and_work_status(self) -> None:
        """Application metadata and localization limits are visible without debug."""
        raw = decode(PixelImage(RAW, width=W, height=H), library_dir=LIBS).to_raw_dict()
        raw["localizationLimited"] = True
        raw["scan"]["unfinished"] = False
        raw["scan"]["barcodes"][0].update(
            {
                "gs1": True,
                "readerInitialization": False,
                "structuredAppend": {
                    "index": 1,
                    "count": 2,
                    "id": "group",
                    "parity": 7,
                },
                "eanAddOn": "12",
            }
        )
        for debug in (False, True):
            result = _from_json(json.dumps(raw).encode(), W, H, debug=debug)
            self.assertTrue(result.unfinished)
            self.assertTrue(result[0].gs1)
            self.assertFalse(result[0].reader_initialization)
            self.assertEqual(result[0].ean_add_on, "12")
            append = result[0].structured_append
            self.assertIsNotNone(append)
            if append is not None:
                self.assertEqual(
                    (append.index, append.count, append.id, append.parity),
                    (1, 2, "group", 7),
                )
                with self.assertRaises(FrozenInstanceError):
                    append.index = 2  # ty: ignore[invalid-assignment]
            self.assertEqual(result.debug is not None, debug)

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
            self.assertEqual(scanner.scan(GRAY).values, [TEXT])
            self.assertEqual(scanner.scan(GRAY, formats="2D").values, [])

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
        self.assertEqual(len(decode(image, library_dir=LIBS)), 2)
        self.assertIsNotNone(decode(image, library_dir=LIBS).best)
        with Scanner(library_dir=LIBS) as scanner:
            result = scanner.scan(
                torch.from_numpy(np.asarray(image).copy()), debug=True
            )
        if result.debug is None or result.debug.regions is None:
            self.fail("Region evidence was requested")
        self.assertTrue(result.debug.regions.search_windows)

    def test_unified_result(self) -> None:
        """Verify unified result."""
        result = barcode.scan(PixelImage(RAW, width=W, height=H), library_dir=LIBS)
        self.assertEqual(result.values, [TEXT])
        self.assertEqual(result[0].text, TEXT)
        self.assertGreater(result[0].support, 0)
        self.assertIsNone(result[0].structured_append)
        self.assertEqual(tuple(result), result.barcodes)
        self.assertEqual(result[:1], result.barcodes[:1])
        self.assertEqual(result.best, result[0])
        self.assertIsNone(result.debug)
        self.assertEqual(result.image, (W, H))
        self.assertEqual(result.unfinished, result.to_raw_dict()["scan"]["unfinished"])
        details = barcode.scan(
            PixelImage(RAW, width=W, height=H), library_dir=LIBS, debug=True
        )
        if details.debug is None:
            self.fail("Diagnostics were requested")
        regions = details.debug.regions
        if regions is None:
            self.fail("Region evidence was requested")
        self.assertTrue(regions.proposals)
        self.assertTrue(regions.candidates)
        if regions.search_windows is None:
            self.fail("Search windows were requested")
        self.assertEqual(regions.search_windows[0].kind, "full_frame_search")
        self.assertTrue(any(c.detections for c in regions.candidates))
        for candidate in regions.candidates:
            self.assertEqual(len(candidate.polygon), 4)
            for detection in candidate.detections:
                self.assertEqual(detection.text, TEXT)

        with self.assertRaises(FrozenInstanceError):
            result[0].text = "changed"  # ty: ignore[invalid-assignment]
        exported = result.to_raw_dict()
        exported["scan"]["barcodes"].clear()
        self.assertEqual(result.values, [TEXT])
        blank = barcode.scan(
            PixelImage(bytes([255]) * (W * H), width=W, height=H), library_dir=LIBS
        )
        self.assertFalse(blank)
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
            self.assertEqual(scanner.scan(GRAY, color_order="BGR").values, [TEXT])
        self.assertEqual(
            barcode.scan(GRAY, color_order="BGR", library_dir=LIBS).values, [TEXT]
        )
        with self.assertRaisesRegex(ValueError, "only"):
            image_bytes(Image.fromarray(GRAY), color_order="BGR")
        with self.assertRaisesRegex(ValueError, "color_order"):
            image_bytes(GRAY, color_order="typo")  # ty: ignore[invalid-argument-type]

    def test_public_serialization(self) -> None:
        """JSON export contains application fields and owns its nested collections."""
        with Scanner(library_dir=LIBS) as scanner:
            result = scanner.scan(GRAY, debug=True)
        exported = result.as_dict()
        self.assertEqual(json.loads(json.dumps(exported)), exported)
        self.assertNotIn("_json", exported)
        self.assertNotIn("scan", exported)
        self.assertNotIn("debug", exported)
        self.assertEqual(exported["image"], {"width": W, "height": H})
        self.assertEqual(exported["best"], result[0].as_dict())
        exported.clear()
        self.assertEqual(result.as_dict()["values"], [TEXT])

    def test_undecoded_region_types(self) -> None:
        """Keep undecoded geometry separate, including empty decoded payloads."""
        value = json.loads(
            '{"schemaVersion":2,"mode":"low","elapsedMs":0,'
            '"localizationLimited":false,"scan":{"unfinished":false,"barcodes":[]}}'
        )
        polygons = [
            [[x, 0], [x + 10, 0], [x + 10, 10], [x, 10]] for x in (0, 20, 40, 60)
        ]
        read = {
            "text": "",
            "format": "QRCode",
            "polygon": polygons[0],
            "support": 1,
            "candidate_indices": [0],
        }
        value["scan"]["barcodes"] = [read]
        unread = {
            "text": "",
            "format": "DataMatrix",
            "polygon": polygons[1],
            "support": 0,
        }
        value["scan"]["regions"] = [dict(read), unread]
        result = _from_json(json.dumps(value).encode(), 80, 20, debug=True)
        if result.debug is None or result.debug.regions is None:
            self.fail("Missing requested region evidence")
        regions = result.debug.regions
        self.assertEqual(len(result), 1)
        self.assertEqual(len(regions.undecoded), 1)
        region = regions.undecoded[0]
        self.assertIsInstance(region, barcode.UndecodedRegion)
        self.assertNotIsInstance(region, Barcode)
        self.assertEqual(region.format, "DataMatrix")
        self.assertEqual(region.polygon, tuple(map(tuple, polygons[1])))
        self.assertFalse(hasattr(region, "text"))
        self.assertFalse(hasattr(region, "payload_bytes"))
        self.assertFalse(hasattr(regions, "additional"))
        with self.assertRaises(FrozenInstanceError):
            region.format = "Unknown"  # ty: ignore[invalid-assignment]
        # Candidate IDs are local to each recovery crop.
        del value["scan"]["regions"]
        value["localization"] = {
            "proposals": [{"polygon": p, "score": 1} for p in polygons[:2]],
            "omitted": 0,
            "workLimited": False,
        }
        value["recovery"] = {
            "attempts": [
                {
                    "reads": [{"candidate_indices": [0]}],
                    "proposals": [{"polygon": p} for p in polygons[2:]],
                }
            ]
        }
        recovered = _from_json(json.dumps(value).encode(), 80, 20, debug=True)
        if recovered.debug is None or recovered.debug.regions is None:
            self.fail("Missing recovery evidence")
        self.assertEqual(
            [r.polygon for r in recovered.debug.regions.undecoded],
            [tuple(map(tuple, polygons[i])) for i in (1, 3)],
        )

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
                    scanner.scan(image)
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
                    scanner.scan(image)
                convert.assert_not_called()
            # The exact pixel-count boundary reaches storage validation.
            with self.assertRaisesRegex(ValueError, "buffer is too short"):
                scanner.scan(PixelImage(b"", width=width, height=height - 1))

    def test_native_error_messages(self) -> None:
        """Numeric native status remains available with descriptive error text."""
        for code, message in (
            (1, "Invalid scanner arguments"),
            (2, "handle"),
            (3, "buffer is too small"),
            (4, "Internal scanner"),
            (5, "capacity exceeded"),
            (99, "Unknown native"),
        ):
            with self.subTest(code=code):
                error = barcode.ScannerError(code)
                self.assertIsInstance(error, RuntimeError)
                self.assertEqual(error.code, code)
                self.assertIn(message, str(error))
                self.assertIn(f"code {code}", str(error))

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
                    scanner.scan(image)
            image = PixelImage(RAW, width=W, height=H)
            for options in (
                {"multiple": False},
                {"include_regions": True},
                {"debug": 1},
            ):
                with self.assertRaises(TypeError):
                    scanner.scan(image, **options)  # ty: ignore[invalid-argument-type]
            with self.assertRaises(TypeError):
                scanner.scan((RAW, W, H))  # ty: ignore[invalid-argument-type]
            for options in ({"layout": "HW"}, {"value_range": "0_255"}):
                with self.assertRaises(ValueError):
                    scanner.scan(image, **options)  # ty: ignore[invalid-argument-type]
            for name in ("data", "type", "quality", "orientation"):
                self.assertFalse(hasattr(scanner.scan(image)[0], name))
            scanner.close()
            # Closure is checked before expensive conversion or validation.
            with self.assertRaisesRegex(RuntimeError, "closed"):
                scanner.scan(PixelImage(b"", width=W, height=H))

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

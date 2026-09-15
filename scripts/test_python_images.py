#!/usr/bin/env python3
"""Image adapters and unified typed API against the actual native scanner."""

import os
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

import numpy as np
import torch
from fixture_data import TEXT, fixtures
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(
    0, os.environ.get("BARCODE_PYTHON_PACKAGE", str(ROOT / "bindings/python/src"))
)
import tapirscan as barcode
from tapirscan import Barcode, Scanner
from tapirscan._images import image_bytes
from tapirscan.pyzbar import decode as alias

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
            self.assertEqual([r.data for r in reads], [TEXT.encode()])
            self.assertIsInstance(reads[0], Barcode)
            self.assertEqual(reads[0].type, "EAN13")
            self.assertGreater(reads[0].rect.width, 0)
            self.assertEqual(len(reads[0].polygon), 4)
            self.assertIsNone(reads[0].orientation)

    def test_raw_and_pillow(self) -> None:
        """Verify raw and pillow."""
        self.check_image((RAW, W, H))
        for mode in ("L", "1", "RGB", "RGBA", "P", "CMYK", "I", "F"):
            with self.subTest(mode=mode):
                self.check_image(Image.fromarray(GRAY).convert(mode))

    def test_numpy(self) -> None:
        """Verify numpy."""

        class ArraySubclass(np.ndarray[tuple[int, ...], np.dtype[np.uint8]]):
            pass

        for a in (
            GRAY.view(ArraySubclass),
            GRAY,
            GRAY.astype(np.float32),
            np.asfortranarray(GRAY),
            GRAY.T,
            GRAY[:, ::-1],
            np.repeat(GRAY[:, :, None], 3, axis=2),
        ):
            self.check_image(a)
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
            images.extend([gray.to(dtype), gray.to(dtype) / 255])
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
        self.assertEqual(len(decode(image, multiple=False, library_dir=LIBS)), 1)
        with Scanner(library_dir=LIBS) as scanner:
            result = scanner.scan(
                torch.from_numpy(np.asarray(image).copy()), debug=True
            )
        if result.debug is None or result.debug.regions is None:
            self.fail("Region evidence was requested")
        self.assertTrue(result.debug.regions.search_windows)

    def test_unified_result(self) -> None:
        """Verify unified result."""
        result = barcode.scan((RAW, W, H), library_dir=LIBS)
        self.assertEqual(result.values, [TEXT])
        self.assertEqual(result[0].text, TEXT)
        self.assertEqual(tuple(result), result.barcodes)
        self.assertEqual(result[:1], result.barcodes[:1])
        self.assertEqual(result.best, result[0])
        self.assertIsNone(result.debug)
        self.assertEqual(result.image, (W, H))
        self.assertEqual(result.unfinished, result.to_dict()["scan"]["unfinished"])
        details = barcode.scan((RAW, W, H), library_dir=LIBS, debug=True)
        if details.debug is None:
            self.fail("Diagnostics were requested")
        regions = details.debug.regions
        if regions is None:
            self.fail("Region evidence was requested")
        self.assertTrue(regions.proposals)
        self.assertTrue(regions.candidates)
        self.assertEqual(regions.search_windows[0].kind, "full_frame_search")
        self.assertTrue(any(c.detections for c in regions.candidates))
        for candidate in regions.candidates:
            self.assertEqual(len(candidate.polygon), 4)
            for detection in candidate.detections:
                self.assertEqual(detection.text, TEXT)

        with self.assertRaises(FrozenInstanceError):
            result[0].text = "changed"  # ty: ignore[invalid-assignment]
        exported = result.to_dict()
        exported["scan"]["barcodes"].clear()
        self.assertEqual(result.values, [TEXT])
        blank = barcode.scan((bytes([255]) * (W * H), W, H), library_dir=LIBS)
        self.assertFalse(blank)
        self.assertEqual(blank.values, [])
        self.assertIsNone(blank.best)

        self.assertIs(alias, barcode.scan)

    def test_invalid(self) -> None:
        """Verify invalid."""
        for image in (
            (RAW[:-1], W, H),
            (RAW, True, H),
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

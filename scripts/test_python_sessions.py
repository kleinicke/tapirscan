"""Camera-independent session contract."""

import os
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(
    0,
    os.environ.get(
        "BARCODE_PYTHON_PACKAGE",
        str(Path(__file__).resolve().parents[1] / "bindings/python/src"),
    ),
)
from tapirscan import ScanSession


class SessionTests(unittest.TestCase):
    """Acceptance behavior shared by all high-level bindings."""

    def test_single(self) -> None:
        """Check single behavior."""
        s = ScanSession()
        self.assertFalse(s.undo())
        self.assertTrue(s.accept("QRCode", ""))
        self.assertTrue(s.finished)
        self.assertFalse(s.can_undo)
        self.assertFalse(s.accept("QRCode", "next"))
        self.assertFalse(s.undo())
        s.reset()
        self.assertFalse(s.finished)
        self.assertEqual(s.entries, ())

    def test_list(self) -> None:
        """Check list behavior."""
        s = ScanSession(mode="list")
        self.assertTrue(s.accept("QRCode", "a\0é"))
        self.assertFalse(s.accept("QRCode", "a\0é"))
        snapshot = s.entries
        self.assertTrue(s.accept("QRCode", "a\0é", repeat=True))
        self.assertEqual(snapshot[0].quantity, 1)
        self.assertEqual(s.entries[0].quantity, 2)
        with self.assertRaises(FrozenInstanceError):
            snapshot[0].quantity = 9  # ty: ignore[invalid-assignment]
        self.assertTrue(s.accept("EAN13", "a\0é"))
        self.assertTrue(s.undo())
        self.assertEqual(len(s.entries), 1)
        self.assertTrue(s.undo())
        self.assertEqual(s.entries[0].quantity, 1)
        self.assertTrue(s.undo())
        self.assertFalse(s.undo())
        for text in ("é", "e\u0301", " é"):
            self.assertTrue(s.accept("QRCode", text))
        done = s.finish()
        self.assertEqual(len(done), 3)
        self.assertFalse(s.undo())
        self.assertFalse(s.accept("QRCode", "new"))
        s.reset()
        self.assertEqual(len(done), 3)
        self.assertEqual(s.entries, ())
        self.assertEqual(s.mode, "list")
        self.assertEqual(s.finish(), ())
        self.assertTrue(s.finished)

    def test_invalid_inputs(self) -> None:
        """Check invalid inputs behavior."""
        with self.assertRaises(ValueError):
            ScanSession(mode="bad")  # ty: ignore[invalid-argument-type]
        s = ScanSession(mode="list")
        with self.assertRaises(ValueError):
            s.accept("bad", "x")  # ty: ignore[invalid-argument-type]
        with self.assertRaises(TypeError):
            s.accept("QRCode", 3)  # ty: ignore[invalid-argument-type]
        with self.assertRaises(TypeError):
            s.accept("QRCode", "x", repeat=1)  # ty: ignore[invalid-argument-type]
        self.assertEqual(s.entries, ())


if __name__ == "__main__":
    unittest.main()

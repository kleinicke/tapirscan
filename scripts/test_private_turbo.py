"""Keep private Turbo recipes out of release recording and public mode choices."""

import contextlib
import io
import os
import unittest
from unittest.mock import patch

from build_turbo import TIERS, environment
from build_wasm import arguments


class PrivateTurboTests(unittest.TestCase):
    """Private recipe inputs must be explicit and deterministic."""

    def test_recipe_clears_unselected_overrides(self) -> None:
        """A caller's previous experiment must not silently alter a recipe."""
        with patch.dict(os.environ, {"TAPIRSCAN_TURBO_MATRIX_CAP": "64"}):
            original = environment("original")
            tier = environment("16")
        self.assertNotIn("TAPIRSCAN_TURBO_MATRIX_CAP", original)
        self.assertEqual(original["TAPIRSCAN_TURBO_TIER"], "0")
        self.assertEqual(tier["TAPIRSCAN_TURBO_DIRECT_SAMPLE"], "1")
        self.assertNotIn("32", TIERS)

    def test_release_recording_rejects_private_selection(self) -> None:
        """Never record a private Low-slot scanner as an ordinary release."""
        with (
            patch.dict(os.environ, {"TAPIRSCAN_EXPERIMENTAL_TURBO": "1"}),
            patch("sys.argv", ["build_wasm.py", "--record", "low"]),
            contextlib.redirect_stderr(io.StringIO()),
            self.assertRaises(SystemExit) as caught,
        ):
            arguments()
        self.assertEqual(caught.exception.code, 2)

    def test_development_accepts_private_selection(self) -> None:
        """Explicit development artifacts remain buildable."""
        with (
            patch.dict(os.environ, {"TAPIRSCAN_EXPERIMENTAL_TURBO": "1"}),
            patch("sys.argv", ["build_wasm.py", "--development", "low"]),
        ):
            args = arguments()
        self.assertTrue(args.development)
        self.assertEqual(args.modes, ["low"])


if __name__ == "__main__":
    unittest.main()

"""Keep private Turbo recipes out of package builds and public mode choices."""

import contextlib
import io
import os
import unittest
from unittest.mock import patch

from build_native import cargo_env, rustflags
from build_turbo import TIERS, environment
from build_wasm import arguments

from build import ROOT, clean_env

STRAY = {
    "TAPIRSCAN_TURBO_TILE_CACHE": "1",
    "TAPIRSCAN_EXPERIMENTAL_TURBO": "1",
}


class PrivateTurboTests(unittest.TestCase):
    """Private recipe inputs must be explicit and deterministic."""

    def test_recipe_clears_unselected_overrides(self) -> None:
        """A caller's previous experiment must not silently alter a recipe."""
        with patch.dict(
            os.environ,
            {"TAPIRSCAN_TURBO_MATRIX_CAP": "64"},
        ):
            tier = environment("16")
        self.assertNotIn("TAPIRSCAN_TURBO_MATRIX_CAP", tier)
        self.assertEqual(tier["TAPIRSCAN_TURBO_TIER"], "16")
        self.assertEqual(tier["TAPIRSCAN_TURBO_DIRECT_SAMPLE"], "1")
        self.assertNotIn("32", TIERS)

    def test_every_preset_caches_tiles(self) -> None:
        """All four Turbo presets build with the tile cache."""
        for preset in ("2", "4", "8", "16"):
            self.assertEqual(environment(preset)["TAPIRSCAN_TURBO_TILE_CACHE"], "1")

    def test_unknown_preset_is_rejected(self) -> None:
        """Internal callers cannot silently build an unspecified recipe."""
        for preset in ("0", "32", "low"):
            with self.subTest(preset=preset), self.assertRaises(ValueError):
                environment(preset)

    def test_ordinary_builds_drop_stray_switches(self) -> None:
        """A switch left in the shell never reaches a normal native or core build."""
        with patch.dict(os.environ, STRAY):
            self.assertFalse(set(STRAY) & set(clean_env()))
            self.assertFalse(set(STRAY) & set(cargo_env(ROOT / "build/x")))
            android = cargo_env(ROOT / "build/x", "aarch64-linux-android")
            self.assertFalse(set(STRAY) & set(android))
            turbo = cargo_env(ROOT / "build/x", turbo="8")
        self.assertEqual(turbo["TAPIRSCAN_TURBO_PROFILE_CAP"], "576")

    def test_caller_rustflags_reach_host_builds_only(self) -> None:
        """Host-only flags such as target-cpu=native stay out of cross builds."""
        native = ["-C", "target-cpu=native"]
        with patch.dict(os.environ, {"RUSTFLAGS": " ".join(native)}):
            os.environ.pop("CARGO_ENCODED_RUSTFLAGS", None)
            self.assertEqual(rustflags()[-2:], native)
            self.assertEqual(rustflags("aarch64-apple-ios"), [])
            self.assertEqual(
                rustflags("x86_64-linux-android"), ["-C", "target-cpu=x86-64-v2"]
            )

    def test_package_build_rejects_private_selection(self) -> None:
        """Never build a private Low-slot scanner into the package assets."""
        with (
            patch.dict(os.environ, {"TAPIRSCAN_EXPERIMENTAL_TURBO": "1"}),
            patch("sys.argv", ["build_wasm.py", "low"]),
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

"""Exercise research rejection while protecting intentional Turbo development."""

import tempfile
import unittest
from pathlib import Path

from check_repository_boundary import violations


class Boundary(unittest.TestCase):
    """Test actual filesystem additions, independent of the Git index."""

    def test_turbo_and_selected_recovery_are_allowed(self) -> None:
        """Experimental naming must not remove supported Turbo or recovery code."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in (
                "docs/EXPERIMENTAL_TURBO.md",
                "core/src/subpixel.rs",
                "core/src/experiment.rs",
                "core/src/lowres.rs",
            ):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            self.assertEqual(violations(root), [])

    def test_archive_report_and_unused_module_are_rejected(self) -> None:
        """Untracked research files cannot enter a normal verified build."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in (
                "historical/core/src/lib.rs",
                "docs/RECOVERY_20260928.md",
                "core/src/neural_input.rs",
            ):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.touch()
            self.assertEqual(
                violations(root),
                ["core/src/neural_input.rs", "docs/RECOVERY_20260928.md", "historical"],
            )


if __name__ == "__main__":
    unittest.main()

"""Package updates preserve Cargo inputs and reject competing consumers."""

import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from build_wasm import ROOT, source_files
from prepare_rust import prepared_package, sync_tree


class Preparation(unittest.TestCase):
    """Exercise update, deletion, interrupted generation and build locking."""

    def test_mode_selection_participates_in_wasm_identity(self) -> None:
        """Mode order determines adapter IDs and must invalidate source identity."""
        before = source_files()
        read = Path.read_bytes
        selection = ROOT / "provenance/modes.json"

        def changed(path: Path) -> bytes:
            data = read(path)
            return data + b" " if path == selection else data

        with patch.object(Path, "read_bytes", changed):
            after = source_files()
        self.assertNotEqual(
            before["provenance/modes.json"], after["provenance/modes.json"]
        )

    def test_refresh_keeps_mtime_and_removes_stale_files(self) -> None:
        """Unchanged inputs keep timestamps; deleted modules do not survive."""
        with tempfile.TemporaryDirectory() as directory:
            source, target = (Path(directory) / name for name in ("source", "target"))
            source.mkdir()
            (source / "module.rs").write_text("original")
            sync_tree(source, target)
            stamp = (target / "module.rs").stat().st_mtime_ns
            (target / "stale.rs").write_text("stale")
            sync_tree(source, target)
            self.assertEqual((target / "module.rs").stat().st_mtime_ns, stamp)
            self.assertFalse((target / "stale.rs").exists())
            (source / "module.rs").write_text("changed")
            sync_tree(source, target)
            self.assertEqual((target / "module.rs").read_text(), "changed")

    def test_build_owns_lock_and_failed_generation_preserves_package(self) -> None:
        """The lock survives preparation until the consumer exits, then releases."""
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "package"
            target.mkdir()
            (target / "Cargo.toml").write_text("original")

            def assemble(staged: Path) -> None:
                (staged / "Cargo.toml").write_text("updated")

            with (
                patch("prepare_rust.subprocess.run"),
                patch("prepare_rust.assemble", side_effect=assemble),
            ):
                with (
                    prepared_package(target, refresh=True),
                    self.assertRaisesRegex(RuntimeError, "Package is in use"),
                    prepared_package(target, refresh=True),
                ):
                    self.fail("second consumer acquired lock")
                self.assertFalse(target.with_name("package.lock").exists())
            with (
                patch("prepare_rust.subprocess.run"),
                patch("prepare_rust.assemble", side_effect=ValueError("bad source")),
                self.assertRaisesRegex(ValueError, "bad source"),
                prepared_package(target, refresh=True),
            ):
                self.fail("invalid source was installed")
            self.assertEqual((target / "Cargo.toml").read_text(), "updated")
            self.assertFalse(target.with_name("package.lock").exists())


if __name__ == "__main__":
    unittest.main()

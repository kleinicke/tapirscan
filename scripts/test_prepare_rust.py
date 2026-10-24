"""Package updates preserve Cargo inputs and reject competing consumers."""

import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from build_wasm import ROOT, source_files
from prepare_rust import exclusive, prepared_package, sync_tree, try_lock, unlock


class Preparation(unittest.TestCase):
    """Exercise update, deletion, interrupted generation and build locking."""

    def test_mode_selection_participates_in_wasm_identity(self) -> None:
        """Mode order determines adapter IDs and must invalidate source identity."""
        before = source_files()
        read = Path.read_bytes
        selection = ROOT / "config/modes.json"

        def changed(path: Path) -> bytes:
            data = read(path)
            return data + b" " if path == selection else data

        with patch.object(Path, "read_bytes", changed):
            after = source_files()
        self.assertNotEqual(before["config/modes.json"], after["config/modes.json"])

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
            lock = target.with_name("package.build-lock")

            def assemble(staged: Path) -> None:
                (staged / "Cargo.toml").write_text("updated")

            with (
                patch("prepare_rust.subprocess.run"),
                patch("prepare_rust.assemble", side_effect=assemble),
                prepared_package(target, refresh=True),
                self.assertRaisesRegex(RuntimeError, "already holds"),
                prepared_package(target, refresh=True),
            ):
                self.fail("second consumer acquired lock")
            self.assertTrue(self.available(lock))
            with (
                patch("prepare_rust.subprocess.run"),
                patch("prepare_rust.assemble", side_effect=ValueError("bad source")),
                self.assertRaisesRegex(ValueError, "bad source"),
                prepared_package(target, refresh=True),
            ):
                self.fail("invalid source was installed")
            self.assertEqual((target / "Cargo.toml").read_text(), "updated")
            self.assertTrue(self.available(lock))

    def test_other_process_holds_lock_until_it_dies(self) -> None:
        """A live holder excludes others; killing it releases the lock."""
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "package.build-lock"
            script = (
                "import sys, time\n"
                f"sys.path.insert(0, {str(Path(__file__).parent)!r})\n"
                "from pathlib import Path\n"
                "from prepare_rust import exclusive\n"
                f"with exclusive(Path({str(lock)!r})):\n"
                "    print('held', flush=True)\n"
                "    time.sleep(60)\n"
            )
            holder = subprocess.Popen(
                [sys.executable, "-c", script], stdout=subprocess.PIPE, text=True
            )
            try:
                self.assertEqual(holder.stdout and holder.stdout.readline(), "held\n")
                self.assertFalse(self.available(lock))
            finally:
                holder.kill()
                holder.wait()
                if holder.stdout:
                    holder.stdout.close()
            with (
                patch("prepare_rust.time.sleep", side_effect=AssertionError),
                exclusive(lock),
            ):
                self.assertFalse(self.available(lock))

    def test_waiter_reports_and_acquires_after_release(self) -> None:
        """A waiting build says so and proceeds once the holder releases."""
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "package.build-lock"
            with lock.open("a+b") as handle:
                self.assertTrue(try_lock(handle))
                waits = []

                def release(_: float) -> None:
                    waits.append(time.monotonic())
                    unlock(handle)

                with (
                    patch("prepare_rust.time.sleep", side_effect=release),
                    patch("sys.stderr") as stderr,
                    exclusive(lock),
                ):
                    self.assertEqual(len(waits), 1)
                    self.assertIn("Waiting for another build", str(stderr.mock_calls))

    @staticmethod
    def available(lock: Path) -> bool:
        """Probe the lock from a separate process, as another build would."""
        probe = (
            "import sys\n"
            f"sys.path.insert(0, {str(Path(__file__).parent)!r})\n"
            "from prepare_rust import try_lock\n"
            f"with open({str(lock)!r}, 'a+b') as handle:\n"
            "    sys.exit(0 if try_lock(handle) else 1)\n"
        )
        return (
            subprocess.run([sys.executable, "-c", probe], check=False).returncode == 0
        )


if __name__ == "__main__":
    unittest.main()

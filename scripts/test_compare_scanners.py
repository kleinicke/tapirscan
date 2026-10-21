"""Regression checks for comparison evidence and failure detection."""

import hashlib
import json
import subprocess
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path

from compare_scanners import (
    NativeComparison,
    checkout,
    clean,
    distribution,
    timing_indices,
    variants,
)


class ComparisonEvidence(unittest.TestCase):
    """Keep meaningful result differences visible and measurements deterministic."""

    def test_checkout_records_symlinks_without_reading_targets(self) -> None:
        """Directory and dangling links are source entries, not input byte streams."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            repo = root / "repo"
            repo.mkdir()
            hooks = root / "empty-hooks"
            hooks.mkdir()
            subprocess.run(["git", "init", "-q", str(repo)], check=True)
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repo),
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    f"core.hooksPath={hooks}",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "fixture",
                ],
                check=True,
            )
            target = root / "target"
            target.mkdir()
            payload = target / "payload"
            payload.write_bytes(b"outside checkout")
            (repo / "plain").write_bytes(b"source bytes")
            (repo / "directory").symlink_to("../target", target_is_directory=True)
            (repo / "file-link").symlink_to("../target/payload")
            (repo / "dangling").symlink_to("../missing")
            before = checkout(repo)
            self.assertTrue(before["dirty"])
            self.assertEqual(
                before["untracked"],
                {
                    "plain": hashlib.sha256(b"source bytes").hexdigest(),
                    "directory": {"symlink": "../target"},
                    "file-link": {"symlink": "../target/payload"},
                    "dangling": {"symlink": "../missing"},
                },
            )
            payload.write_bytes(b"changed external bytes")
            self.assertEqual(checkout(repo), before)
            (repo / "file-link").unlink()
            (repo / "file-link").symlink_to("../target/other")
            self.assertNotEqual(checkout(repo)["untracked"], before["untracked"])

    def test_only_timing_is_ignored(self) -> None:
        """Geometry, ordering, metadata and work counters remain comparison inputs."""
        value = {"elapsedMs": 10, "reads": [{"text": "a", "support": 2}], "work": 5}
        expected = {"reads": [{"text": "a", "support": 2}], "work": 5}
        self.assertEqual(clean(value), expected)
        self.assertNotEqual(clean(value), {**expected, "work": 6})
        self.assertNotEqual(
            clean(value), {**expected, "reads": [{"text": "a", "support": 3}]}
        )

    def test_bounded_retention_does_not_hide_failure_count(self) -> None:
        """All mismatches count even after the diagnostic retention cap is reached."""
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            with ExitStack() as stack:
                comparison = NativeComparison({"maxDifferences": 1}, output, stack)
                comparison.compare({"text": "a"}, {"text": "b"}, {"case": "first"})
                comparison.compare([1, 2], [2, 1], {"case": "second"})
                comparison.compare(None, None, {"case": "equal"})
                self.assertEqual(comparison.differing, 2)
            rows = (output / "differences.jsonl").read_text().splitlines()
            self.assertEqual(len(rows), 1)
            self.assertEqual(json.loads(rows[0])["case"], "first")

    def test_timing_cohort_and_percentiles(self) -> None:
        """Small and empty timing cohorts behave predictably without duplication."""
        self.assertEqual(timing_indices(5, 20), set(range(5)))
        self.assertEqual(timing_indices(5, 2), {0, 2})
        self.assertEqual(timing_indices(5, 0), set())
        self.assertEqual(distribution([3, 1, 2])["medianMs"], 2)
        self.assertEqual(distribution([3, 1, 2])["p95Ms"], 3)
        with self.assertRaises(ValueError):
            distribution([])

    def test_fixture_formats_override_image_presets(self) -> None:
        """Explicit fixtures run once per selection, not per preset."""
        self.assertEqual(
            variants(
                {"formats": "QRCode"},
                {"formats": ["EAN13", "retail"]},
            ),
            [('"QRCode"', "QRCode")],
        )


if __name__ == "__main__":
    unittest.main()

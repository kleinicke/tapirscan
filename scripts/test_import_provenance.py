"""Verify that a release revision cannot hide imported-source or patch drift."""

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def digest(data: bytes) -> str:
    """Return the exact content hash used by source provenance."""
    return hashlib.sha256(data).hexdigest()


class Provenance(unittest.TestCase):
    """Exercise verification on isolated, small imported snapshots."""

    def test_editable_production_and_frozen_history(self) -> None:
        """Permit development edits; keep history and release checks strict."""
        with tempfile.TemporaryDirectory(prefix="tapirscan-history-test-") as tmp:
            root = Path(tmp)
            for directory in (
                "scripts",
                "core/src",
                "historical/core/src",
                "provenance",
            ):
                (root / directory).mkdir(parents=True, exist_ok=True)
            (root / "scripts/verify_import.py").write_bytes(
                (ROOT / "scripts/verify_import.py").read_bytes()
            )
            archived = root / "historical/core/src/lib.rs"
            current = root / "core/src/lib.rs"
            archived.write_bytes(b"history")
            current.write_bytes(b"production")
            (root / "provenance/import.json").write_text(
                json.dumps({"files": {"core/src/lib.rs": digest(b"history")}})
            )
            (root / "provenance/modes.json").write_text(
                json.dumps({"runtimeRevision": "runtime.json"})
            )
            (root / "runtime.json").write_text(
                json.dumps({"files": {"core/src/lib.rs": digest(b"production")}})
            )
            command = [sys.executable, str(root / "scripts/verify_import.py")]
            for changed, historical_only, expected in (
                (False, False, True),
                (True, False, False),
                (True, True, True),
            ):
                current.write_bytes(b"experiment" if changed else b"production")
                result = subprocess.run(
                    command + (["--historical-only"] if historical_only else []),
                    capture_output=True,
                    check=False,
                )
                self.assertEqual(result.returncode == 0, expected, result.stderr)
            archived.write_bytes(b"tampered")
            result = subprocess.run(
                [*command, "--historical-only"], capture_output=True, check=False
            )
            self.assertNotEqual(result.returncode, 0)

    def test_reversible_revision_and_tampering(self) -> None:
        """Accept exact revisions and reject drift in every recorded boundary."""
        with tempfile.TemporaryDirectory(prefix="tapirscan-import-test-") as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            (root / "scripts/verify_import.py").write_bytes(
                (ROOT / "scripts/verify_import.py").read_bytes()
            )
            patch = (
                b"--- a/source.txt\n+++ b/source.txt\n@@ -1 +1 @@\n-before\n+after\n"
            )
            revision = {
                "patch": "change.patch",
                "patchSha256": digest(patch),
                "baseHashes": {"source.txt": digest(b"before\n")},
                "targetHashes": {"source.txt": digest(b"after\n")},
            }
            (root / "provenance").mkdir()
            (root / "provenance/import.json").write_text(
                json.dumps(
                    {
                        "files": {"source.txt": digest(b"before\n")},
                        "releaseRevision": "revision.json",
                    }
                )
            )
            (root / "provenance/modes.json").write_text(
                json.dumps({"runtimeRevision": "runtime.json"})
            )
            for mutation in ("none", "source", "patch", "base", "target", "runtime"):
                with self.subTest(mutation=mutation):
                    current = json.loads(json.dumps(revision))
                    (root / "runtime.txt").write_bytes(b"runtime\n")
                    (root / "runtime.json").write_text(
                        json.dumps({"files": {"runtime.txt": digest(b"runtime\n")}})
                    )
                    (root / "source.txt").write_bytes(b"after\n")
                    (root / "change.patch").write_bytes(patch)
                    if mutation == "source":
                        (root / "source.txt").write_bytes(b"drift\n")
                    elif mutation == "patch":
                        (root / "change.patch").write_bytes(patch + b"drift\n")
                    elif mutation == "base":
                        current["baseHashes"]["source.txt"] = digest(b"wrong\n")
                    elif mutation == "runtime":
                        (root / "runtime.txt").write_bytes(b"drift\n")
                    elif mutation == "target":
                        current["targetHashes"]["source.txt"] = digest(b"wrong\n")
                    (root / "revision.json").write_text(json.dumps(current))
                    result = subprocess.run(
                        [sys.executable, str(root / "scripts/verify_import.py")],
                        capture_output=True,
                        text=True,
                        check=False,
                    )
                    self.assertEqual(
                        result.returncode == 0, mutation == "none", result.stderr
                    )


if __name__ == "__main__":
    unittest.main()

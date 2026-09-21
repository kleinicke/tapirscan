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

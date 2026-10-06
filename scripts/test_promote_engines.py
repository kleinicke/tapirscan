"""Engine promotion must never reuse a recorded or published engine name."""

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from promote_engines import collisions, current_suffix, engine_names

SELECTION = {
    "modes": [{"mode": "low", "tag": "low-old-20260101"}],
    "experimentalTurbo": [{"preset": 2, "tag": "experimental-turbo2-old-20260101"}],
}


class Promotion(unittest.TestCase):
    """Collision checks over manifests, demo registry and local assets."""

    def root(self, tmp: str) -> Path:
        """Create a minimal repository layout with one recorded name per source."""
        root = Path(tmp)
        (root / "bindings/javascript/wasm").mkdir(parents=True)
        (root / "provenance").mkdir()
        (root / "demo/src/lib").mkdir(parents=True)
        (root / "bindings/javascript/wasm/low-asset-20260102.wasm").write_bytes(b"")
        (root / "provenance/wasm-old.json").write_text(
            json.dumps({"modes": [{"file": "low-recorded-20260103.wasm"}]})
        )
        (root / "demo/src/lib/scanner-versions.json").write_text(
            json.dumps({"versions": [{"modes": [{"file": "low-demo-20260104.wasm"}]}]})
        )
        return root

    def test_names_and_suffix(self) -> None:
        """Every mode and Turbo preset is renamed with one shared suffix."""
        self.assertEqual(current_suffix(SELECTION), "old-20260101")
        self.assertEqual(
            engine_names(SELECTION, "new-20260105"),
            ["low-new-20260105.wasm", "experimental-turbo2-new-20260105.wasm"],
        )

    def test_used_names_are_refused(self) -> None:
        """Local assets, recorded manifests and demo registry entries block a tag."""
        with tempfile.TemporaryDirectory() as tmp:
            root = self.root(tmp)
            for tag in ("asset-20260102", "recorded-20260103", "demo-20260104"):
                self.assertTrue(collisions(root, SELECTION, tag), tag)
            self.assertEqual(collisions(root, SELECTION, "new-20260105"), [])
            (root / "provenance/new-20260105.json").write_text("{}")
            self.assertTrue(collisions(root, SELECTION, "new-20260105"))


if __name__ == "__main__":
    unittest.main()

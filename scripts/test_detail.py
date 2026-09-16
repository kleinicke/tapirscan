"""Generated small/rotated barcode checks for the full source-detail pipeline."""

import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

from fixture_data import fixtures
from PIL import Image
from test_bindings import LIBS, ROOT, PixelImage, Scanner, run

PAIR_ANGLE = 43
FIXTURE_ARG_COUNT = 3


def make_fixtures(destination: Path) -> list[dict[str, Any]]:
    """Generate image pixels only in the caller's temporary directory."""
    destination.mkdir(parents=True, exist_ok=True)
    _, pixels, width, height, *_ = next(fixtures())
    base = Image.frombytes("L", (width, height), pixels)
    manifest = []
    for scale in (0.25, 0.35, 0.5):
        for angle in (0, 17, 43, 90):
            tile = base.resize(
                (round(width * scale), round(height * scale)), Image.Resampling.LANCZOS
            ).rotate(
                angle, resample=Image.Resampling.BICUBIC, expand=True, fillcolor=255
            )
            image = Image.new("RGBA", (1600, 1100), "white")
            image.paste(tile, (347, 261))
            if angle == PAIR_ANGLE:
                image.paste(tile, (1100, 700))
            name = f"scale-{scale}-angle-{angle}"
            (destination / f"{name}.rgba").write_bytes(image.tobytes())
            manifest.append(
                {"name": name, "width": image.width, "height": image.height}
            )
    (destination / "manifest.json").write_text(json.dumps(manifest))
    return manifest


def normalized_reads(value: dict[str, Any]) -> list[dict[str, Any]]:
    """Allow only tiny native/libm coordinate rounding; values/support stay exact."""
    return [
        {
            "text": b["text"],
            "support": b["support"],
            "polygon": [[round(n, 5) for n in point] for point in b["polygon"]],
        }
        for b in value["scan"]["barcodes"]
    ]


class Detail(unittest.TestCase):
    """Exercise real additional reads, not merely successful library loading."""

    def test_recovery_native_wasm_parity(self) -> None:
        """Keep recovery, rotation, source geometry, and distinct copies aligned."""
        recovered = 0
        with tempfile.TemporaryDirectory(prefix="tapirscan-detail-") as temp:
            destination = Path(temp)
            manifest = make_fixtures(destination)
            for mode in ("medium", "high", "very-high"):
                with Scanner(mode, library_dir=LIBS) as scanner:
                    for fixture in manifest:
                        with self.subTest(mode=mode, image=fixture["name"]):
                            width, height = fixture["width"], fixture["height"]
                            path = destination / f"{fixture['name']}.rgba"
                            native = scanner.scan(
                                PixelImage(
                                    path.read_bytes(),
                                    width=width,
                                    height=height,
                                    channels=4,
                                    stride=width * 4,
                                ),
                                debug=True,
                            ).to_raw_dict()
                            wasm = run(
                                "node",
                                ROOT / "bindings/javascript/test/native_parity.mjs",
                                mode,
                                width,
                                height,
                                4,
                                width * 4,
                                path,
                                1,
                                1,
                            )
                            self.assertEqual(
                                normalized_reads(native), normalized_reads(wasm)
                            )
                            self.assertTrue(native["scan"]["unfinished"])
                            self.assertEqual(
                                len(native["recovery"]["attempts"]),
                                len(wasm["recovery"]["attempts"]),
                            )
                            recovered += len(native["recovery"]["additions"])
            self.assertGreater(recovered, 0, "Fixtures must exercise actual recovery")


if __name__ == "__main__":
    if len(sys.argv) == FIXTURE_ARG_COUNT and sys.argv[1] == "--fixtures":
        make_fixtures(Path(sys.argv[2]))
    else:
        unittest.main()

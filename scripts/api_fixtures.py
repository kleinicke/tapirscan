"""Generate public-API fixtures with a test-only Zint encoder, never a decoder."""

import argparse
import json
import os
import subprocess
from pathlib import Path
from typing import Any

from PIL import Image


def work_limit_fixtures(destination: Path) -> list[dict[str, Any]]:
    """Generate an overlong parse workload and a blank control, without an encoder."""
    # Code128 start B (104), 256 copies of A (33), checksum 52, stop.
    # Deliberately exceeds the reader's parse cap; it is a work-limit regression.
    widths = [10, *map(int, "211214" + "111323" * 256 + "213311" + "2331112"), 10]
    row = b"".join(
        bytes([255 if i % 2 == 0 else 0]) * (width * 2)
        for i, width in enumerate(widths)
    )
    white = bytes([255]) * len(row)
    (destination / "parse-cap.raw").write_bytes(white * 20 + row * 40 + white * 20)
    (destination / "blank.raw").write_bytes(bytes([255]) * 80 * 60)
    return [
        {
            "name": "parse-cap",
            "file": "parse-cap.raw",
            "width": len(row),
            "height": 80,
            "formats": "Code128",
            "expected": [],
            "unfinished": True,
        },
        {
            "name": "blank",
            "file": "blank.raw",
            "width": 80,
            "height": 60,
            "formats": "Code128",
            "expected": [],
            "unfinished": False,
        },
    ]


def generate(destination: Path, encoder: str) -> Path:
    """Write decoded pixels and expectations independently of scanner output."""
    destination.mkdir(parents=True, exist_ok=True)
    cases = [
        (
            "gs1",
            "Code128",
            16,
            "[01]09506000134352[10]LOT42",
            ["--gs1"],
            [{"text": "010950600013435210LOT42", "gs1": True}],
        ),
        (
            "plain",
            "Code128",
            20,
            "Tapir plain",
            [],
            [{"text": "Tapir plain", "gs1": False}],
        ),
        (
            "part-one",
            "QRCode",
            58,
            "Tapir part one",
            ["--structapp=1,2,73"],
            [
                {
                    "text": "Tapir part one",
                    "gs1": False,
                    "structuredAppend": {"index": 1, "count": 2, "parity": 73},
                }
            ],
        ),
        (
            "part-two",
            "QRCode",
            58,
            "Tapir part two",
            ["--structapp=2,2,73"],
            [
                {
                    "text": "Tapir part two",
                    "gs1": False,
                    "structuredAppend": {"index": 2, "count": 2, "parity": 73},
                }
            ],
        ),
    ]
    for name, fmt, symbology in (
        ("binary-qr", "QRCode", 58),
        ("binary-dm", "DataMatrix", 71),
        ("binary-aztec", "Aztec", 92),
        ("binary-pdf", "PDF417", 55),
        ("binary-maxi", "MaxiCode", 57),
    ):
        cases.append(
            (
                name,
                fmt,
                symbology,
                r"\x00\x80\xFFTapir",
                ["--binary", "--esc", "--eci=3"],
                [
                    {
                        "text": "\x00\x80\xffTapir",
                        "gs1": False,
                        "payloadBytes": [0, 128, 255, 84, 97, 112, 105, 114],
                    }
                ],
            )
        )
    manifest = []
    for name, fmt, symbology, text, options, expected in cases:
        bitmap = destination / f"{name}.bmp"
        subprocess.run(
            [
                encoder,
                "-b",
                str(symbology),
                "-d",
                text,
                "--notext",
                "--quietzones",
                "--scale=3",
                "-o",
                str(bitmap),
                *options,
            ],
            check=True,
        )
        with Image.open(bitmap) as image:
            gray = Image.new("L", (image.width + 40, image.height + 40), 255)
            gray.paste(image.convert("L"), (20, 20))
            (destination / f"{name}.raw").write_bytes(gray.tobytes())
            manifest.append(
                {
                    "name": name,
                    "file": f"{name}.raw",
                    "width": gray.width,
                    "height": gray.height,
                    "formats": fmt,
                    "expected": expected,
                }
            )
    # Separate physical copies and multipart payloads must remain separate reads.
    with (
        Image.open(destination / "part-one.bmp") as one,
        Image.open(destination / "part-two.bmp") as two,
    ):
        scene = Image.new(
            "L", (one.width * 3 + 80, max(one.height, two.height) + 40), 255
        )
        for i, tile in enumerate((one, one, two)):
            scene.paste(tile.convert("L"), (20 + i * (one.width + 20), 20))
        (destination / "multipart.raw").write_bytes(scene.tobytes())
        manifest.append(
            {
                "name": "multipart",
                "file": "multipart.raw",
                "width": scene.width,
                "height": scene.height,
                "formats": "QRCode",
                "expected": [
                    *manifest[2]["expected"],
                    *manifest[2]["expected"],
                    *manifest[3]["expected"],
                ],
            }
        )
    manifest.extend(work_limit_fixtures(destination))
    path = destination / "manifest.json"
    path.write_text(json.dumps(manifest, indent=2) + "\n")
    return path


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--encoder", default=os.environ.get("ZINT", "zint"))
    args = parser.parse_args()
    print(generate(args.destination, args.encoder))

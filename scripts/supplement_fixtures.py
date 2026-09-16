"""Encode known EAN/UPC supplements and policy expectations, without a decoder."""

import argparse
import json
import os
import subprocess
from pathlib import Path
from typing import Any

from PIL import Image, ImageOps


def render(encoder: str, symbol: int, text: str, path: Path) -> Image.Image:
    """Create a padded grayscale fixture using the test-only Zint encoder."""
    subprocess.run(
        [
            encoder,
            "-b",
            str(symbol),
            "-d",
            text,
            "--notext",
            "--quietzones",
            "--scale=2",
            "--height=30",
            "-o",
            str(path),
        ],
        check=True,
    )
    with Image.open(path) as bitmap:
        gray = Image.new("L", (bitmap.width + 40, bitmap.height + 40), 255)
        gray.paste(bitmap.convert("L"), (20, 20))
    return gray


def pair_cases(
    destination: Path, name: str, suffixes: tuple[str, str]
) -> list[dict[str, Any]]:
    """Check physical association when main payloads are identical."""
    tiles = [
        Image.open(destination / f"EAN13-{suffix}.png").convert("L")
        for suffix in suffixes
    ]
    image = Image.new(
        "L",
        (sum(tile.width for tile in tiles) + 80, max(tile.height for tile in tiles)),
        255,
    )
    offset = 0
    locations = []
    for suffix, tile in zip(suffixes, tiles, strict=True):
        image.paste(tile, (offset, 0))
        locations.append(
            {
                "left": offset + 64,
                "right": offset + 444,
                "eanAddOn": None if suffix == "none" else suffix,
            }
        )
        offset += tile.width + 80
    (destination / f"{name}.raw").write_bytes(image.tobytes())
    fixtures = []
    for policy in ("Ignore", "Read", "Require"):
        geometry = [
            dict(item, eanAddOn=None if policy == "Ignore" else item["eanAddOn"])
            for item in locations
            if policy != "Require" or item["eanAddOn"]
        ]
        fixtures.append(
            {
                "name": f"{name}-{policy}",
                "file": f"{name}.raw",
                "width": image.width,
                "height": image.height,
                "formats": "EAN13",
                "eanAddOnPolicy": policy,
                "geometry": geometry,
                "expected": [
                    {"text": "4006381333931", "eanAddOn": g["eanAddOn"]}
                    for g in geometry
                ],
            }
        )
    return fixtures


def generate(destination: Path, encoder: str) -> Path:
    """Write pixels with independent text and supplement expectations."""
    destination.mkdir(parents=True, exist_ok=True)
    fixtures: list[dict[str, Any]] = []
    images = {}
    for fmt, symbol, text in (
        ("EAN13", 15, "4006381333931"),
        ("UPCA", 34, "012345678905"),
        ("EAN8", 10, "96385074"),
        ("UPCE", 37, "01234565"),
    ):
        base = render(encoder, symbol, text, destination / f"{fmt}-base.bmp")
        bounds = ImageOps.invert(base).getbbox()
        if bounds is None:
            msg = "Encoder produced a blank fixture"
            raise RuntimeError(msg)
        for suffix in ("", "12", "51234", "erased"):
            image = render(
                encoder,
                symbol,
                text
                + ("+51234" if suffix == "erased" else "+" + suffix if suffix else ""),
                destination / f"{fmt}-{suffix}.bmp",
            )
            if suffix == "erased":
                image.paste(255, (bounds[2], 0, image.width, image.height))
            name = f"{fmt}-{suffix or 'none'}"
            image.save(destination / f"{name}.png")
            (destination / f"{name}.raw").write_bytes(image.tobytes())
            if fmt == "EAN13" and not suffix:
                images["retail"] = image
            addon = suffix if suffix in ("12", "51234") else None
            for policy in ("Ignore", "Read", "Require"):
                expected = (
                    []
                    if policy == "Require" and addon is None
                    else [
                        {
                            "text": text,
                            "eanAddOn": addon if policy != "Ignore" else None,
                        }
                    ]
                )
                fixtures.append(
                    {
                        "name": f"{name}-{policy}",
                        "file": f"{name}.raw",
                        "width": image.width,
                        "height": image.height,
                        "formats": fmt,
                        "eanAddOnPolicy": policy,
                        "expected": expected,
                        "expectUnread": policy == "Require" and addon is None,
                        "geometry": [
                            {
                                "left": bounds[0],
                                "right": bounds[2],
                                "eanAddOn": expected[0]["eanAddOn"],
                            }
                        ]
                        if expected
                        else [],
                    }
                )
    qr = render(encoder, 58, "Keep this QR", destination / "qr.bmp")
    retail = images["retail"]
    mixed = Image.new(
        "L", (retail.width + qr.width + 40, max(retail.height, qr.height)), 255
    )
    mixed.paste(retail, (0, 0))
    mixed.paste(qr, (retail.width + 40, 0))
    (destination / "mixed.raw").write_bytes(mixed.tobytes())
    for policy in ("Ignore", "Read", "Require"):
        expected = [{"text": "Keep this QR", "format": "QRCode", "eanAddOn": None}]
        if policy != "Require":
            expected.append(
                {"text": "4006381333931", "format": "EAN13", "eanAddOn": None}
            )
        fixtures.append(
            {
                "name": f"mixed-{policy}",
                "file": "mixed.raw",
                "width": mixed.width,
                "height": mixed.height,
                "formats": ["EAN13", "QRCode"],
                "eanAddOnPolicy": policy,
                "expected": expected,
            }
        )
    for name, suffixes in (
        ("repeated", ("none", "12")),
        ("different-supplements", ("12", "51234")),
        ("reversed-supplements", ("51234", "12")),
    ):
        fixtures.extend(pair_cases(destination, name, suffixes))
    path = destination / "manifest.json"
    path.write_text(json.dumps(fixtures, indent=2) + "\n")
    return path


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--encoder", default=os.environ.get("ZINT", "zint"))
    args = parser.parse_args()
    print(generate(args.destination, args.encoder))

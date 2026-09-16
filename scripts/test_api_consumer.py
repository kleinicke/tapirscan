"""Exercise an installed public Python API using generated pixel fixtures."""

import argparse
import json
import unittest
from pathlib import Path
from typing import Any

import tapirscan


def metadata(read: tapirscan.Barcode, case: dict[str, Any]) -> dict[str, Any]:
    """Select independently expected metadata from one decoded result."""
    checks = unittest.TestCase()
    fields = {"text": read.text}
    if any("gs1" in b for b in case["expected"]):
        fields["gs1"] = read.gs1
    if any("eanAddOn" in b for b in case["expected"]):
        fields["eanAddOn"] = read.ean_add_on
    if any("format" in b for b in case["expected"]):
        fields["format"] = read.format
    if read.structured_append is not None:
        append = read.structured_append
        fields["structuredAppend"] = {
            "index": append.index,
            "count": append.count,
            "parity": append.parity,
        }
    if read.payload_bytes is not None:
        expected_read = next(b for b in case["expected"] if b["text"] == read.text)
        if "payloadBytes" in expected_read:
            fields["payloadBytes"] = list(read.payload_bytes)
            checks.assertNotEqual(read.payload_bytes, read.text.encode("utf-8"))
    return fields


def check_geometry(result: tapirscan.ScanResult, case: dict[str, Any]) -> None:
    """Assert main-barcode extents and supplement association by physical position."""
    if "geometry" not in case:
        return
    checks = unittest.TestCase()
    reads = sorted(result, key=lambda read: read.rect.left)
    checks.assertEqual(len(reads), len(case["geometry"]), case["name"])
    for read, expected in zip(reads, case["geometry"], strict=True):
        checks.assertEqual(read.ean_add_on, expected["eanAddOn"], case["name"])
        checks.assertAlmostEqual(
            min(p.x for p in read.polygon), expected["left"], delta=4
        )
        checks.assertAlmostEqual(
            max(p.x for p in read.polygon), expected["right"], delta=4
        )


def check(manifest_path: Path, library_dir: str | None = None) -> list[dict[str, Any]]:
    """Check real metadata, geometry, lifetime and single-format selection."""
    checks = unittest.TestCase()
    fixtures = json.loads(manifest_path.read_text())
    observations = []
    for mode in ("low", "medium", "high", "very-high"):
        for case in fixtures:
            pixels = tapirscan.PixelImage(
                (manifest_path.parent / case["file"]).read_bytes(),
                width=case["width"],
                height=case["height"],
            )
            with tapirscan.Scanner(
                mode,
                formats=case["formats"],
                library_dir=library_dir,
                ean_add_on_policy=case.get("eanAddOnPolicy", "Ignore"),
            ) as scanner:
                for debug in (False, True):
                    result = scanner.scan(pixels, debug=debug)
                    check_geometry(result, case)
                    checks.assertCountEqual(
                        result.values,
                        [b["text"] for b in case["expected"]],
                        (mode, case["name"]),
                    )
                    if "unfinished" in case:
                        checks.assertEqual(result.unfinished, case["unfinished"])
                    exported = result.as_dict()
                    checks.assertEqual(json.loads(json.dumps(exported)), exported)
                    checks.assertEqual(exported["values"], result.values)
                    actual = []
                    for read in result:
                        actual.append(metadata(read, case))
                        checks.assertIn(
                            read.format,
                            [case["formats"]]
                            if isinstance(case["formats"], str)
                            else case["formats"],
                        )
                        checks.assertGreater(read.support, 0)
                        checks.assertEqual(len(read.polygon), 4)
                        checks.assertGreater(read.rect.width, 0)
                        checks.assertGreater(read.rect.height, 0)
                        checks.assertTrue(
                            all(
                                0 <= p.x <= case["width"] and 0 <= p.y <= case["height"]
                                for p in read.polygon
                            )
                        )
                    checks.assertCountEqual(actual, case["expected"], case["name"])
                    checks.assertEqual(result.debug is not None, debug)
                    if debug and case.get("expectUnread"):
                        checks.assertIsNotNone(result.debug)
                        if (
                            result.debug is not None
                            and result.debug.regions is not None
                        ):
                            checks.assertTrue(
                                result.debug.regions.undecoded, case["name"]
                            )
                        else:
                            checks.fail("Missing undecoded evidence")
                    if not debug:
                        observations.append(
                            {
                                "mode": mode,
                                "name": case["name"],
                                "unfinished": result.unfinished,
                                "barcodes": [
                                    {
                                        "text": b.text,
                                        "format": b.format,
                                        "support": b.support,
                                        "eanAddOn": b.ean_add_on,
                                        "polygon": b.polygon,
                                    }
                                    for b in result
                                ],
                            }
                        )
            checks.assertCountEqual(
                result.values, [b["text"] for b in case["expected"]]
            )
    return observations


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--library-dir")
    args = parser.parse_args()
    print(
        json.dumps(
            {
                "package": tapirscan.__file__,
                "observations": check(args.manifest, args.library_dir),
            }
        )
    )

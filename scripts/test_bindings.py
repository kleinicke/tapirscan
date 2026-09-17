#!/usr/bin/env python3
"""End-to-end native binding tests, against Rust and the original WASM host."""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, cast

from build_java import JAR, tool
from fixture_data import TEXT, fixtures

from build import ROOT

# Set BARCODE_PYTHON_PACKAGE to test an installed wheel instead of source.
sys.path.insert(
    0, os.environ.get("BARCODE_PYTHON_PACKAGE", str(ROOT / "bindings/python/src"))
)
from tapirscan import PixelImage, Scanner

LIBS = ROOT / "build/native"


def run(*args: object) -> dict[str, Any]:
    """Run a checked local command and return its decoded output."""
    return cast(
        "dict[str, Any]",
        json.loads(subprocess.check_output(list(map(str, args)), text=True)),
    )


class Bindings(unittest.TestCase):
    """Verify native and WASM results across language interfaces."""

    def test_continuation_parity(self) -> None:
        """The public continuation flag preserves native/WASM reader parity."""
        with tempfile.TemporaryDirectory(prefix="barcode-continuation-") as temp:
            path = Path(temp) / "pixels.raw"
            for mode in ("low", "medium", "high", "very-high"):
                with Scanner(mode, library_dir=LIBS) as scanner:
                    for name, pixels, w, h, channels, stride, _ in fixtures():
                        with self.subTest(mode=mode, fixture=name):
                            path.write_bytes(pixels)
                            native = scanner.scan(
                                PixelImage(
                                    pixels,
                                    width=w,
                                    height=h,
                                    channels=channels,
                                    stride=stride,
                                ),
                                debug=True,
                                extended_budget=True,
                            ).to_raw_dict()
                            wasm = run(
                                "node",
                                ROOT / "bindings/javascript/test/native_parity.mjs",
                                mode,
                                w,
                                h,
                                channels,
                                stride,
                                path,
                                1,
                                1,
                                "EAN13",
                                1,
                            )
                            self.assertEqual(
                                native["scan"]["barcodes"], wasm["scan"]["barcodes"]
                            )
                            self.assertEqual(
                                native["scan"]["unfinished"], wasm["scan"]["unfinished"]
                            )

    def test_all_languages_and_modes(self) -> None:
        """Verify all languages and modes."""
        with tempfile.TemporaryDirectory(prefix="barcode-binding-test-") as temp:
            for mode in ("low", "medium", "high", "very-high"):
                with Scanner(mode, library_dir=LIBS) as scanner:
                    for name, pixels, w, h, c, stride, expected in fixtures():
                        with self.subTest(mode=mode, fixture=name):
                            path = Path(temp) / "pixels.raw"
                            path.write_bytes(pixels)
                            result = scanner.scan(
                                PixelImage(
                                    pixels, width=w, height=h, channels=c, stride=stride
                                ),
                                debug=True,
                            ).to_raw_dict()
                            reads = result["scan"]["barcodes"]
                            self.assertEqual(len(reads), expected)
                            self.assertTrue(all(b["text"] == TEXT for b in reads))
                            self.assertEqual(result["mode"], mode)
                            self.assertEqual(len(result["searchWindows"]), 1)
                            rust = run(
                                ROOT
                                / f"build/{mode}/cargo-target/release/examples"
                                / "scan_raw",
                                w,
                                h,
                                path,
                                c,
                                stride,
                            )
                            cpp = run(
                                ROOT / f"build/cpp-{mode}/scan_raw",
                                w,
                                h,
                                c,
                                stride,
                                path,
                                1,
                                1,
                            )
                            classpath = os.pathsep.join(
                                map(
                                    str,
                                    [
                                        JAR,
                                        ROOT / "build/java/test-classes",
                                    ],
                                )
                            )
                            java = run(
                                tool("java"),
                                "--enable-native-access=ALL-UNNAMED",
                                "-cp",
                                classpath,
                                "org.tapirscan.Smoke",
                                LIBS,
                                mode,
                                w,
                                h,
                                c,
                                stride,
                                path,
                                1,
                                1,
                            )
                            js = run(
                                "node",
                                ROOT / "bindings/javascript/test/native_parity.mjs",
                                mode,
                                w,
                                h,
                                c,
                                stride,
                                path,
                                1,
                                1,
                            )
                            typed = [
                                {k: b[k] for k in ("text", "support", "polygon")}
                                for b in reads
                            ]
                            self.assertEqual(cpp["typed"], typed)
                            self.assertEqual(java["typed"], typed)
                            cpp = cpp["result"]
                            java = java["result"]
                            for foreign in (
                                rust,
                                cpp["scan"],
                                java["scan"],
                                js["scan"],
                            ):
                                for key in result["scan"]:
                                    self.assertEqual(
                                        foreign[key], result["scan"][key], key
                                    )
                            for foreign in (cpp, java):
                                self.assertEqual(
                                    foreign["localization"], result["localization"]
                                )
                                self.assertEqual(
                                    foreign["searchWindows"], result["searchWindows"]
                                )
                            self.assertEqual(
                                js["localization"]["proposals"],
                                result["localization"]["proposals"],
                            )
                            self.assertEqual(
                                js["localization"]["omitted"],
                                result["localization"]["omitted"],
                            )
                            self.assertEqual(
                                js["localization"]["workLimited"],
                                result["localization"]["workLimited"],
                            )
                            self.assertEqual(
                                js["searchWindows"], result["searchWindows"]
                            )
                            self.assertEqual(
                                max(
                                    result["scan"]["barcodes"],
                                    key=lambda b: b["support"],
                                    default=None,
                                ),
                                max(reads, key=lambda b: b["support"], default=None),
                            )

    def test_output_choices_across_languages(self) -> None:
        """Verify output choices across languages."""
        sample = next(f for f in fixtures() if f[0] == "same-value-pair")
        _, pixels, w, h, c, stride, _ = sample
        classpath = os.pathsep.join(
            map(
                str,
                [
                    JAR,
                    ROOT / "build/java/test-classes",
                ],
            )
        )
        with tempfile.TemporaryDirectory(prefix="barcode-options-") as temp:
            path = Path(temp) / "pixels.raw"
            path.write_bytes(pixels)
            for mode in ("low", "medium", "high", "very-high"):
                with Scanner(mode, library_dir=LIBS) as scanner:
                    defaults = scanner.scan(
                        PixelImage(pixels, width=w, height=h)
                    ).to_raw_dict()
                    self.assertTrue(defaults["multiple"])
                    self.assertEqual(len(defaults["scan"]["barcodes"]), 2)
                    self.assertNotIn("localization", defaults)
                    all_details = scanner.scan(
                        PixelImage(pixels, width=w, height=h), debug=True
                    ).to_raw_dict()
                    for multiple in (True, False):
                        for regions in (False, True):
                            with self.subTest(
                                mode=mode, multiple=multiple, regions=regions
                            ):
                                result = scanner.scan(
                                    PixelImage(pixels, width=w, height=h), debug=regions
                                ).to_raw_dict()
                                self.assertEqual(result["schemaVersion"], 2)
                                expected = (
                                    defaults["scan"]["barcodes"]
                                    if multiple
                                    else [
                                        max(
                                            defaults["scan"]["barcodes"],
                                            key=lambda b: b["support"],
                                        )
                                    ]
                                )
                                self.assertEqual(
                                    result["scan"]["barcodes"],
                                    defaults["scan"]["barcodes"],
                                )
                                self.assertEqual(
                                    result["scan"]["unfinished"],
                                    defaults["scan"]["unfinished"],
                                )
                                for key in ("localization", "searchWindows"):
                                    self.assertEqual(key in result, regions)
                                self.assertEqual(
                                    "candidates" in result["scan"], regions
                                )
                                if regions:
                                    self.assertEqual(
                                        result["scan"]["candidates"],
                                        all_details["scan"]["candidates"],
                                    )
                                args = [
                                    w,
                                    h,
                                    c,
                                    stride,
                                    path,
                                    int(multiple),
                                    int(regions),
                                ]
                                cpp = run(ROOT / f"build/cpp-{mode}/scan_raw", *args)
                                java = run(
                                    tool("java"),
                                    "--enable-native-access=ALL-UNNAMED",
                                    "-cp",
                                    classpath,
                                    "org.tapirscan.Smoke",
                                    LIBS,
                                    mode,
                                    *args,
                                )
                                js = run(
                                    "node",
                                    ROOT / "bindings/javascript/test/native_parity.mjs",
                                    mode,
                                    *args,
                                )
                                rust = run(
                                    ROOT
                                    / f"build/{mode}/cargo-target/release/examples"
                                    / "scan_options",
                                    mode,
                                    *args,
                                )
                                typed = [
                                    {k: b[k] for k in ("text", "support", "polygon")}
                                    for b in expected
                                ]
                                self.assertEqual(cpp["typed"], typed)
                                self.assertEqual(java["typed"], typed)
                                for other in (cpp["result"], java["result"], js, rust):
                                    self.assertEqual(other["multiple"], multiple)
                                    self.assertEqual(
                                        other["scan"]["barcodes"], expected
                                    )
                                    self.assertEqual("localization" in other, regions)
                                    self.assertEqual("searchWindows" in other, regions)
                                    self.assertEqual(
                                        "candidates" in other["scan"], regions
                                    )
                                    if regions:
                                        self.assertEqual(
                                            other["scan"]["candidates"],
                                            result["scan"]["candidates"],
                                        )
                    blank = scanner.scan(
                        PixelImage(bytes([255]) * len(pixels), width=w, height=h)
                    ).to_raw_dict()
                    self.assertEqual(blank["scan"]["barcodes"], [])

    def test_python_validation_and_lifetime(self) -> None:
        """Verify python validation and lifetime."""
        for mode in ("low", "medium", "high", "very-high"):
            scanner = Scanner(mode, library_dir=LIBS)
            _, pixels, w, h, _c, _stride, _ = next(fixtures())
            first = scanner.scan(PixelImage(pixels, width=w, height=h)).to_raw_dict()
            frozen = json.dumps(first)
            for options, data in [
                ({"width": -1, "height": h}, pixels),
                ({"width": True, "height": h}, pixels),
                ({"width": w, "height": h, "channels": 2}, pixels),
                ({"width": w, "height": h, "stride": w - 1}, pixels),
                ({"width": w, "height": h}, pixels[:1]),
            ]:
                with self.assertRaises(ValueError):
                    scanner.scan(PixelImage(data, **options)).to_raw_dict()
            with self.assertRaises(ValueError):
                scanner.scan(
                    PixelImage(memoryview(pixels)[::2], width=w, height=h)
                ).to_raw_dict()
            self.assertEqual(
                scanner.scan(
                    PixelImage(bytes([255]) * len(pixels), width=w, height=h)
                ).to_raw_dict()["scan"]["barcodes"],
                [],
            )
            self.assertEqual(json.dumps(first), frozen)
            scanner.close()
            scanner.close()
            with self.assertRaises(RuntimeError):
                scanner.scan(PixelImage(pixels, width=w, height=h)).to_raw_dict()
            self.assertEqual(json.dumps(first), frozen)
        with self.assertRaises(ValueError):
            Scanner("typo", library_dir=LIBS)  # ty: ignore[invalid-argument-type]

    def test_concurrent_modes(self) -> None:
        """Verify concurrent modes."""
        _, pixels, w, h, _, _, _ = next(fixtures())
        with (
            Scanner("medium", library_dir=LIBS) as fast,
            Scanner("high", library_dir=LIBS) as quality,
        ):
            with ThreadPoolExecutor(max_workers=4) as pool:
                scanners = [fast, quality, fast, quality]
                results = list(
                    pool.map(
                        lambda scanner: scanner.scan(
                            PixelImage(pixels, width=w, height=h)
                        ).to_raw_dict(),
                        scanners,
                    )
                )
            self.assertEqual(
                [r["mode"] for r in results], ["medium", "high", "medium", "high"]
            )
            self.assertTrue(
                all(r["scan"]["barcodes"][0]["text"] == TEXT for r in results)
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)

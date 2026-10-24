#!/usr/bin/env python3
"""End-to-end parity of the Python, Rust, C++, Java and JavaScript bindings."""

import json
import math
import os
import subprocess
import sys
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, cast

from build_java import JAR, tool
from build_native import EXAMPLES, library_name
from fixture_data import CODABAR_SYMBOLS, TEXT, codabar, fixtures

from build import ROOT

# Set BARCODE_PYTHON_PACKAGE to test an installed wheel instead of source.
sys.path.insert(
    0, os.environ.get("BARCODE_PYTHON_PACKAGE", str(ROOT / "bindings/python/src"))
)
from tapirscan import Content, ContentField, InspectionResult, PixelImage, Scanner

LIBS = ROOT / "build/native"
MODES = ("low", "medium", "high", "very-high")
EAN13 = 1
# Engine evidence compared across languages; timing fields are excluded.
EVIDENCE = ("scan", "localization", "searchWindows")


def assert_wasm_parity(
    actual: object, expected: object, path: str = "result", *, polygon: bool = False
) -> None:
    """Compare JSON exactly except for finite polygon coordinates within 1e-9 px."""
    checks = unittest.TestCase()
    if isinstance(actual, dict) and isinstance(expected, dict):
        checks.assertEqual(actual.keys(), expected.keys(), path)
        for key in expected:
            assert_wasm_parity(
                actual[key], expected[key], f"{path}.{key}", polygon=key == "polygon"
            )
    elif isinstance(actual, list) and isinstance(expected, list):
        checks.assertEqual(len(actual), len(expected), path)
        for i, (left, right) in enumerate(zip(actual, expected, strict=True)):
            assert_wasm_parity(left, right, f"{path}[{i}]", polygon=polygon)
    elif (
        polygon
        and isinstance(actual, (int, float))
        and isinstance(expected, (int, float))
    ):
        # Native and WASM geometry can differ by a few f64 ULPs. Do not round
        # outputs or use relative tolerance: the allowance is fixed in pixels.
        checks.assertIn(type(actual), (int, float), path)
        checks.assertIn(type(expected), (int, float), path)
        checks.assertTrue(math.isfinite(actual) and math.isfinite(expected), path)
        checks.assertAlmostEqual(actual, expected, delta=1e-9, msg=path)
    elif polygon:
        checks.fail(f"{path}: polygon coordinates must be numbers")
    else:
        checks.assertEqual(actual, expected, path)


def run(*args: object) -> dict[str, Any]:
    """Run a checked local command and return its decoded output."""
    return cast(
        "dict[str, Any]",
        json.loads(subprocess.check_output(list(map(str, args)), text=True)),
    )


def harnesses(
    mode: str,
    image: tuple[int, int, int, int, Path],
    *,
    debug: bool,
    formats: tuple[int, str] = (EAN13, "EAN13"),
) -> dict[str, dict[str, Any]]:
    """Scan one raw image with every non-Python binding's scan_raw harness."""
    w, h, c, stride, path = image
    bits, name = formats
    args = [mode, w, h, c, stride, path, int(debug), bits]
    classpath = os.pathsep.join([str(JAR), str(ROOT / "build/java/test-classes")])
    return {
        "rust": run(EXAMPLES / "scan_raw", *args),
        "cpp": run(ROOT / "build/cpp/scan_raw", *args),
        "java": run(
            tool("java"),
            f"-Dtapirscan.library={LIBS / library_name()}",
            "--enable-native-access=ALL-UNNAMED",
            "-cp",
            classpath,
            "org.tapirscan.Smoke",
            *args,
        ),
        "js": run(
            "node",
            ROOT / "bindings/javascript/test/native_parity.mjs",
            *args[:7],
            name,
        ),
    }


def typed(result: InspectionResult) -> dict[str, Any]:
    """Project a Python result onto the harnesses' typed JSON shape."""
    best = (
        None
        if result.best is None
        else next(i for i, b in enumerate(result.barcodes) if b is result.best)
    )
    return {
        "mode": result.mode,
        "best": best,
        "barcodes": [
            {
                "text": b.text,
                "format": b.format,
                "support": b.support,
                "polygon": [[p.x, p.y] for p in b.polygon],
                "content": b.as_dict()["content"],
                "codabarStartStop": b.codabar_start_stop,
            }
            for b in result.barcodes
        ],
        "undecoded": [
            {"format": r.format, "polygon": [[p.x, p.y] for p in r.polygon]}
            for r in result.undecoded
        ],
    }


class Bindings(unittest.TestCase):
    """Verify every language binding returns the same results."""

    def test_all_languages_modes_and_diagnostics(self) -> None:
        """Typed results and engine evidence agree in every binding and mode."""
        with tempfile.TemporaryDirectory(prefix="tapirscan-bindings-") as temp:
            path = Path(temp) / "pixels.raw"
            for mode in MODES:
                with Scanner(mode, formats="EAN13", library_dir=LIBS) as scanner:
                    for name, pixels, w, h, c, stride, expected in fixtures():
                        path.write_bytes(pixels)
                        image = PixelImage(
                            pixels, width=w, height=h, channels=c, stride=stride
                        )
                        for debug in (False, True):
                            with self.subTest(mode=mode, fixture=name, debug=debug):
                                result = scanner.inspect(image)
                                self.assertEqual(
                                    scanner.scan(image).barcodes, result.barcodes
                                )
                                self.assertEqual(result.values, [TEXT] * expected)
                                reference = typed(result)
                                raw = result.diagnostics.to_raw_dict()
                                for language, other in harnesses(
                                    mode, (w, h, c, stride, path), debug=debug
                                ).items():
                                    evidence = other.pop("debug")
                                    if language == "js":
                                        assert_wasm_parity(other, reference, language)
                                    else:
                                        self.assertEqual(other, reference, language)
                                    if not debug:
                                        self.assertIsNone(evidence, language)
                                        continue
                                    for key in EVIDENCE:
                                        if language == "js":
                                            assert_wasm_parity(
                                                evidence[key], raw[key], f"js.{key}"
                                            )
                                        else:
                                            self.assertEqual(
                                                evidence[key], raw[key], (language, key)
                                            )

    def test_content_json_agrees_across_languages(self) -> None:
        """A GS1-128 symbol has the same content JSON in every binding.

        Rust serde, C++ and Java parse it from the typed result, JavaScript
        receives it in the WASM JSON, and Python parses it while scanning.
        """
        # Start C, FNC1, (01)09506000134352, check character and stop.
        widths = (
            "2112324111312221222212132311313141112122221221321123312133111311412331112"
        )
        width, height, scale = 480, 120, 3
        pixels = bytearray([255]) * (width * height)
        x = 40
        for i, digit in enumerate(widths):
            modules = int(digit) * scale
            if i % 2 == 0:
                for y in range(20, 100):
                    pixels[y * width + x : y * width + x + modules] = bytes(modules)
            x += modules
        expected = {
            "kind": "GS1",
            "fields": [{"name": "01", "value": "09506000134352"}],
        }
        code128 = (16, "Code128")
        with (
            tempfile.TemporaryDirectory(prefix="tapirscan-content-") as temp,
            Scanner("medium", formats="Code128", library_dir=LIBS) as scanner,
        ):
            path = Path(temp) / "pixels.raw"
            path.write_bytes(bytes(pixels))
            image = PixelImage(bytes(pixels), width=width, height=height)
            result = scanner.inspect(image)
            self.assertEqual(
                [b.as_dict()["content"] for b in result.barcodes], [expected]
            )
            self.assertEqual(
                [b.content for b in scanner.scan(image).barcodes],
                [Content("GS1", (ContentField("01", "09506000134352"),))],
            )
            reference = typed(result)
            for language, other in harnesses(
                "medium", (width, height, 1, width, path), debug=False, formats=code128
            ).items():
                other.pop("debug")
                if language == "js":
                    assert_wasm_parity(other, reference, language)
                else:
                    self.assertEqual(other, reference, language)

    def test_codabar_start_stop_agrees_across_languages(self) -> None:
        """Every binding reports Codabar start/stop letters beside the stripped text."""
        pixels = codabar()
        width, height = 480, len(pixels) // 480
        expected = sorted((data, start + stop) for start, data, stop in CODABAR_SYMBOLS)
        with tempfile.TemporaryDirectory(prefix="tapirscan-codabar-") as temp:
            path = Path(temp) / "pixels.raw"
            path.write_bytes(pixels)
            for mode in MODES:
                with (
                    self.subTest(mode=mode),
                    Scanner(mode, formats="Codabar", library_dir=LIBS) as scanner,
                ):
                    result = scanner.inspect(
                        PixelImage(pixels, width=width, height=height)
                    )
                    self.assertEqual(
                        sorted((b.text, b.codabar_start_stop) for b in result.barcodes),
                        expected,
                    )
                    reference = typed(result)
                    for language, other in harnesses(
                        mode,
                        (width, height, 1, width, path),
                        debug=False,
                        formats=(128, "Codabar"),
                    ).items():
                        other.pop("debug")
                        if language == "js":
                            assert_wasm_parity(other, reference, language)
                        else:
                            self.assertEqual(other, reference, language)

    def test_python_validation_and_lifetime(self) -> None:
        """Invalid input is rejected and results outlive their scanner."""
        for mode in MODES:
            scanner = Scanner(mode, library_dir=LIBS)
            _, pixels, w, h, _c, _stride, _ = next(fixtures())
            first = scanner.inspect(
                PixelImage(pixels, width=w, height=h)
            ).diagnostics.to_raw_dict()
            frozen = json.dumps(first)
            for options, data in [
                ({"width": -1, "height": h}, pixels),
                ({"width": True, "height": h}, pixels),
                ({"width": w, "height": h, "channels": 2}, pixels),
                ({"width": w, "height": h, "stride": w - 1}, pixels),
                ({"width": w, "height": h}, pixels[:1]),
            ]:
                with self.assertRaises(ValueError):
                    scanner.inspect(PixelImage(data, **options))
            with self.assertRaises(ValueError):
                scanner.inspect(PixelImage(memoryview(pixels)[::2], width=w, height=h))
            self.assertEqual(
                scanner.inspect(
                    PixelImage(bytes([255]) * len(pixels), width=w, height=h)
                ).values,
                [],
            )
            scanner.close()
            scanner.close()
            with self.assertRaises(RuntimeError):
                scanner.inspect(PixelImage(pixels, width=w, height=h))
            self.assertEqual(json.dumps(first), frozen)
        with self.assertRaises(ValueError):
            Scanner("typo", library_dir=LIBS)  # ty: ignore[invalid-argument-type]

    def test_concurrent_modes(self) -> None:
        """Scanners in different modes share one library and run concurrently."""
        _, pixels, w, h, _, _, _ = next(fixtures())
        with (
            Scanner("medium", library_dir=LIBS) as medium,
            Scanner("high", library_dir=LIBS) as high,
            ThreadPoolExecutor(max_workers=4) as pool,
        ):
            results = list(
                pool.map(
                    lambda scanner: scanner.inspect(
                        PixelImage(pixels, width=w, height=h)
                    ),
                    [medium, high, medium, high],
                )
            )
        self.assertEqual(
            [r.mode for r in results], ["medium", "high", "medium", "high"]
        )
        self.assertTrue(all(r.values == [TEXT] for r in results))


class WasmParityComparison(unittest.TestCase):
    """Keep the coordinate allowance from hiding scanner or metadata regressions."""

    def test_coordinate_roundoff(self) -> None:
        """Accept the observed native/WASM difference in public and raw polygons."""
        native = {"polygon": [[429.25898295157083, 149.58305617058048]]}
        wasm = {"polygon": [[429.25898295157083, 149.58305617058056]]}
        for key in ("barcodes", "undecoded", "scan"):
            with self.subTest(key=key):
                assert_wasm_parity({key: [wasm]}, {key: [native]})

    def test_geometry_regressions(self) -> None:
        """Reject shifts, missing coordinates, nonnumeric values and nonfinite data."""
        for left, right in (
            ([[149.0, 30.0]], [[149.000001, 30.0]]),
            ([[1_000_000.0, 30.0]], [[1_000_000.000001, 30.0]]),
            ([[149.0, 30.0]], [[149.0]]),
            ([[149.0, 30.0]], []),
            ([[0.0, 30.0]], [[False, 30.0]]),
            ([[149.0, 30.0]], [["149", 30.0]]),
            ([[math.inf, 30.0]], [[math.inf, 30.0]]),
            ([[math.nan, 30.0]], [[math.nan, 30.0]]),
        ):
            with (
                self.subTest(left=left, right=right),
                self.assertRaises(AssertionError),
            ):
                assert_wasm_parity({"polygon": left}, {"polygon": right})

    def test_everything_else_stays_exact(self) -> None:
        """Preserve metadata, collection shape/order and nonpolygon float checks."""
        for left, right in (
            ({"text": "123"}, {"text": "124"}),
            ({"support": 7}, {"support": 8}),
            ({"mode": "low"}, {"mode": "high"}),
            ({"best": 0}, {"best": 1}),
            ({"score": 1.0}, {"score": 1.0 + 1e-12}),
            ({"barcodes": ["first", "second"]}, {"barcodes": ["second", "first"]}),
            ({"barcodes": []}, {"barcodes": [], "undecoded": []}),
        ):
            with (
                self.subTest(left=left, right=right),
                self.assertRaises(AssertionError),
            ):
                assert_wasm_parity(left, right)


if __name__ == "__main__":
    unittest.main(verbosity=2)

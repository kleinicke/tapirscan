"""Paired multi-language tests; reference code is used only to encode known inputs."""

import os
import tempfile
import unittest
from pathlib import Path

import numpy as np
import zxingcpp
from build_java import JAR, tool
from test_bindings import LIBS, PixelImage, Scanner, run

from build import ROOT

MODES = ("low", "medium", "high", "very-high")
CASES = (
    ("EAN8", "EAN8", "96385074", 4),
    ("Code128", "Code128", "Tapirscan 1234567890", 16),
    ("QRCode", "QRCode", 'Tapirscan: "Grüße"\n' + "Long payload " * 18, 512),
    ("DataMatrix", "DataMatrix", "Tapirscan matrix 123", 1024),
    ("Aztec", "AztecCode", "Tapirscan aztec 123", 4096),
)


class Formats(unittest.TestCase):
    """Check full payloads, format masks, source geometry and shared ownership."""

    def test_distinct_copies_and_selection(self) -> None:
        """Keep spatially separate equal values when combining format readers."""
        linear = np.asarray(
            zxingcpp.write_barcode_to_image(
                zxingcpp.create_barcode("TAPIR123", zxingcpp.BarcodeFormat.Code128),
                scale=3,
                add_hrt=False,
                add_quiet_zones=True,
            )
        )
        matrix = np.asarray(
            zxingcpp.write_barcode_to_image(
                zxingcpp.create_barcode("Tapir matrix", zxingcpp.BarcodeFormat.QRCode),
                scale=3,
                add_hrt=False,
                add_quiet_zones=True,
            )
        )
        image = np.full((700, 1000), 255, dtype=np.uint8)
        for tile, top, left in (
            (linear, 30, 30),
            (linear, 300, 400),
            (matrix, 520, 30),
        ):
            height, width = tile.shape
            image[top : top + height, left : left + width] = tile
        pixels = image.tobytes()
        with tempfile.TemporaryDirectory(prefix="tapirscan-mixed-") as tmp:
            path = Path(tmp) / "pixels.raw"
            path.write_bytes(pixels)
            for mode in MODES:
                with (
                    self.subTest(mode=mode),
                    Scanner(mode, library_dir=LIBS) as scanner,
                ):
                    result = scanner.scan(
                        PixelImage(pixels, width=1000, height=700),
                        formats=["EAN13", "Code128", "QRCode"],
                        debug=True,
                    )
                    self.assertCountEqual(
                        result.values, ["TAPIR123", "TAPIR123", "Tapir matrix"]
                    )
                    js = run(
                        "node",
                        ROOT / "bindings/javascript/test/native_parity.mjs",
                        mode,
                        1000,
                        700,
                        1,
                        1000,
                        path,
                        1,
                        1,
                        "EAN13,Code128,QRCode",
                    )
                    self.assertEqual(
                        [(b.text, b.format) for b in result],
                        [(b["text"], b["format"]) for b in js["scan"]["barcodes"]],
                    )
                    self.assertEqual(result.best, result[0])
                    linear_only = scanner.scan(
                        PixelImage(pixels, width=1000, height=700), formats=["Code128"]
                    )
                    self.assertEqual(linear_only.values, ["TAPIR123", "TAPIR123"])

    def test_qr_pixel_layout_parity(self) -> None:
        """Direct WASM RGBA upload preserves conversion, alpha and padded layouts."""
        text = "Tapir QR rgba 1234567890"
        tile = np.asarray(
            zxingcpp.write_barcode_to_image(
                zxingcpp.create_barcode(text, zxingcpp.BarcodeFormat.QRCode),
                scale=3,
                add_hrt=False,
                add_quiet_zones=True,
            )
        )
        height, width = tile.shape
        rgba = np.empty((height, width, 4), dtype=np.uint8)
        rgba[:, :, :3] = tile[:, :, None]
        rgba[:, :, 3] = np.arange(width, dtype=np.uint8)
        rgba[tile == 0, :3] = [12, 31, 7]
        rgba[tile != 0, :3] = [241, 250, 235]
        gray = (
            (
                rgba[:, :, 0].astype(np.uint32) * 77
                + rgba[:, :, 1].astype(np.uint32) * 150
                + rgba[:, :, 2].astype(np.uint32) * 29
            )
            >> 8
        ).astype(np.uint8)
        padded = np.full((height, width * 4 + 11), 123, dtype=np.uint8)
        padded[:, : width * 4] = rgba.reshape(height, width * 4)
        layouts = [
            (gray.tobytes(), 1, width),
            (rgba.tobytes(), 4, width * 4),
            (rgba[:, :, :3].tobytes(), 3, width * 3),
            (padded.tobytes(), 4, width * 4 + 11),
        ]
        with tempfile.TemporaryDirectory(prefix="tapirscan-qr-layout-") as tmp:
            path = Path(tmp) / "pixels.raw"
            for mode in MODES:
                with Scanner(mode, formats="QRCode", library_dir=LIBS) as scanner:
                    expected = None
                    for pixels, channels, stride in layouts:
                        with self.subTest(mode=mode, channels=channels, stride=stride):
                            result = scanner.scan(
                                PixelImage(
                                    pixels,
                                    width=width,
                                    height=height,
                                    channels=channels,
                                    stride=stride,
                                )
                            )
                            self.assertEqual(result.values, [text])
                            observed = [(b.text, b.support, b.polygon) for b in result]
                            if expected is not None:
                                self.assertEqual(observed, expected)
                            expected = observed
                            path.write_bytes(pixels)
                            js = run(
                                "node",
                                ROOT / "bindings/javascript/test/native_parity.mjs",
                                mode,
                                width,
                                height,
                                channels,
                                stride,
                                path,
                                1,
                                1,
                                "QRCode",
                            )
                            self.assertEqual(
                                result.unfinished, js["scan"]["unfinished"]
                            )
                            self.assertEqual(len(result), len(js["scan"]["barcodes"]))
                            for native, wasm in zip(
                                result, js["scan"]["barcodes"], strict=True
                            ):
                                self.assertEqual(native.text, wasm["text"])
                                self.assertEqual(native.support, wasm["support"])
                                for point, coordinates in zip(
                                    native.polygon, wasm["polygon"], strict=True
                                ):
                                    self.assertAlmostEqual(
                                        point.x, coordinates[0], delta=0.0001
                                    )
                                    self.assertAlmostEqual(
                                        point.y, coordinates[1], delta=0.0001
                                    )

    def test_formats_across_languages(self) -> None:
        """Compare native and WASM on identical independently encoded pixels."""
        with tempfile.TemporaryDirectory(prefix="tapirscan-formats-") as tmp:
            path = Path(tmp) / "pixels.raw"
            for fmt, encoder, text, mask in CASES:
                encoded = zxingcpp.create_barcode(
                    text, getattr(zxingcpp.BarcodeFormat, encoder)
                )
                image = np.asarray(
                    zxingcpp.write_barcode_to_image(
                        encoded, scale=3, add_hrt=False, add_quiet_zones=True
                    )
                )
                height, width = image.shape
                pixels = image.tobytes()
                path.write_bytes(pixels)
                for mode in MODES:
                    with (
                        self.subTest(format=fmt, mode=mode),
                        Scanner(mode, library_dir=LIBS) as scanner,
                    ):
                        result = scanner.scan(
                            PixelImage(pixels, width=width, height=height),
                            formats=[fmt],
                            debug=True,
                        )
                        self.assertEqual(result.values, [text])
                        self.assertEqual(result[0].format, fmt)
                        self.assertEqual(result[0].text.encode(), text.encode())
                        js = run(
                            "node",
                            ROOT / "bindings/javascript/test/native_parity.mjs",
                            mode,
                            width,
                            height,
                            1,
                            width,
                            path,
                            1,
                            1,
                            fmt,
                        )
                        cpp = run(
                            ROOT / f"build/cpp-{mode}/scan_raw",
                            width,
                            height,
                            1,
                            width,
                            path,
                            1,
                            1,
                            mask,
                        )
                        java = run(
                            tool("java"),
                            "--enable-native-access=ALL-UNNAMED",
                            "-cp",
                            os.pathsep.join(
                                [
                                    str(JAR),
                                    str(ROOT / "build/java/test-classes"),
                                ]
                            ),
                            "org.tapirscan.Smoke",
                            LIBS,
                            mode,
                            width,
                            height,
                            1,
                            width,
                            path,
                            1,
                            1,
                            mask,
                        )
                        rust = run(
                            ROOT
                            / f"build/{mode}/cargo-target/release/examples"
                            / "scan_options",
                            mode,
                            width,
                            height,
                            1,
                            width,
                            path,
                            1,
                            1,
                            mask,
                        )
                        self.assertEqual(
                            rust["scan"]["barcodes"],
                            result.to_raw_dict()["scan"]["barcodes"],
                        )
                        for foreign in (cpp, java):
                            self.assertEqual(foreign["typed"][0]["text"], text)
                            self.assertEqual(
                                foreign["result"]["scan"]["barcodes"],
                                result.to_raw_dict()["scan"]["barcodes"],
                            )
                        if result.debug is None:
                            self.fail("Diagnostics were requested")
                        self.assertEqual(
                            [b.support for b in result.debug.barcodes],
                            [b["support"] for b in js["scan"]["barcodes"]],
                        )
                        for b, other in zip(
                            result, js["scan"]["barcodes"], strict=True
                        ):
                            self.assertEqual(
                                (b.text, b.format),
                                (other["text"], other["format"]),
                            )
                            for point, foreign in zip(
                                b.polygon, other["polygon"], strict=True
                            ):
                                self.assertAlmostEqual(point.x, foreign[0], places=4)
                                self.assertAlmostEqual(point.y, foreign[1], places=4)
                        self.assertEqual(result.unfinished, js["scan"]["unfinished"])
                        with self.assertRaises(ValueError):
                            scanner.scan(
                                PixelImage(pixels, width=width, height=height),
                                formats=[],
                            )


if __name__ == "__main__":
    unittest.main(verbosity=2)

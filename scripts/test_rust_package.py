#!/usr/bin/env python3
"""Compare packaged Rust modes against the existing native bindings."""

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

from api_fixtures import generate as api_fixtures
from fixture_data import fixtures
from supplement_fixtures import generate as supplement_fixtures
from test_detail import make_fixtures as detail_fixtures

from build import ROOT

sys.path.insert(0, str(ROOT / "bindings/python/src"))
from tapirscan import Mode, PixelImage, Scanner


def check(package: Path) -> None:
    """Exercise layouts, physical copies, metadata, work limits and supplements."""
    destination = ROOT / "build/rust-parity"
    destination.mkdir(parents=True, exist_ok=True)
    images: list[dict[str, Any]] = []
    for name, pixels, width, height, channels, stride, _ in fixtures():
        path = destination / f"{name}.raw"
        path.write_bytes(pixels)
        images.append(
            {
                "file": str(path),
                "width": width,
                "height": height,
                "channels": channels,
                "stride": stride,
                "formats": "EAN13",
            }
        )
    detail = destination / "detail"
    images.extend(
        {
            **case,
            "file": str(detail / f"{case['name']}.rgba"),
            "channels": 4,
            "stride": case["width"] * 4,
            "formats": "EAN13",
        }
        for case in detail_fixtures(detail)
    )
    encoder = os.environ.get("ZINT", str(ROOT / "build/zint-encoder/frontend/zint"))
    for generate, name in [(api_fixtures, "api"), (supplement_fixtures, "supplements")]:
        manifest = generate(destination / name, encoder)
        images.extend(
            {**case, "file": str(manifest.parent / case["file"])}
            for case in json.loads(manifest.read_text())
        )
    bits = {
        "EAN13": 1,
        "UPCA": 2,
        "EAN8": 4,
        "UPCE": 8,
        "Code128": 16,
        "QRCode": 512,
        "DataMatrix": 1024,
        "Aztec": 4096,
        "PDF417": 2048,
        "MaxiCode": 131072,
    }
    policies = ["Ignore", "Read", "Require"]
    cases: list[dict[str, Any]] = []
    modes: list[Mode] = ["low", "medium", "high", "very-high"]
    for mode_index, mode in enumerate(modes):
        for image in images:
            selected = image["formats"]
            mask = (
                sum(bits[name] for name in selected)
                if isinstance(selected, list)
                else bits[selected]
            )
            policy = image.get("eanAddOnPolicy", "Ignore")
            channels, stride = (
                image.get("channels", 1),
                image.get("stride", image["width"]),
            )
            for complete in [False, True]:
                with Scanner(
                    mode,
                    formats=image["formats"],
                    ean_add_on_policy=policy,
                    library_dir=ROOT / "build/native",
                ) as scanner:
                    result = scanner.scan(
                        PixelImage(
                            Path(image["file"]).read_bytes(),
                            width=image["width"],
                            height=image["height"],
                            channels=channels,
                            stride=stride,
                        ),
                        debug=True,
                        extended_budget=complete,
                    )
                expected = result.to_raw_dict()
                del expected["elapsedMs"]
                if result.debug is None or result.debug.regions is None:
                    message = "Requested diagnostics missing"
                    raise AssertionError(message)
                cases.append(
                    {
                        "mode": mode_index,
                        "pixels": image["file"],
                        "width": image["width"],
                        "height": image["height"],
                        "channels": channels,
                        "stride": stride,
                        "formats": mask,
                        "addons": policies.index(policy),
                        "complete": complete,
                        "expected": expected,
                        "undecoded": len(result.debug.regions.undecoded),
                    }
                )
    if not any(case["expected"].get("recovery", {}).get("additions") for case in cases):
        message = "Corpus must exercise successful source-detail recovery"
        raise AssertionError(message)
    path = destination / "cases.json"
    path.write_text(json.dumps(cases))
    subprocess.run(
        [
            "cargo",
            "+1.91.1",
            "test",
            "--offline",
            "--release",
            "--manifest-path",
            str(package / "Cargo.toml"),
            "--test",
            "parity",
            "--",
            "--ignored",
            "--nocapture",
        ],
        env={**os.environ, "TAPIRSCAN_PARITY_CASES": str(path)},
        check=True,
    )


if __name__ == "__main__":
    check(Path(sys.argv[1]).resolve())

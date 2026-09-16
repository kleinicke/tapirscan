"""Measure warm-process creation and paired repeated scans; never enforce timings."""

import argparse
import json
import platform
import statistics
import sys
from collections.abc import Callable
from functools import partial
from pathlib import Path
from time import perf_counter_ns
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bindings/python/src"))
import tapirscan  # noqa: E402 - select the source checkout before import

WARMUP = 5
POLICIES: tuple[tapirscan.EanAddOnPolicy, ...] = ("Ignore", "Read", "Require")
SCENES = ("EAN13-none", "EAN13-12", "EAN13-51234", "different-supplements")


def timed(action: Callable[[], Any]) -> tuple[Any, float]:
    """Time only the API operation, excluding disposal and result validation."""
    start = perf_counter_ns()
    value = action()
    return value, (perf_counter_ns() - start) / 1_000_000


def summary(samples: list[float]) -> dict[str, float]:
    """Summarize warm-process timing samples in milliseconds."""
    return {
        "medianMs": statistics.median(samples),
        "minMs": min(samples),
        "maxMs": max(samples),
    }


def measure(
    fixtures: list[dict[str, Any]],
    root: Path,
    mode: tapirscan.Mode,
    scene: str,
    count: int,
) -> list[dict[str, Any]]:
    """Rotate policy order within each repetition on the exact same pixels."""
    cases = {
        p: next(f for f in fixtures if f["name"] == f"{scene}-{p}") for p in POLICIES
    }
    case = cases["Ignore"]
    pixels = tapirscan.PixelImage(
        (root / case["file"]).read_bytes(), width=case["width"], height=case["height"]
    )
    creation: dict[str, list[float]] = {p: [] for p in POLICIES}
    scans: dict[str, list[float]] = {p: [] for p in POLICIES}
    for repeat in range(count + WARMUP):
        for policy in POLICIES[repeat % 3 :] + POLICIES[: repeat % 3]:
            scanner, elapsed = timed(
                partial(
                    tapirscan.Scanner,
                    mode,
                    ean_add_on_policy=policy,
                    library_dir=str(ROOT / "build/native"),
                )
            )
            scanner.close()
            if repeat >= WARMUP:
                creation[policy].append(elapsed)
    scanners = {
        p: tapirscan.Scanner(
            mode, ean_add_on_policy=p, library_dir=str(ROOT / "build/native")
        )
        for p in POLICIES
    }
    try:
        for repeat in range(count + WARMUP):
            for policy in POLICIES[repeat % 3 :] + POLICIES[: repeat % 3]:
                result, elapsed = timed(partial(scanners[policy].scan, pixels))
                actual = sorted((b.text, b.ean_add_on or "") for b in result)
                expected = sorted(
                    (b["text"], b["eanAddOn"] or "") for b in cases[policy]["expected"]
                )
                if actual != expected:
                    msg = f"Unexpected benchmark decode: {scene}/{policy}/{mode}"
                    raise RuntimeError(msg)
                if repeat >= WARMUP:
                    scans[policy].append(elapsed)
    finally:
        for scanner in scanners.values():
            scanner.close()
    return [
        {
            "mode": mode,
            "scene": scene,
            "policy": p,
            "width": case["width"],
            "height": case["height"],
            "create": summary(creation[p]),
            "scan": summary(scans[p]),
            "createSamplesMs": creation[p],
            "scanSamplesMs": scans[p],
        }
        for p in POLICIES
    ]


def main() -> None:
    """Write reproducible samples and environment details as JSON."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--samples", type=int, default=25)
    args = parser.parse_args()
    if args.samples < 1:
        parser.error("--samples must be positive")
    fixtures = json.loads(args.manifest.read_text())
    rows = []
    for mode in ("low", "medium", "high", "very-high"):
        for scene in SCENES:
            rows.extend(
                measure(
                    fixtures,
                    args.manifest.parent,
                    mode,
                    scene,
                    args.samples,
                )
            )
    print(
        json.dumps(
            {
                "runtime": sys.version,
                "platform": platform.platform(),
                "warmup": WARMUP,
                "samples": args.samples,
                "results": rows,
            }
        )
    )


if __name__ == "__main__":
    main()

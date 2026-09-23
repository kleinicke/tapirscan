#!/usr/bin/env python3
"""Compare built native or WASM scanners on identical inputs and paired timings."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import math
import os
import platform
import subprocess
import sys
import time
from contextlib import ExitStack
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

from PIL import Image
from PIL import __version__ as pillow_version

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bindings/python/src"))
from tapirscan import PixelImage, Scanner  # noqa: E402 - choose the source facade

if TYPE_CHECKING:
    from tapirscan import EanAddOnPolicy, Mode
    from tapirscan.formats import FormatSelection

MODES = ("low", "medium", "high", "very-high")
TIMING_KEYS = frozenset(
    {
        "elapsedMs",
        "extraMs",
        "elapsed_ms",
        "sampling_ms",
        "interpretation_ms",
        "support_ms",
    }
)


def clean(value: object) -> object:
    """Ignore timing fields; retain order, geometry and work counters."""
    if isinstance(value, dict):
        return {
            key: clean(item) for key, item in value.items() if key not in TIMING_KEYS
        }
    if isinstance(value, list):
        return [clean(item) for item in value]
    return value


def digest(path: Path) -> str:
    """Identify the exact input bytes."""
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def checkout(path: Path) -> dict[str, object]:
    """Record source state independently of the separately hashed built artifacts."""
    command = ["git", "-C", str(path)]
    commit = subprocess.check_output([*command, "rev-parse", "HEAD"], text=True).strip()
    changes = subprocess.check_output([*command, "diff", "HEAD", "--binary"])
    untracked = (
        subprocess.check_output(
            [*command, "ls-files", "--others", "--exclude-standard", "-z"]
        )
        .decode()
        .split("\0")
    )
    return {
        "path": str(path),
        "commit": commit,
        "trackedDiffSha256": hashlib.sha256(changes).hexdigest(),
        "untracked": {name: digest(path / name) for name in untracked if name},
        "dirty": bool(changes or any(untracked)),
    }


def timing_indices(length: int, count: int) -> set[int]:
    """Select an evenly spaced, deterministic subset without duplicate indices."""
    count = min(length, count)
    return {index * length // count for index in range(count)} if count else set()


def distribution(values: list[float]) -> dict[str, float]:
    """Report mean and nearest-rank percentiles in milliseconds."""
    if not values:
        msg = "cannot summarize an empty measurement group"
        raise ValueError(msg)
    values = sorted(values)
    return {
        "meanMs": sum(values) / len(values),
        **{
            name: values[max(0, math.ceil(q * len(values)) - 1)]
            for name, q in [("medianMs", 0.5), ("p90Ms", 0.9), ("p95Ms", 0.95)]
        },
    }


def summaries(samples: list[dict[str, Any]]) -> dict[str, object]:
    """Keep paired mode/format/budget groups separate."""
    result: dict[str, object] = {}
    for group in sorted({str(row["group"]) for row in samples}):
        rows = [row for row in samples if row["group"] == group]
        before = distribution([row["baseline"] for row in rows])
        after = distribution([row["candidate"] for row in rows])
        result[group] = {
            "pairs": len(rows),
            "baseline": before,
            "candidate": after,
            "meanRatio": after["meanMs"] / before["meanMs"],
        }
    return result


def load_case(row: dict[str, Any], index: int, root: Path) -> dict[str, Any]:
    """Read an image or explicit raw fixture without altering the stored input."""
    path = (root / row.get("image", row.get("file", ""))).resolve()
    if not path.is_file():
        msg = f"missing input image: {path}"
        raise ValueError(msg)
    if "image" in row:
        with Image.open(path) as image:
            rgba = image.convert("RGBA")
            data, width, height = rgba.tobytes(), rgba.width, rgba.height
        channels, stride = 4, width * 4
    else:
        data = path.read_bytes()
        width, height = row["width"], row["height"]
        channels = row.get("channels", 1)
        stride = row.get("stride", width * channels)
    addon = row.get("eanAddOnPolicy", "Ignore")
    if addon not in ("Ignore", "Read", "Require"):
        msg = f"invalid supplement policy: {addon}"
        raise ValueError(msg)
    return {
        "index": index,
        "name": row.get("name", row.get("image", row.get("file"))),
        "path": str(path),
        "sha256": digest(path),
        "pixelsSha256": hashlib.sha256(data).hexdigest(),
        "data": data,
        "width": width,
        "height": height,
        "channels": channels,
        "stride": stride,
        "formats": row.get("formats"),
        "addon": addon,
    }


def variants(
    case: dict[str, Any], config: dict[str, Any]
) -> list[tuple[str, object, bool]]:
    """Explicit fixture formats override global image selections."""
    selections = [case["formats"]] if case["formats"] else config["formats"]
    return [
        (
            json.dumps(selection, separators=(",", ":")),
            None if selection == "retail" else selection,
            budget == "extended",
        )
        for selection in selections
        for budget in config["budgets"]
    ]


def scan_native(
    scanner: Scanner,
    case: dict[str, Any],
    formats: object,
    *,
    extended: bool,
    debug: bool,
) -> tuple[float, object]:
    """Time the public scan call, then normalize its result outside the measurement."""
    start = time.perf_counter_ns()
    result = scanner.scan(
        case["image"],
        formats=cast("FormatSelection | None", formats),
        extended_budget=extended,
        debug=debug,
    )
    elapsed = (time.perf_counter_ns() - start) / 1e6
    return elapsed, clean(
        {
            "public": result.as_dict(),
            "diagnostics": result.to_raw_dict() if debug else None,
        }
    )


class NativeComparison:
    """Own paired scanners, bounded differences and measurement evidence for one run."""

    def __init__(self, config: dict[str, Any], output: Path, stack: ExitStack) -> None:
        """Keep scanners alive across frames to measure normal reusable sessions."""
        self.config, self.output, self.stack = config, output, stack
        self.scanners: dict[tuple[str, str], tuple[Scanner, Scanner]] = {}
        self.samples: list[dict[str, Any]] = []
        self.retained: list[dict[str, Any]] = []
        self.identities: list[dict[str, Any]] = []
        self.comparisons = self.differing = self.timed_comparisons = 0
        self.results = (
            stack.enter_context((output / "results.jsonl").open("w"))
            if config.get("saveResults")
            else None
        )
        self.differences = stack.enter_context((output / "differences.jsonl").open("w"))

    def pair(self, mode: str, addon: str) -> tuple[Scanner, Scanner]:
        """Lazily create the selected mode and supplement-policy sessions."""
        key = mode, addon
        if key not in self.scanners:
            instances = [
                self.stack.enter_context(
                    Scanner(
                        cast("Mode", mode),
                        library_dir=self.config[label + "Native"],
                        ean_add_on_policy=cast("EanAddOnPolicy", addon),
                    )
                )
                for label in ("baseline", "candidate")
            ]
            self.scanners[key] = instances[0], instances[1]
        return self.scanners[key]

    def compare(
        self, before: object, after: object, context: dict[str, object]
    ) -> None:
        """Count every difference while bounding retained full diagnostic output."""
        if before != after:
            self.differing += 1
            if self.differing <= self.config["maxDifferences"]:
                self.differences.write(
                    json.dumps({**context, "baseline": before, "candidate": after})
                    + "\n"
                )

    def parity(self, case: dict[str, Any]) -> None:
        """Compare each configured mode, format selection and budget on one image."""
        for mode in self.config["modes"]:
            before, after = self.pair(mode, case["addon"])
            for label, formats, extended in variants(case, self.config):
                a = scan_native(
                    before,
                    case,
                    formats,
                    extended=extended,
                    debug=self.config["diagnostics"],
                )[1]
                b = scan_native(
                    after,
                    case,
                    formats,
                    extended=extended,
                    debug=self.config["diagnostics"],
                )[1]
                if self.results is not None:
                    self.results.write(
                        json.dumps(
                            {
                                "index": case["index"],
                                "case": case["name"],
                                "mode": mode,
                                "selection": label,
                                "extendedBudget": extended,
                                "baseline": cast("dict[str, object]", a)["public"],
                                "candidate": cast("dict[str, object]", b)["public"],
                            }
                        )
                        + "\n"
                    )
                self.comparisons += 1
                self.compare(
                    a,
                    b,
                    {
                        "case": case["name"],
                        "mode": mode,
                        "selection": label,
                        "extendedBudget": extended,
                        "phase": "parity",
                    },
                )

    def warmup(self, mode: str) -> None:
        """Exercise the same format and budget combinations before timing."""
        for case in self.retained[: self.config["warmup"]]:
            for _, formats, extended in variants(case, self.config):
                for scanner in self.pair(mode, case["addon"]):
                    scan_native(scanner, case, formats, extended=extended, debug=False)

    def time_case(
        self, mode: str, case: dict[str, Any], repetition: int, index: int
    ) -> None:
        """Alternate paired order and verify timed results as well as elapsed time."""
        before, after = self.pair(mode, case["addon"])
        for label, formats, extended in variants(case, self.config):
            order = [("baseline", before), ("candidate", after)]
            if (index + repetition) % 2:
                order.reverse()
            measured = {
                name: scan_native(
                    scanner, case, formats, extended=extended, debug=False
                )
                for name, scanner in order
            }
            budget = "extended" if extended else "default"
            group = f"{mode}/{label}/{budget}/{case['addon']}"
            self.compare(
                measured["baseline"][1],
                measured["candidate"][1],
                {
                    "case": case["name"],
                    "group": group,
                    "phase": "timing",
                    "repetition": repetition,
                },
            )
            self.timed_comparisons += 1
            self.samples.append(
                {
                    "case": case["name"],
                    "group": group,
                    "repetition": repetition,
                    **{name: value[0] for name, value in measured.items()},
                }
            )

    def run(self, cases: list[dict[str, Any]]) -> dict[str, Any]:
        """Finish input decoding and parity before collecting timing samples."""
        selected = timing_indices(len(cases), self.config["timingCount"])
        for index, row in enumerate(cases):
            case = load_case(row, index, Path(self.config["datasetRoot"]))
            self.identities.append(
                {
                    key: case[key]
                    for key in (
                        "index",
                        "name",
                        "path",
                        "sha256",
                        "pixelsSha256",
                        "width",
                        "height",
                        "channels",
                        "stride",
                        "formats",
                        "addon",
                    )
                }
            )
            case["image"] = PixelImage(
                case.pop("data"),
                width=case["width"],
                height=case["height"],
                channels=case["channels"],
                stride=case["stride"],
            )
            self.parity(case)
            if index in selected:
                self.retained.append(case)
            if (index + 1) % 50 == 0:
                print(f"{index + 1} images compared", file=sys.stderr, flush=True)
        for mode in self.config["modes"]:
            self.warmup(mode)
            for repetition in range(self.config["repeats"]):
                for index, case in enumerate(self.retained):
                    self.time_case(mode, case, repetition, index)
        (self.output / "inputs.json").write_text(
            json.dumps(self.identities, indent=2) + "\n"
        )
        (self.output / "timings.jsonl").write_text(
            "".join(json.dumps(row) + "\n" for row in self.samples)
        )
        return {
            "comparisons": self.comparisons,
            "timedComparisons": self.timed_comparisons,
            "differences": self.differing,
            "timings": summaries(self.samples),
            "runtime": {"python": platform.python_version()},
        }


def native(
    config: dict[str, Any], cases: list[dict[str, Any]], output: Path
) -> dict[str, Any]:
    """Close all native sessions even if a scan or evidence write fails."""
    with ExitStack() as stack:
        return NativeComparison(config, output, stack).run(cases)


def wasm(
    config: dict[str, Any], cases: list[dict[str, Any]], output: Path
) -> dict[str, Any]:
    """Stream decoded pixels to Node, retaining only the requested timing cohort."""
    identities = []
    config_path = output / "wasm-config.json"
    config_path.write_text(
        json.dumps(
            {
                **config,
                "output": str(output),
                "timingIndices": sorted(
                    timing_indices(len(cases), config["timingCount"])
                ),
            }
        )
    )
    with subprocess.Popen(
        [config["node"], str(ROOT / "scripts/compare_wasm.mjs"), str(config_path)],
        stdin=subprocess.PIPE,
        text=True,
    ) as process:
        if process.stdin is None:
            msg = "Node input pipe was not created"
            raise RuntimeError(msg)
        try:
            for index, row in enumerate(cases):
                case = load_case(row, index, Path(config["datasetRoot"]))
                identities.append(
                    {
                        key: case[key]
                        for key in (
                            "index",
                            "name",
                            "path",
                            "sha256",
                            "pixelsSha256",
                            "width",
                            "height",
                            "channels",
                            "stride",
                            "formats",
                            "addon",
                        )
                    }
                )
                case["data"] = base64.b64encode(case["data"]).decode("ascii")
                process.stdin.write(json.dumps(case) + "\n")
        finally:
            process.stdin.close()
        if process.wait():
            msg = "WASM comparison worker failed; see its error above"
            raise RuntimeError(msg)
    (output / "inputs.json").write_text(json.dumps(identities, indent=2) + "\n")
    result = json.loads((output / "wasm-result.json").read_text())
    samples = [
        json.loads(line) for line in (output / "timings.jsonl").read_text().splitlines()
    ]
    result["timings"] = summaries(samples)
    return result


def artifacts(config: dict[str, Any], label: str) -> dict[str, object]:
    """Hash actual libraries and assets independently of the source checkout."""
    if config["backend"] == "native":
        root = Path(config[label + "Native"])
        suffix = (
            ".dll"
            if sys.platform == "win32"
            else ".dylib"
            if sys.platform == "darwin"
            else ".so"
        )
        prefix = "" if sys.platform == "win32" else "lib"
        files = [
            root / f"{prefix}tapirscan_{mode.replace('-', '_')}{suffix}"
            for mode in config["modes"]
        ]
    else:
        manifest = Path(config[label + "Wasm"])
        entries = json.loads(manifest.read_text())["modes"]
        root = Path(config[label + "Assets"])
        files = [
            manifest,
            *sorted((Path(config[label]) / "bindings/javascript/dist").rglob("*.js")),
            *[
                root / next(entry["file"] for entry in entries if entry["mode"] == mode)
                for mode in config["modes"]
            ],
        ]
    return {str(path): digest(path) for path in files}


def arguments() -> argparse.Namespace:
    """Require explicit source, dataset and new evidence locations."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", default=ROOT, type=Path)
    parser.add_argument("--dataset-root", required=True, type=Path)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument(
        "--output",
        required=True,
        type=Path,
        help="New directory; existing evidence is never overwritten",
    )
    parser.add_argument(
        "--backend", choices=("native", "wasm", "browser"), default="native"
    )
    parser.add_argument("--modes", nargs="+", choices=MODES, default=list(MODES))
    parser.add_argument("--formats", nargs="+", default=["EAN13", "retail"])
    parser.add_argument(
        "--budgets", nargs="+", choices=("default", "extended"), default=["default"]
    )
    parser.add_argument(
        "--diagnostics", action=argparse.BooleanOptionalAction, default=True
    )
    parser.add_argument("--timing-count", type=int, default=50)
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--limit", type=int)
    parser.add_argument("--max-differences", type=int, default=10)
    parser.add_argument("--node", default="node")
    parser.add_argument(
        "--browser-channel", choices=("chrome", "chromium"), default="chromium"
    )
    parser.add_argument(
        "--playwright-module", help="Absolute path to a Playwright module"
    )
    parser.add_argument(
        "--allow-differences",
        action="store_true",
        help="Record algorithm changes without requiring exact parity",
    )
    parser.add_argument(
        "--save-results",
        action="store_true",
        help="Retain public outputs for labeled evaluation",
    )
    for label in ("baseline", "candidate"):
        parser.add_argument(f"--{label}-native", type=Path)
        parser.add_argument(
            f"--{label}-wasm", type=Path, help="WASM identity manifest override"
        )
        parser.add_argument(
            f"--{label}-assets", type=Path, help="WASM asset directory override"
        )
    args = parser.parse_args()
    if (
        min(args.timing_count, args.warmup, args.max_differences) < 0
        or args.repeats < 1
        or (args.limit is not None and args.limit < 1)
    ):
        parser.error("counts must be nonnegative; repeats and limit must be positive")
    return args


def main() -> None:
    """Write identities, exact differences and paired distribution summaries."""
    args = arguments()
    config = {
        "backend": args.backend,
        "datasetRoot": str(args.dataset_root.resolve()),
        "modes": args.modes,
        "formats": args.formats,
        "budgets": args.budgets,
        "diagnostics": args.diagnostics,
        "timingCount": args.timing_count,
        "repeats": args.repeats,
        "warmup": args.warmup,
        "maxDifferences": args.max_differences,
        "node": args.node,
        "playwrightModule": args.playwright_module,
        "browserChannel": args.browser_channel,
        "saveResults": args.save_results,
        "allowDifferences": args.allow_differences,
    }
    sources = {}
    for label in ("baseline", "candidate"):
        root = getattr(args, label).resolve()
        sources[label] = checkout(root)
        config[label] = str(root)
        config[label + "Native"] = str(
            (getattr(args, label + "_native") or root / "build/native").resolve()
        )
        selection = json.loads((root / "provenance/modes.json").read_text())
        config[label + "Wasm"] = str(
            (getattr(args, label + "_wasm") or root / selection["apiWasm"]).resolve()
        )
        config[label + "Assets"] = str(
            (
                getattr(args, label + "_assets") or root / "bindings/javascript/wasm"
            ).resolve()
        )
    cases = json.loads(args.manifest.read_text())
    if not isinstance(cases, list) or not cases:
        msg = "manifest must be a nonempty JSON array of image or raw fixture records"
        raise ValueError(msg)
    if args.limit:
        cases = cases[: args.limit]
    identities = {
        label: artifacts(config, label) for label in ("baseline", "candidate")
    }
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {
        "schema": 1,
        "configuration": config,
        "sources": sources,
        "artifacts": identities,
        "manifest": {
            "path": str(args.manifest.resolve()),
            "sha256": digest(args.manifest),
        },
        "host": {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "cpuCount": os.cpu_count(),
        },
        "harness": {
            "checkout": checkout(ROOT),
            "scriptSha256": digest(Path(__file__)),
            "wasmWorkerSha256": digest(ROOT / "scripts/compare_wasm.mjs"),
            "browserWorkerSha256": digest(ROOT / "scripts/compare_browser.mjs"),
            "pillowVersion": pillow_version,
            "nativeFacade": (
                "Current harness Python facade; both libraries must support its ABI"
            ),
        },
        "method": (
            "Exact ordered results and optional diagnostics, excluding timing fields. "
            "Timing uses preloaded images after parity, alternating paired order, "
            "warm scans, debug false. Includes public binding conversion, excludes "
            "load/compile and image decoding. Runtime-specific measurements, "
            "not cross-runtime or general speed claims."
        ),
    }
    result = (native if args.backend == "native" else wasm)(config, cases, output)
    report.update(result)
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                "report": str(output / "report.json"),
                "comparisons": result["comparisons"],
                "timedComparisons": result["timedComparisons"],
                "differences": result["differences"],
            }
        )
    )
    if result["differences"] and not args.allow_differences:
        raise SystemExit(1)


if __name__ == "__main__":
    main()

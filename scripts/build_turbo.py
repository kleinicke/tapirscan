#!/usr/bin/env python3
"""Build private Turbo artifacts without adding a public scanner mode."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TIERS = ("original", "2", "4", "8", "16")


def environment(tier: str) -> dict[str, str]:
    """Use only the selected, validated private recipe."""
    env = {
        key: value
        for key, value in os.environ.items()
        if not key.startswith(
            ("TAPIRSCAN_TURBO_", "TAPIRSCAN_EXPERIMENT_", "TAPIRSCAN_EXPERIMENTAL_")
        )
    }
    env["TAPIRSCAN_EXPERIMENTAL_TURBO"] = "1"
    env["TAPIRSCAN_TURBO_TIER"] = "0" if tier == "original" else tier
    if tier != "original":
        for key in (
            "TAPIRSCAN_TURBO_LEGACY_OUTLINES",
            "TAPIRSCAN_TURBO_BAND_OUTLINES",
            "TAPIRSCAN_EXPERIMENT_TILE_CACHE",
            "TAPIRSCAN_TURBO_FINITE_EXTREMA",
        ):
            env[key] = "1"
        if tier != "16":
            env["TAPIRSCAN_TURBO_SELECTIVE_OUTLINES"] = "1"
        if tier == "8":
            env["TAPIRSCAN_TURBO_PROFILE_CAP"] = "576"
        if tier == "16":
            env["TAPIRSCAN_TURBO_DIRECT_SAMPLE"] = "1"
    return env


def main() -> None:
    """Delegate compilation to ordinary builders and retain explicit identities."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tier", choices=TIERS)
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--wasm", action="store_true")
    args = parser.parse_args()
    if not args.native and not args.wasm:
        parser.error("select --native and/or --wasm")
    env = environment(args.tier)
    destination = ROOT / "build/private-turbo" / args.tier
    destination.mkdir(parents=True, exist_ok=True)
    artifacts = {}
    for enabled, script, sources in (
        (args.native, "build_native.py", destination),
        (args.wasm, "build_wasm.py", ROOT / "build/wasm-development/assets"),
    ):
        if not enabled:
            continue
        command = [sys.executable, str(ROOT / "scripts" / script)]
        if script == "build_wasm.py":
            command.append("--development")
        else:
            command.extend(["--output", str(destination)])
        subprocess.run([*command, "low"], cwd=ROOT, env=env, check=True)
        for source in sources.iterdir():
            if source.name == "low.wasm" or source.name.startswith(
                ("libtapirscan_low.", "tapirscan_low.")
            ):
                target = destination / source.name
                if source != target:
                    shutil.copy2(source, target)
                artifacts[target.name] = hashlib.sha256(target.read_bytes()).hexdigest()
    record = {
        "private": True,
        "tier": args.tier,
        "apiMode": "low",
        "environment": {
            key: value for key, value in env.items() if key.startswith("TAPIRSCAN_")
        },
        "artifacts": artifacts,
    }
    (destination / "manifest.json").write_text(json.dumps(record, indent=2) + "\n")
    print(destination)


if __name__ == "__main__":
    main()

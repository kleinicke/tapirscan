#!/usr/bin/env python3
"""Build a selected mode directly from the maintained production core."""

import argparse
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MODE_CONFIG = json.loads((ROOT / "provenance/modes.json").read_text())["modes"]
MODES = {m["mode"]: (m["recipe"], m["tag"]) for m in MODE_CONFIG}


def wasm_flags() -> str:
    """Canonicalize std source paths whether rust-src is installed or absent."""
    sysroot = subprocess.check_output(
        ["rustc", "+1.91.1", "--print", "sysroot"],
        text=True,
    ).strip()
    commit = "ed61e7d7e242494fb7057f2657300d9e77bb4fcb"
    return (
        "-C target-feature=+simd128 "
        f"--remap-path-prefix={sysroot}/lib/rustlib/src/rust/library="
        f"/rustc/{commit}/library"
    )


def main() -> None:
    """Test maintained source, or explicitly reproduce a frozen historical recipe."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("--historical", action="store_true")
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--resume", action="store_true")
    args = parser.parse_args()
    if args.resume and not args.historical:
        parser.error("--resume applies only to --historical builds")
    if shutil.disk_usage(ROOT).free < 10 * 1024**3:
        msg = "need a 10 GiB free-space reserve before building"
        raise SystemExit(msg)
    subprocess.run(
        [sys.executable, str(ROOT / "scripts/verify_import.py"), "--historical-only"],
        check=True,
    )
    if args.historical:
        output = ROOT / "build/history"
        output.mkdir(parents=True, exist_ok=True)
        link = ROOT / "historical/build"
        if not link.exists():
            link.symlink_to(output, target_is_directory=True)
        command = [sys.executable, str(ROOT / "historical/scripts/build.py"), args.mode]
        if args.prepare_only:
            command.append("--prepare-only")
        if args.resume:
            command.append("--resume")
        subprocess.run(command, check=True)
        return
    if args.prepare_only:
        print(
            f"Production {args.mode}: {ROOT / 'core/src'} "
            "(no patches or preparation needed)"
        )
        return
    env = dict(
        os.environ,
        CARGO_TARGET_DIR=str(ROOT / "build/core-target"),
        RUSTUP_TOOLCHAIN="1.91.1",
    )
    env.pop("RUSTFLAGS", None)
    subprocess.run(
        [
            "cargo",
            "test",
            "--offline",
            "--manifest-path",
            str(ROOT / "core/Cargo.toml"),
            "--all-targets",
            "--no-default-features",
            "--features",
            f"mode-{args.mode}",
        ],
        env=env,
        check=True,
    )


if __name__ == "__main__":
    main()

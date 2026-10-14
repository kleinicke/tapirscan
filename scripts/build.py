#!/usr/bin/env python3
"""Build a selected mode directly from the maintained production core."""

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_CONFIG = json.loads((ROOT / "config/modes.json").read_text())
# Mode order defines engine IDs; budgets live in bindings/rust/src/effort.rs.
MODES = tuple(_CONFIG["modes"])
TURBO_PRESETS = tuple(_CONFIG["experimentalTurbo"])


def wasm_flags() -> list[str]:
    """Canonicalize std source paths whether rust-src is installed or absent."""
    sysroot = subprocess.check_output(
        ["rustc", "+1.99.0", "--print", "sysroot"],
        text=True,
    ).strip()
    version = subprocess.check_output(["rustc", "+1.99.0", "-vV"], text=True)
    commit = next(
        line.split(": ", 1)[1]
        for line in version.splitlines()
        if line.startswith("commit-hash: ")
    )
    return [
        "-C",
        "target-feature=+simd128",
        (
            f"--remap-path-prefix={sysroot}/lib/rustlib/src/rust/library="
            f"/rustc/{commit}/library"
        ),
    ]


def main() -> None:
    """Test the selected maintained production core."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    subprocess.run(
        [sys.executable, str(ROOT / "scripts/verify_sources.py")],
        check=True,
    )
    if args.prepare_only:
        print(
            f"Production {args.mode}: {ROOT / 'core/src'} "
            "(no patches or preparation needed)"
        )
        return
    env = dict(
        os.environ,
        CARGO_TARGET_DIR=str(ROOT / "build/core-target"),
        RUSTUP_TOOLCHAIN="1.99.0",
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

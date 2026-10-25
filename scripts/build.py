#!/usr/bin/env python3
"""Build a selected mode directly from the maintained production core."""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_CONFIG = json.loads((ROOT / "config/modes.json").read_text(encoding="utf-8"))
# Mode order defines engine IDs; budgets live in bindings/rust/src/effort.rs.
MODES = tuple(_CONFIG["modes"])
TURBO_PRESETS = tuple(_CONFIG["experimentalTurbo"])
# Build-time switches that the Rust sources read with `option_env!`, set only by
# Turbo presets (scripts/build_turbo.py).
PRIVATE_PREFIXES = ("TAPIRSCAN_TURBO_", "TAPIRSCAN_EXPERIMENTAL_")


def is_private(key: str) -> bool:
    """Whether an environment variable selects a private compile-time policy."""
    return key.startswith(PRIVATE_PREFIXES)


def clean_env() -> dict[str, str]:
    """Return the environment without private policy switches.

    Ordinary builds use it so that a variable left in the shell cannot silently
    change the compiled scanner; Turbo builds add their recipe on top.
    """
    return {key: value for key, value in os.environ.items() if not is_private(key)}


def rust_toolchain() -> str:
    """Return the Rust channel pinned by rust-toolchain.toml, the single source."""
    match = re.search(
        r'^channel\s*=\s*"([^"]+)"',
        (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if match is None:
        message = "rust-toolchain.toml has no channel"
        raise ValueError(message)
    return str(match[1])


def wasm_flags() -> list[str]:
    """Canonicalize std source paths whether rust-src is installed or absent."""
    rustc = ["rustc", f"+{rust_toolchain()}"]
    sysroot = subprocess.check_output(
        [*rustc, "--print", "sysroot"],
        text=True,
    ).strip()
    version = subprocess.check_output([*rustc, "-vV"], text=True)
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
        clean_env(),
        CARGO_TARGET_DIR=str(ROOT / "build/core-target"),
        RUSTUP_TOOLCHAIN=rust_toolchain(),
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

#!/usr/bin/env python3
"""Build the native library, containing all four modes, over the public Rust crate."""

import argparse
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path

from prepare_rust import prepared_package, sync_tree, write_changed

from build import ROOT

TARGET = ROOT / "build/native-target"
# Rust examples used by the cross-language parity tests.
EXAMPLES = TARGET / "release/examples"
# x86-64 builds target x86-64-v2 (SSE4.2, POPCNT): every Intel CPU since 2008 and
# AMD since 2011. Scans are 20-30% faster than the baseline; AVX2 (v3) adds 2-4%.
X86_64_RUSTFLAGS = "-C target-cpu=x86-64-v2"


def rustflags(target: str | None = None) -> list[str]:
    """Return a Rust target's fixed compiler flags followed by the caller's own.

    `target` defaults to the host. The caller's CARGO_ENCODED_RUSTFLAGS, which
    cargo prefers, or else RUSTFLAGS comes after the fixed flag, so a caller can
    still override it.
    """
    arch = target.split("-")[0] if target else platform.machine().lower()
    fixed = X86_64_RUSTFLAGS.split() if arch in ("x86_64", "amd64") else []
    encoded = os.environ.get("CARGO_ENCODED_RUSTFLAGS")
    if encoded is not None:
        caller = encoded.split("\x1f") if encoded else []
    else:
        caller = os.environ.get("RUSTFLAGS", "").split()
    return [*fixed, *caller]


def cargo_env(target_dir: Path, target: str | None = None) -> dict[str, str]:
    """Return the environment for cargo builds of a Rust target into `target_dir`."""
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    if flags := rustflags(target):
        env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    env["CARGO_TARGET_DIR"] = str(target_dir)
    env["CARGO_INCREMENTAL"] = "0"
    return env


def static_library(native: Path, target: str, env: dict[str, str]) -> Path:
    """Build the C ABI crate as a static library for one Rust target."""
    subprocess.run(
        [
            "cargo",
            "rustc",
            "--offline",
            "--release",
            "--lib",
            "--crate-type",
            "staticlib",
            "--target",
            target,
            "--manifest-path",
            str(native / "Cargo.toml"),
        ],
        env=env,
        check=True,
    )
    return Path(env["CARGO_TARGET_DIR"]) / target / "release/libtapirscan.a"


def library_name() -> str:
    """Platform file name of the native library."""
    if sys.platform == "win32":
        return "tapirscan.dll"
    return "libtapirscan.dylib" if sys.platform == "darwin" else "libtapirscan.so"


def prepare_native_crate(public_crate: Path) -> Path:
    """Write the C ABI crate over the prepared public crate; return its directory."""
    native = ROOT / "build/native-crate"
    sync_tree(ROOT / "bindings/c/src", native / "src")
    write_changed(
        native / "Cargo.toml",
        (ROOT / "bindings/c/Cargo.toml.in")
        .read_text()
        .replace("@PUBLIC_CRATE@", public_crate.as_posix())
        .encode(),
    )
    public_lock = public_crate / "Cargo.lock"
    if public_lock.exists():
        write_changed(native / "Cargo.lock", public_lock.read_bytes())
    return native


def build(public_crate: Path, destination: Path) -> None:
    """Build, test and install the native library and the Rust examples."""
    native = prepare_native_crate(public_crate)
    env = cargo_env(TARGET)
    common = ["--offline", "--manifest-path", str(native / "Cargo.toml")]
    subprocess.run(["cargo", "test", *common], env=env, check=True)
    name = library_name()
    command = ["cargo", "rustc", "--release", "--lib", *common]
    if sys.platform == "darwin":
        command += ["--", "-C", f"link-arg=-Wl,-install_name,@rpath/{name}"]
    elif sys.platform.startswith("linux"):
        command += ["--", "-C", f"link-arg=-Wl,-soname,{name}"]
    subprocess.run(command, env=env, check=True)
    subprocess.run(
        [
            "cargo",
            "build",
            "--offline",
            "--release",
            "--manifest-path",
            str(public_crate / "Cargo.toml"),
            "--examples",
        ],
        env=env,
        check=True,
    )
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(TARGET / "release" / name, destination / name)
    if sys.platform == "win32":
        shutil.copy2(
            TARGET / "release/tapirscan.dll.lib", destination / "tapirscan.dll.lib"
        )
    print(f"Built {destination / name}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "build/native",
        help="Separate output directory for private artifacts",
    )
    args = parser.parse_args()
    public = ROOT / "build/crates/tapirscan"
    with prepared_package(public, refresh=public.exists()):
        build(public, args.output)

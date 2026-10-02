#!/usr/bin/env python3
"""Build the native library, containing all four modes, over the public Rust crate."""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

from prepare_rust import prepared_package, sync_tree, write_changed

from build import ROOT

TARGET = ROOT / "build/native-target"
# Rust examples used by the cross-language parity tests.
EXAMPLES = TARGET / "release/examples"


def library_name() -> str:
    """Platform file name of the native library."""
    if sys.platform == "win32":
        return "tapirscan.dll"
    return "libtapirscan.dylib" if sys.platform == "darwin" else "libtapirscan.so"


def build(public_crate: Path, destination: Path) -> None:
    """Build, test and install the native library and the Rust examples."""
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
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    env["CARGO_TARGET_DIR"] = str(TARGET)
    env["CARGO_INCREMENTAL"] = "0"
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

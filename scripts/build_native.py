#!/usr/bin/env python3
"""Build native ABI libraries over the prepared public Rust package."""

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path

from prepare_rust import prepared_package, sync_tree, write_changed

from build import MODES, ROOT


def build(mode: str, public_crate: Path) -> None:
    """Build and validate one native adapter mode."""
    _, tag = MODES[mode]
    out = ROOT / "build" / mode
    out.mkdir(parents=True, exist_ok=True)
    native = out / "native"
    sync_tree(ROOT / "bindings/c/src", native / "src")
    write_changed(
        native / "Cargo.toml",
        (
            (ROOT / "bindings/c/Cargo.toml.in")
            .read_text()
            .replace("@MODE@", mode)
            .replace("@LIB_MODE@", mode.replace("-", "_"))
            .replace("@PUBLIC_CRATE@", public_crate.as_posix())
        ).encode(),
    )
    public_lock = public_crate / "Cargo.lock"
    if public_lock.exists():
        write_changed(native / "Cargo.lock", public_lock.read_bytes())
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    target = ROOT / "build/native-target" / mode
    env["CARGO_TARGET_DIR"] = str(target)
    env["CARGO_INCREMENTAL"] = "0"
    common = ["--offline", "--manifest-path", str(native / "Cargo.toml")]

    subprocess.run(["cargo", "test", *common], env=env, check=True)
    suffix = (
        ".dll"
        if sys.platform == "win32"
        else ".dylib"
        if sys.platform == "darwin"
        else ".so"
    )
    prefix = "" if sys.platform == "win32" else "lib"
    name = f"{prefix}tapirscan_{mode.replace(chr(45), chr(95))}{suffix}"
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
            "--no-default-features",
            "--features",
            f"mode-{mode}",
            "--examples",
        ],
        env=env,
        check=True,
    )
    dest = ROOT / "build/native"
    dest.mkdir(exist_ok=True)
    shutil.copy2(target / "release" / name, dest / name)
    if sys.platform == "win32":
        shutil.copy2(
            target / "release" / f"tapirscan_{mode.replace(chr(45), chr(95))}.dll.lib",
            dest / f"tapirscan_{mode.replace(chr(45), chr(95))}.dll.lib",
        )
    legacy_examples = out / "cargo-target/release/examples"
    legacy_examples.mkdir(parents=True, exist_ok=True)
    for example in ("scan_raw", "scan_options"):
        executable = example + (".exe" if sys.platform == "win32" else "")
        shutil.copy2(
            target / "release/examples" / executable, legacy_examples / executable
        )
    print(f"Built {tag}: {dest / name}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("modes", nargs="*", metavar="MODE")
    selected = parser.parse_args().modes or list(MODES)
    if unknown := [mode for mode in selected if mode not in MODES]:
        parser.error(f"unknown mode: {', '.join(unknown)}")
    public = ROOT / "build/crates/tapirscan"
    with prepared_package(public, refresh=public.exists()):
        for mode in selected:
            build(mode, public)

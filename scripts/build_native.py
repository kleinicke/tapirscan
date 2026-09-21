#!/usr/bin/env python3
"""Build selected native modes from verified pinned sources."""

import argparse
import os
import shutil
import subprocess
import sys

from build import (
    MODES,
    ROOT,
    prepare_facade,
    recipe_manifest,
)
from build_support import verify_source_hashes


def build(mode: str) -> None:
    """Build and validate one native scanner mode."""
    recipe, tag = MODES[mode]
    manifest = recipe_manifest(recipe)
    verify_source_hashes(ROOT / "core", manifest["baseHashes"])
    verify_source_hashes(ROOT, manifest.get("externalHashes", {}))
    out = ROOT / "build" / mode
    if not out.exists():
        subprocess.run(
            [sys.executable, str(ROOT / "scripts/build.py"), mode, "--prepare-only"],
            check=True,
        )
    prepared = out / "temporarysource"
    expected = dict(manifest["baseHashes"]) | manifest["targetHashes"]
    verify_source_hashes(prepared, expected)
    sdk = prepare_facade(out, mode, manifest)
    native = out / "native"
    shutil.copytree(ROOT / "bindings/c/src", native / "src", dirs_exist_ok=True)
    (native / "Cargo.toml").write_text(
        (ROOT / "bindings/c/Cargo.toml.in")
        .read_text()
        .replace("@MODE@", mode)
        .replace("@LIB_MODE@", mode.replace("-", "_"))
        .replace("@MODE_ID@", str(list(MODES).index(mode)))
    )
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    env["CARGO_TARGET_DIR"] = str(out / "cargo-target")
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
        ["cargo", "test", "--offline", "--manifest-path", str(sdk / "Cargo.toml")],
        env=env,
        check=True,
    )
    subprocess.run(
        [
            "cargo",
            "build",
            "--offline",
            "--release",
            "--manifest-path",
            str(sdk / "Cargo.toml"),
            "--examples",
        ],
        env=env,
        check=True,
    )
    dest = ROOT / "build/native"
    dest.mkdir(exist_ok=True)
    shutil.copy2(out / "cargo-target/release" / name, dest / name)
    if sys.platform == "win32":
        shutil.copy2(
            out
            / "cargo-target/release"
            / f"tapirscan_{mode.replace(chr(45), chr(95))}.dll.lib",
            dest / f"tapirscan_{mode.replace(chr(45), chr(95))}.dll.lib",
        )
    print(f"Built {tag}: {dest / name}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("modes", nargs="*", choices=list(MODES), default=list(MODES))
    selected = parser.parse_args().modes
    subprocess.run([sys.executable, str(ROOT / "scripts/verify_import.py")], check=True)
    for mode in selected:
        build(mode)

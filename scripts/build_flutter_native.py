#!/usr/bin/env python3
"""Build the native library for one Rust target; called by the Dart build hook.

The hook needs one shared library per target and app build, with no tests or
examples: unlike build_native.py (host library, tests) and build_android.py
(static library linked into the JNI bridge and an AAR).
"""

import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import cast

from build_native import TARGET, apple_link_arguments, cargo_env, prepare_native_crate
from native_identity import cargo_artifact, source_identity
from prepare_rust import prepared_package

from build import ROOT

# Read by bindings/flutter/hook/build.dart.
DEPENDENCIES = "dependencies.json"
DEPENDENCY_SELF = "scripts/build_flutter_native.py"


def library_name(target: str) -> str:
    """File name of the native library on the target's operating system."""
    if "windows" in target:
        return "tapirscan.dll"
    return "libtapirscan.dylib" if "apple" in target else "libtapirscan.so"


def link_arguments(target: str, linker: str | None, android_api: int) -> list[str]:
    """Return per-target `rustc` codegen arguments for the final link."""
    name = library_name(target)
    if "apple" in target:
        return apple_link_arguments(name)
    if "android" in target:
        # The target and API level repeat the wrapper's, for a plain clang
        # linker; 16 KiB pages as in the AAR.
        return [
            *(["-C", f"linker={linker}"] if linker else []),
            "-C",
            f"link-arg=--target={target}{android_api}",
            "-C",
            "link-arg=-Wl,-z,max-page-size=16384",
            "-C",
            f"link-arg=-Wl,-soname,{name}",
            "-C",
            "strip=symbols",
        ]
    if "linux" in target:
        return ["-C", f"link-arg=-Wl,-soname,{name}"]
    return []


def host_target(env: dict[str, str]) -> str:
    """Return the Rust host triple; host builds share the host output directory."""
    output = subprocess.run(
        ["rustc", "-vV"], env=env, text=True, stdout=subprocess.PIPE, check=True
    ).stdout
    return next(line for line in output.splitlines() if line.startswith("host: "))[6:]


def build(
    public_crate: Path,
    target: str,
    destination: Path,
    linker: str | None,
    android_api: int,
) -> Path:
    """Build the C ABI cdylib for `target` and copy it into `destination`."""
    identity = source_identity(ROOT)
    native = prepare_native_crate(public_crate)
    env = cargo_env(TARGET)
    env["RUSTUP_TOOLCHAIN"] = cast("str", identity["toolchain"])
    # A host build shares build_native.py's cache and compiler flags.
    cross = target != host_target(env)
    if cross:
        env = cargo_env(TARGET, target)
        env["RUSTUP_TOOLCHAIN"] = cast("str", identity["toolchain"])
        # Output reaches the hook, which shows it only when the build fails.
        subprocess.run(["rustup", "target", "add", target], env=env, check=True)
    if "android" in target and linker:
        # Prefer the NDK's per-target wrapper, as build_android.py does; it also
        # compiles any C sources of dependencies for the right API level.
        tools = Path(linker).parent
        wrapper = tools / f"{target}{android_api}-clang"
        linker = str(wrapper) if wrapper.exists() else linker
        env[f"CC_{target.replace('-', '_')}"] = linker
        env[f"AR_{target.replace('-', '_')}"] = str(tools / "llvm-ar")
    name = library_name(target)
    completed = subprocess.run(
        [
            "cargo",
            "rustc",
            "--offline",
            "--release",
            "--lib",
            "--message-format=json-render-diagnostics",
            *(["--target", target] if cross else []),
            "--manifest-path",
            str(native / "Cargo.toml"),
            "--",
            *link_arguments(target, linker, android_api),
        ],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        check=True,
    )
    artifact = cargo_artifact(completed.stdout, native, name)
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(artifact, destination / name)
    # The hook rebuilds when any of these files changes: the native identity's
    # inputs plus this recipe.
    sources = [*cast("dict[str, str]", identity["sourceFiles"]), DEPENDENCY_SELF]
    (destination / DEPENDENCIES).write_text(
        json.dumps([str(ROOT / source) for source in sources], indent=1) + "\n"
    )
    return destination / name


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, help="Rust target triple")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--linker", help="Android NDK clang")
    parser.add_argument("--android-api", type=int, default=24)
    args = parser.parse_args()
    public = ROOT / "build/crates/tapirscan"
    with prepared_package(public, refresh=True):
        output = build(public, args.target, args.output, args.linker, args.android_api)
    print(f"Built {output}", file=sys.stderr, flush=True)

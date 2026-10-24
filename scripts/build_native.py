#!/usr/bin/env python3
"""Build the native library, containing all four modes, over the public Rust crate."""

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path
from typing import cast

from build_turbo import TIERS as TURBO_TIERS
from build_turbo import environment as turbo_environment
from native_identity import build_metadata, cargo_artifact, record, source_identity
from prepare_rust import prepared_package, sync_tree, write_changed

from build import ROOT, clean_env

TARGET = ROOT / "build/native-target"
# Rust examples used by the cross-language parity tests.
EXAMPLES = TARGET / "release/examples"
# x86-64 builds target x86-64-v2 (SSE4.2, POPCNT): every Intel CPU since 2008 and
# AMD since 2011. Scans are 20-30% faster than the baseline; AVX2 (v3) adds 2-4%.
X86_64_RUSTFLAGS: str = "-C target-cpu=x86-64-v2"


def rustflags(target: str | None = None) -> list[str]:
    """Return a Rust target's fixed compiler flags, plus the caller's for the host.

    `target` is a Rust target triple for a cross build (iOS, Android, the other
    macOS architecture) or None for the host library. Every x86-64 target gets
    the fixed x86-64-v2 flag; the Android x86-64 ABI already requires it.

    The caller's CARGO_ENCODED_RUSTFLAGS, which cargo prefers, or else RUSTFLAGS
    applies to host builds only, after the fixed flag so a caller can still
    override it. Cross builds ignore it: host-only flags such as
    `-C target-cpu=native` would describe the build machine, not the device.
    """
    arch = target.split("-")[0] if target else platform.machine().lower()
    fixed = X86_64_RUSTFLAGS.split() if arch in ("x86_64", "amd64") else []
    if target is not None:
        return fixed
    encoded = os.environ.get("CARGO_ENCODED_RUSTFLAGS")
    if encoded is not None:
        caller = encoded.split("\x1f") if encoded else []
    else:
        caller = os.environ.get("RUSTFLAGS", "").split()
    return [*fixed, *caller]


def cargo_env(
    target_dir: Path, target: str | None = None, turbo: str | None = None
) -> dict[str, str]:
    """Return the environment for cargo builds of a Rust target into `target_dir`.

    `target` None builds for the host; see `rustflags` for which compiler flags
    each kind of build receives. Turbo compile-time switches are removed from
    the caller's environment; `turbo` names a preset of scripts/build_turbo.py
    whose recipe is applied instead.
    """
    env = clean_env()
    if turbo is not None:
        env.update(
            (key, value)
            for key, value in turbo_environment(turbo).items()
            if key.startswith("TAPIRSCAN_")
        )
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
        cwd=ROOT,
        env=env,
        check=True,
    )
    return Path(env["CARGO_TARGET_DIR"]) / target / "release/libtapirscan.a"


def library_name() -> str:
    """Platform file name of the native library."""
    if sys.platform == "win32":
        return "tapirscan.dll"
    return "libtapirscan.dylib" if sys.platform == "darwin" else "libtapirscan.so"


def apple_link_arguments(name: str) -> list[str]:
    """Return `rustc` arguments for an Apple library that apps rebundle.

    Dart, Flutter and Xcode rewrite the install name with install_name_tool,
    which needs header padding.
    """
    return [
        "-C",
        f"link-arg=-Wl,-install_name,@rpath/{name}",
        "-C",
        "link-arg=-Wl,-headerpad_max_install_names",
    ]


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


def build(
    public_crate: Path,
    destination: Path,
    turbo: str | None = None,
    *,
    identity: dict[str, object] | None = None,
) -> None:
    """Build, test and install the native library and the Rust examples."""
    if identity is None:
        identity = source_identity(ROOT)
    native = prepare_native_crate(public_crate)
    env = cargo_env(TARGET, turbo=turbo)
    env["RUSTUP_TOOLCHAIN"] = cast("str", identity["toolchain"])
    common = ["--offline", "--manifest-path", str(native / "Cargo.toml")]
    subprocess.run(["cargo", "test", *common], cwd=ROOT, env=env, check=True)
    name = library_name()
    command = ["cargo", "rustc", "--release", "--lib", "--message-format=json", *common]
    if sys.platform == "darwin":
        command += ["--", *apple_link_arguments(name)]
    elif sys.platform.startswith("linux"):
        command += ["--", "-C", f"link-arg=-Wl,-soname,{name}"]
    completed = subprocess.run(
        command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE, check=False
    )
    for line in completed.stdout.splitlines():
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            rendered = message.get("message", {}).get("rendered")
            if rendered:
                print(rendered, end="", file=sys.stderr)
    completed.check_returncode()
    artifact = cargo_artifact(completed.stdout, native, name)
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
        cwd=ROOT,
        env=env,
        check=True,
    )
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copy2(artifact, destination / name)
    if sys.platform == "win32":
        shutil.copy2(
            artifact.with_name("tapirscan.dll.lib"), destination / "tapirscan.dll.lib"
        )
    record(ROOT, destination / name, identity, turbo, build_metadata(env))
    print(f"Built {destination / name}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "build/native",
        help="Output directory (build_turbo.py uses a separate one)",
    )
    parser.add_argument(
        "--turbo",
        choices=TURBO_TIERS,
        help="Turbo preset recipe; used by build_turbo.py",
    )
    args = parser.parse_args()
    public = ROOT / "build/crates/tapirscan"
    identity = source_identity(ROOT)
    with prepared_package(public, refresh=True):
        build(public, args.output, args.turbo, identity=identity)

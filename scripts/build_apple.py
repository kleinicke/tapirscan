#!/usr/bin/env python3
"""Build TapirscanC.xcframework (macOS, iOS, iOS simulator) for the Swift package."""

import argparse
import os
import plistlib
import shutil
import subprocess
from pathlib import Path

from build_native import X86_64_RUSTFLAGS, prepare_native_crate
from prepare_rust import prepared_package

from build import ROOT

TARGET = ROOT / "build/apple-target"
# Slices per platform; x86-64 slices use the same baseline as the native library.
# Each entry: XCFramework library identifier, platform, variant, Rust targets.
PLATFORMS = [
    (
        "macos-arm64_x86_64",
        "macos",
        None,
        ["aarch64-apple-darwin", "x86_64-apple-darwin"],
    ),
    ("ios-arm64", "ios", None, ["aarch64-apple-ios"]),
    (
        "ios-arm64_x86_64-simulator",
        "ios",
        "simulator",
        ["aarch64-apple-ios-sim", "x86_64-apple-ios"],
    ),
]
MODULE_MAP = """module TapirscanC {
    header "tapirscan.h"
    export *
}
"""


def static_library(native: Path, target: str) -> Path:
    """Build the C ABI crate as a static library for one Rust target."""
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    if target.startswith("x86_64"):
        env["RUSTFLAGS"] = X86_64_RUSTFLAGS
    env["CARGO_TARGET_DIR"] = str(TARGET)
    env["CARGO_INCREMENTAL"] = "0"
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
    return TARGET / target / "release/libtapirscan.a"


def build(public_crate: Path, output: Path) -> None:
    """Build every slice, merge architectures and assemble the XCFramework.

    The bundle is written directly (directory layout plus Info.plist), so no
    Xcode first-launch setup is needed.
    """
    targets = [target for *_, slices in PLATFORMS for target in slices]
    subprocess.run(
        ["rustup", "target", "add", *targets], check=True, capture_output=True
    )
    native = prepare_native_crate(public_crate)
    shutil.rmtree(output, ignore_errors=True)
    libraries = []
    for identifier, platform, variant, slices in PLATFORMS:
        directory = output / identifier
        headers = directory / "Headers"
        headers.mkdir(parents=True)
        for header in (ROOT / "bindings/c/include").glob("*.h"):
            shutil.copy2(header, headers / header.name)
        (headers / "module.modulemap").write_text(MODULE_MAP)
        built = [str(static_library(native, target)) for target in slices]
        subprocess.run(
            ["lipo", "-create", *built, "-output", str(directory / "libtapirscan.a")],
            check=True,
        )
        library = {
            "LibraryIdentifier": identifier,
            "LibraryPath": "libtapirscan.a",
            "HeadersPath": "Headers",
            "SupportedArchitectures": [
                "arm64" if target.startswith("aarch64") else "x86_64"
                for target in slices
            ],
            "SupportedPlatform": platform,
        }
        if variant:
            library["SupportedPlatformVariant"] = variant
        libraries.append(library)
    with (output / "Info.plist").open("wb") as plist:
        plistlib.dump(
            {
                "AvailableLibraries": libraries,
                "CFBundlePackageType": "XFWK",
                "XCFrameworkFormatVersion": "1.0",
            },
            plist,
        )
    print(f"Built {output}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "bindings/swift/TapirscanC.xcframework",
        help="XCFramework destination",
    )
    args = parser.parse_args()
    public = ROOT / "build/crates/tapirscan"
    with prepared_package(public, refresh=public.exists()):
        build(public, args.output)

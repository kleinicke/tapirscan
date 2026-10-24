#!/usr/bin/env python3
"""Build the Android library (AAR with arm64-v8a and x86_64) using the Android NDK."""

import argparse
import os
import platform
import shutil
import subprocess
import zipfile
from pathlib import Path

from build_java import tool
from build_native import cargo_env, prepare_native_crate, static_library
from prepare_rust import prepared_package

from build import ROOT

TARGET = ROOT / "build/android-target"
OUT = ROOT / "build/android"
ANDROID = ROOT / "bindings/android"
# ABI and Rust target, which also names the NDK clang. Native ABI 7 supports
# 64-bit targets only.
ABIS = [
    ("arm64-v8a", "aarch64-linux-android"),
    ("x86_64", "x86_64-linux-android"),
]
MIN_API = 24
MANIFEST = """<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="org.tapirscan.android">
    <uses-sdk android:minSdkVersion="24" />
</manifest>
"""
# R8 must keep the JNI entry points and the constructors native code calls.
PROGUARD = """-keepclasseswithmembernames class org.tapirscan.android.** {
    native <methods>;
}
-keep class org.tapirscan.android.Barcode { <init>(...); }
-keep class org.tapirscan.android.ScannerException { <init>(...); }
"""


def toolchain(ndk: Path) -> Path:
    """Return the NDK's prebuilt LLVM toolchain directory for this host."""
    host = {
        "darwin": "darwin-x86_64",
        "linux": "linux-x86_64",
        "win32": "windows-x86_64",
    }
    system = platform.system().lower()
    return (
        ndk
        / "toolchains/llvm/prebuilt"
        / host["win32" if system == "windows" else system]
    )


def build(public_crate: Path, ndk: Path) -> Path:
    """Build the C library per ABI, link the JNI bridge into it and package an AAR."""
    tools = toolchain(ndk) / "bin"
    subprocess.run(
        ["rustup", "target", "add", *[target for _, target in ABIS]],
        check=True,
        capture_output=True,
    )
    native = prepare_native_crate(public_crate)
    libraries = OUT / "jni"
    shutil.rmtree(OUT, ignore_errors=True)
    for abi, target in ABIS:
        clang = tools / f"{target}{MIN_API}-clang"
        # The Android x86-64 ABI already requires SSE4.2 and POPCNT.
        env = cargo_env(TARGET, target)
        env[f"CC_{target.replace('-', '_')}"] = str(clang)
        env[f"AR_{target.replace('-', '_')}"] = str(tools / "llvm-ar")
        library = static_library(native, target, env)
        output = libraries / abi / "libtapirscan_jni.so"
        output.parent.mkdir(parents=True)
        subprocess.run(
            [
                str(clang),
                "-shared",
                "-fPIC",
                "-O2",
                "-Wall",
                "-Wextra",
                "-Werror",
                f"-I{ROOT / 'bindings/c/include'}",
                str(ANDROID / "src/main/c/tapirscan_jni.c"),
                str(library),
                "-Wl,--gc-sections",
                # Keep the Rust runtime's symbols out of the app's dynamic namespace.
                "-Wl,--exclude-libs,ALL",
                "-Wl,-z,max-page-size=16384",
                "-s",
                "-llog",
                "-lm",
                "-ldl",
                "-o",
                str(output),
            ],
            check=True,
        )
    classes = OUT / "classes"
    sources = sorted(str(p) for p in (ANDROID / "src/main/java").rglob("*.java"))
    subprocess.run(
        [
            tool("javac"),
            "--release",
            "11",
            "-Xlint:all",
            "-Werror",
            "-d",
            str(classes),
            *sources,
        ],
        check=True,
    )
    aar = OUT / "tapirscan-android.aar"
    with zipfile.ZipFile(aar, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("AndroidManifest.xml", MANIFEST)
        archive.writestr("R.txt", "")
        archive.writestr("proguard.txt", PROGUARD)
        with zipfile.ZipFile(OUT / "classes.jar", "w", zipfile.ZIP_DEFLATED) as jar:
            for path in sorted(classes.rglob("*.class")):
                jar.write(path, path.relative_to(classes).as_posix())
        archive.write(OUT / "classes.jar", "classes.jar")
        for path in sorted(libraries.rglob("*.so")):
            archive.write(path, path.relative_to(OUT).as_posix())
    print(f"Built {aar}", flush=True)
    return aar


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--ndk",
        type=Path,
        default=os.environ.get("ANDROID_NDK_HOME")
        or os.environ.get("ANDROID_NDK_ROOT"),
        help="Android NDK directory (default: ANDROID_NDK_HOME)",
    )
    args = parser.parse_args()
    if not args.ndk:
        parser.error("set ANDROID_NDK_HOME or pass --ndk")
    public = ROOT / "build/crates/tapirscan"
    with prepared_package(public, refresh=True):
        build(public, Path(args.ndk))

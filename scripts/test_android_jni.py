#!/usr/bin/env python3
"""Build the Android JNI bridge for the host JVM and run its test (no Android SDK)."""

import os
import subprocess
import sys
from pathlib import Path

from build_java import tool
from build_native import library_name

from build import ROOT

ANDROID = ROOT / "bindings/android"
OUT = ROOT / "build/android-host"


def main() -> None:
    """Compile the bridge against the JDK headers and native library, then test it."""
    java_home = Path(
        os.environ.get("JAVA_HOME") or Path(tool("javac")).resolve().parents[1]
    )
    native = ROOT / "build/native"
    if not (native / library_name()).is_file():
        sys.exit("Build the native library first: python3 scripts/build_native.py")
    OUT.mkdir(parents=True, exist_ok=True)
    platform = "darwin" if sys.platform == "darwin" else "linux"
    bridge = OUT / (
        "libtapirscan_jni.dylib" if sys.platform == "darwin" else "libtapirscan_jni.so"
    )
    subprocess.run(
        [
            os.environ.get("CC", "cc"),
            "-shared",
            "-fPIC",
            "-Wall",
            "-Wextra",
            "-Werror",
            f"-I{java_home / 'include'}",
            f"-I{java_home / 'include' / platform}",
            f"-I{ROOT / 'bindings/c/include'}",
            str(ANDROID / "src/main/c/tapirscan_jni.c"),
            f"-L{native}",
            "-ltapirscan",
            f"-Wl,-rpath,{native}",
            "-o",
            str(bridge),
        ],
        check=True,
    )
    sources = sorted(str(p) for p in (ANDROID / "src").rglob("*.java"))
    classes = OUT / "classes"
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
    subprocess.run(
        [
            tool("java"),
            "--enable-native-access=ALL-UNNAMED",
            f"-Djava.library.path={OUT}",
            "-cp",
            str(classes),
            "org.tapirscan.android.JniTest",
        ],
        check=True,
    )


if __name__ == "__main__":
    main()

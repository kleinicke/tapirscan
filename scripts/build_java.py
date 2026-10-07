#!/usr/bin/env python3
"""Compile the dependency-free Java binding and package a JAR. Requires JDK 22+."""

import json
import os
import shutil
import subprocess
from pathlib import Path

from build_native import library_name

from build import ROOT

VERSION = json.loads((ROOT / "bindings/javascript/package.json").read_text())["version"]
JAR = ROOT / f"build/java/tapirscan-{VERSION}.jar"


def tool(name: str) -> str:
    """Resolve a JDK executable from JAVA_HOME or PATH."""
    configured = os.environ.get("JAVA_HOME")
    return (
        str(Path(configured) / "bin" / name)
        if configured
        else shutil.which(name) or name
    )


if __name__ == "__main__":
    # Fresh directories, so classes deleted from the sources never reach the JAR.
    classes = ROOT / "build/java/classes"
    tests = ROOT / "build/java/test-classes"
    for stale in (classes, tests):
        shutil.rmtree(stale, ignore_errors=True)
    classes.mkdir(parents=True)
    metadata = classes / "META-INF"
    metadata.mkdir(exist_ok=True)
    shutil.copy2(ROOT / "LICENSE", metadata / "LICENSE")
    tests.mkdir(parents=True)
    sources = sorted((ROOT / "bindings/java/src/main/java").rglob("*.java"))
    subprocess.run(
        [tool("javac"), "--release", "22", "-d", str(classes), *map(str, sources)],
        check=True,
    )
    subprocess.run(
        [
            tool("jar"),
            "--create",
            "--file",
            str(JAR),
            "-C",
            str(classes),
            ".",
        ],
        check=True,
    )
    sources = sorted((ROOT / "bindings/java/src/test/java").rglob("*.java"))
    subprocess.run(
        [
            tool("javac"),
            "--release",
            "22",
            "-cp",
            str(classes),
            "-d",
            str(tests),
            *map(str, sources),
        ],
        check=True,
    )

    subprocess.run(
        [
            tool("java"),
            "--enable-native-access=ALL-UNNAMED",
            f"-Dtapirscan.library={ROOT / 'build/native' / library_name()}",
            "-cp",
            os.pathsep.join([str(classes), str(tests)]),
            "org.tapirscan.ApiTest",
        ],
        check=True,
    )

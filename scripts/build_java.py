#!/usr/bin/env python3
"""Compile the dependency-free Java binding and package a JAR. Requires JDK 22+."""

import os
import shutil
import subprocess
from pathlib import Path

from build import ROOT


def tool(name: str) -> str:
    """Resolve a JDK executable from JAVA_HOME or PATH."""
    configured = os.environ.get("JAVA_HOME")
    return (
        str(Path(configured) / "bin" / name)
        if configured
        else shutil.which(name) or name
    )


if __name__ == "__main__":
    classes = ROOT / "build/java/classes"
    classes.mkdir(parents=True, exist_ok=True)
    metadata = classes / "META-INF"
    metadata.mkdir(exist_ok=True)
    shutil.copy2(ROOT / "LICENSE", metadata / "LICENSE")
    tests = ROOT / "build/java/test-classes"
    tests.mkdir(parents=True, exist_ok=True)
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
            str(ROOT / "build/java/tapirscan-1.0.0.jar"),
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

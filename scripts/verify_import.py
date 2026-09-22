#!/usr/bin/env python3
"""Verify imported hashes and the reversible release-owned source revision."""

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
historical_only = "--historical-only" in sys.argv[1:]


def imported_path(relative: str) -> Path:
    """Original import manifests retain their original, hash-bound path names."""
    return (
        root / "historical" / relative
        if (relative.startswith("core/") or relative == "provenance/modes.json")
        else root / relative
    )


manifest = json.loads((root / "provenance/import.json").read_text())
expected_files = dict(manifest["files"])
if revision_path := manifest.get("releaseRevision"):
    revision = json.loads((root / revision_path).read_text())
    patch = root / revision["patch"]
    if hashlib.sha256(patch.read_bytes()).hexdigest() != revision["patchSha256"]:
        msg = "Release source patch hash mismatch"
        raise SystemExit(msg)
    if revision["baseHashes"] != {
        rel: manifest["files"][rel] for rel in revision["targetHashes"]
    }:
        msg = "Release revision does not match the imported base hashes"
        raise SystemExit(msg)
    expected_files.update(revision["targetHashes"])
    # Reverse the exact patch and prove it restores the imported snapshot.
    with tempfile.TemporaryDirectory(prefix="tapirscan-provenance-") as temporary:
        restored = Path(temporary)
        for rel in revision["targetHashes"]:
            dest = restored / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(imported_path(rel), dest)
        subprocess.run(
            [
                "patch",
                "--batch",
                "--silent",
                "--fuzz=0",
                "--reverse",
                "-p1",
                "-i",
                str(patch),
            ],
            cwd=restored,
            check=True,
        )
        for rel, expected in revision["baseHashes"].items():
            if hashlib.sha256((restored / rel).read_bytes()).hexdigest() != expected:
                msg = f"Release patch does not restore imported source: {rel}"
                raise SystemExit(msg)
for relative, expected in expected_files.items():
    actual = hashlib.sha256(imported_path(relative).read_bytes()).hexdigest()
    if actual != expected:
        msg = f"Source hash mismatch: {relative}"
        raise SystemExit(msg)
selection = json.loads((root / "provenance/modes.json").read_text())
if historical := selection.get("historicalRevision"):
    history = json.loads((root / historical).read_text())
    for relative, expected in history["files"].items():
        if hashlib.sha256((root / relative).read_bytes()).hexdigest() != expected:
            msg = f"Historical source hash mismatch: {relative}"
            raise SystemExit(msg)
runtime = json.loads((root / selection["runtimeRevision"]).read_text())
for relative, expected in ({} if historical_only else runtime["files"]).items():
    if hashlib.sha256((root / relative).read_bytes()).hexdigest() != expected:
        msg = f"Release runtime hash mismatch: {relative}"
        raise SystemExit(msg)
if (root / "config/formats.json").exists():
    subprocess.run(
        [sys.executable, str(root / "scripts/generate_formats.py"), "--check"],
        check=True,
    )
if historical_only:
    print(
        f"Verified {len(expected_files)} frozen historical source hashes; "
        "building current production sources"
    )
else:
    print(
        f"Verified {len(expected_files)} historical and "
        f"{len(runtime['files'])} production source hashes"
    )

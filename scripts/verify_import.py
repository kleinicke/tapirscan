#!/usr/bin/env python3
"""Verify imported hashes and the reversible release-owned source revision."""

import hashlib
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
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
            shutil.copyfile(root / rel, dest)
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
    actual = hashlib.sha256((root / relative).read_bytes()).hexdigest()
    if actual != expected:
        msg = f"Source hash mismatch: {relative}"
        raise SystemExit(msg)
runtime = json.loads((root / "provenance/runtime-refactor-20260921.json").read_text())
for relative, expected in runtime["files"].items():
    if hashlib.sha256((root / relative).read_bytes()).hexdigest() != expected:
        msg = f"Release runtime hash mismatch: {relative}"
        raise SystemExit(msg)
print(
    f"Verified {len(expected_files)} imported and {len(runtime['files'])} runtime "
    "source hashes and release revision"
)

#!/usr/bin/env python3
"""Verify that imported scanner snapshots retain their pinned hashes."""

import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[1]
manifest = json.loads((root / "provenance/import.json").read_text())
for relative, expected in manifest["files"].items():
    actual = hashlib.sha256((root / relative).read_bytes()).hexdigest()
    if actual != expected:
        msg = f"Imported source changed: {relative}"
        raise SystemExit(msg)
print(f"Verified {len(manifest['files'])} imported source hashes")

#!/usr/bin/env python3
"""Verify the repository boundary and generated format declarations."""

import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]

boundary = root / "scripts/check_repository_boundary.py"
if boundary.exists():
    subprocess.run([sys.executable, str(boundary)], check=True)
if (root / "config/formats.json").exists():
    subprocess.run(
        [sys.executable, str(root / "scripts/generate_formats.py"), "--check"],
        check=True,
    )
print("Verified repository boundary and generated formats")

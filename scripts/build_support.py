"""Shared helpers for verifying pinned build inputs."""

import hashlib
from collections.abc import Mapping
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def normalized_source_bytes(
    path: Path,
    relative: str,
    *,
    checkout_root: Path = ROOT,
) -> bytes:
    """Read bytes using the normalization defined by experiment manifests."""
    data = path.read_bytes()
    if relative == "Cargo.toml":
        data = data.replace(b"\r\n", b"\n")
        data = data.replace(
            (checkout_root / "multiformat").as_posix().encode(),
            b"@MULTIFORMAT@",
        )
    return data


def verify_source_hashes(
    root: Path,
    hashes: Mapping[str, str],
    *,
    error_prefix: str = "Source hash mismatch",
    checkout_root: Path = ROOT,
) -> None:
    """Check source files against pinned, recipe-normalized content hashes."""
    for relative, expected in hashes.items():
        actual = hashlib.sha256(
            normalized_source_bytes(
                root / relative,
                relative,
                checkout_root=checkout_root,
            )
        ).hexdigest()
        if actual != expected:
            msg = f"{error_prefix}: {root / relative}"
            raise RuntimeError(msg)

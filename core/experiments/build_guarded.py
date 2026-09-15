#!/usr/bin/env python3
"""Rebuild a pinned mode experiment without changing its base crate."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


HERE = Path(__file__).resolve().parent
BASE = HERE.parent
GIB = 1024 ** 3


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def require_space(path: Path) -> None:
    if shutil.disk_usage(path).free < 10 * GIB:
        raise RuntimeError(f"need a 10 GiB free-space reserve at {path}")


def verify_hashes(root: Path, hashes: dict[str, str], label: str) -> None:
    for relative, expected in hashes.items():
        actual = sha256(root / relative)
        if actual != expected:
            raise RuntimeError(f"{label} hash mismatch for {relative}: {actual} != {expected}")


def copy_declared_sources(destination: Path, files: list[str]) -> None:
    for relative in files:
        source = BASE / relative
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target, follow_symlinks=False)


def run(command: list[str], env: dict[str, str], cwd: Path | None = None) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, check=True, env=env, cwd=cwd)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True, type=Path, help="new, empty experiment output directory")
    parser.add_argument("--prepare-only", action="store_true", help="apply and verify the patch but do not compile")
    parser.add_argument("--recipe", choices=["medium-detail-20260914", "high-detail-20260914", "very-high-detail-20260914", "medium-evidence64-pinned-20260914", "medium-evidence64-20260914", "guarded-quality", "sampling-inline", "very-fast", "very-fast-v2", "very-fast-v3", "nano", "high-effort-transfer", "nano-lint-20260913", "very-fast-v2-lint-20260913", "guarded-quality-lint-20260913", "high-effort-transfer-lint-20260913"], default="guarded-quality-lint-20260913")
    args = parser.parse_args()
    manifest_file = HERE / (args.recipe + ".json")
    manifest = json.loads(manifest_file.read_text())
    manifest_hash = sha256(manifest_file)
    patch_hash = sha256(HERE / manifest["patch"])
    helper_hash = sha256(Path(__file__).resolve())
    if patch_hash != manifest["patchSha256"]:
        raise RuntimeError(f"patch hash mismatch: {patch_hash} != {manifest['patchSha256']}")
    out = args.out.resolve()
    if out.exists():
        raise RuntimeError(f"refusing to overwrite existing output: {out}")
    out.parent.mkdir(parents=True, exist_ok=True)
    require_space(out.parent)
    verify_hashes(BASE, manifest["baseHashes"], "base")

    temporary = out / "temporarysource"
    temporary.mkdir(parents=True)
    copy_declared_sources(temporary, manifest["copyFiles"])
    run(["patch", "--batch", "--forward", "-p1", "-i", str(HERE / manifest["patch"])], os.environ.copy(), temporary)
    verify_hashes(temporary, manifest["targetHashes"], "patched target")
    (out / "reproduction.json").write_text(json.dumps({
        "manifest": manifest,
        "manifestSha256": manifest_hash,
        "patchSha256": patch_hash,
        "helperSha256": helper_hash,
        "base": str(BASE),
        "temporarySource": str(temporary),
        "verification": "prepared-only" if args.prepare_only else "pending",
    }, indent=2) + "\n")
    if args.prepare_only:
        return 0

    require_space(out.parent)
    target = out / "cargo-target"
    test_environment = os.environ.copy() | {
        "CARGO_INCREMENTAL": "0",
        "CARGO_TARGET_DIR": str(target),
    }
    if toolchain := manifest.get("rustToolchain"):
        test_environment["RUSTUP_TOOLCHAIN"] = toolchain
    test_environment.pop("RUSTFLAGS", None)
    build_environment = test_environment | {"RUSTFLAGS": "-C target-feature=+simd128"}
    manifest_path = str(temporary / "Cargo.toml")
    run(["cargo", "test", "--manifest-path", manifest_path, "--all-targets", "--features", manifest.get("testFeature", "guarded-quality"), "--quiet"], test_environment)
    verify_hashes(BASE, manifest["baseHashes"], "post-test base")
    verify_hashes(temporary, manifest["targetHashes"], "post-test target")
    require_space(out.parent)
    run(["cargo", "build", "--manifest-path", manifest_path, "--lib", "--release", "--target", "wasm32-unknown-unknown", "--features", ",".join(manifest["expandedFeatures"])], build_environment)
    verify_hashes(BASE, manifest["baseHashes"], "post-build base")
    verify_hashes(temporary, manifest["targetHashes"], "post-build target")
    wasm = target / "wasm32-unknown-unknown" / "release" / "barcode_research_core.wasm"
    actual = sha256(wasm)
    if actual != manifest["expectedWasmSha256"]:
        raise RuntimeError(f"WASM hash mismatch: {actual} != {manifest['expectedWasmSha256']}")
    if sha256(manifest_file) != manifest_hash or sha256(HERE / manifest["patch"]) != patch_hash or sha256(Path(__file__).resolve()) != helper_hash:
        raise RuntimeError("reproduction helper, manifest or patch changed during build")
    shutil.copy2(wasm, out / (args.recipe + ".wasm"))
    record = json.loads((out / "reproduction.json").read_text())
    record["verification"] = "passed"
    record["wasmSha256"] = actual
    (out / "reproduction.json").write_text(json.dumps(record, indent=2) + "\n")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (RuntimeError, subprocess.CalledProcessError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)

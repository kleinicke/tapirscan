#!/usr/bin/env python3
"""Prepare/test a pinned scanner mode without mutating the source snapshot."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
MODE_CONFIG = json.loads((ROOT / "provenance/modes.json").read_text())["modes"]
MODES = {m["mode"]: (m["recipe"], m["tag"]) for m in MODE_CONFIG}


def wasm_flags() -> str:
    """Canonicalize std source paths whether rust-src is installed or absent."""
    sysroot = subprocess.check_output(
        ["rustc", "+1.91.1", "--print", "sysroot"],
        text=True,
    ).strip()
    commit = "ed61e7d7e242494fb7057f2657300d9e77bb4fcb"
    return (
        "-C target-feature=+simd128 "
        f"--remap-path-prefix={sysroot}/lib/rustlib/src/rust/library="
        f"/rustc/{commit}/library"
    )


def distribution_hash(recipe: str) -> str:
    """Read the canonical release hash separately from original import recipes."""
    return str(next(m["binarySha256"] for m in MODE_CONFIG if m["recipe"] == recipe))


def facade_manifest(mode: str, manifest: dict[str, Any]) -> str:
    """Select host policy alongside the exact compiled implementation."""
    text = (ROOT / "bindings/rust/Cargo.toml.in").read_text()
    return (
        text.replace("@FEATURES@", json.dumps(manifest["expandedFeatures"]))
        .replace("@MODE@", mode)
        .replace("@ROOT@", ROOT.as_posix())
        .replace(
            "@LOW_FEATURES@",
            json.dumps(
                json.loads(
                    (ROOT / "core/experiments/nano-clippy-20260916.json").read_text()
                )["expandedFeatures"]
            ),
        )
    )


def prepare_native_source(out: Path) -> None:
    """Expose one existing safe helper in a separate native-only source copy."""
    dest = out / "native-core"
    shutil.copytree(out / "temporarysource", dest, dirs_exist_ok=True)
    path = dest / "src/stripes.rs"
    text = path.read_text()
    old = "pub(crate) fn detect_secondary("
    if text.count(old) != 1:
        msg = "Unexpected secondary localizer visibility"
        raise RuntimeError(msg)
    path.write_text(text.replace(old, "pub fn detect_secondary("))
    (out / "native-adapter.json").write_text(
        json.dumps(
            {
                "change": "Expose detect_secondary; visibility-only adapter",
                "upstreamSha256": hashlib.sha256(
                    (out / "temporarysource/src/stripes.rs").read_bytes()
                ).hexdigest(),
                "nativeSha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            },
            indent=2,
        )
        + "\n"
    )


def prepare_recovery_source(out: Path) -> None:
    """Keep the pinned Low feature set separate from the primary crate."""
    dest = out / "recovery"
    if not dest.exists():
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "core/experiments/build_guarded.py"),
                "--recipe",
                "nano-clippy-20260916",
                "--out",
                str(dest),
                "--prepare-only",
            ],
            check=True,
        )
    source = dest / "temporarysource"
    recipe = json.loads(
        (ROOT / "core/experiments/nano-clippy-20260916.json").read_text()
    )
    for rel, expected in (recipe["baseHashes"] | recipe["targetHashes"]).items():
        if hashlib.sha256((source / rel).read_bytes()).hexdigest() != expected:
            msg = f"Recovery source hash mismatch: {rel}"
            raise RuntimeError(msg)
    copied = out / "recovery-core"
    shutil.copytree(source, copied, dirs_exist_ok=True)
    manifest = copied / "Cargo.toml"
    manifest.write_text(
        manifest.read_text().replace(
            'name = "barcode-research-core"', 'name = "tapirscan-recovery-core"', 1
        )
    )
    # Both crates expose the same WASM C symbols. Recovery is only called through
    # Rust here, so keep its symbols mangled to avoid duplicate native exports.
    for path in (copied / "src").rglob("*.rs"):
        text = path.read_text()
        if "#[no_mangle]" in text:
            path.write_text(text.replace("#[no_mangle]", ""))
    (out / "recovery-adapter.json").write_text(
        json.dumps(
            {
                "change": "Isolate recovery features and native symbols",
                "recipe": "nano-clippy-20260916",
            },
            indent=2,
        )
        + "\n"
    )


def prepare_test_source(out: Path) -> Path:
    """Keep exact decisions but allow two rounding ULPs in a libm weight test."""
    if out.name == "low":
        return out / "temporarysource/Cargo.toml"
    copied = out / "test-core"
    shutil.copytree(out / "temporarysource", copied, dirs_exist_ok=True)
    path = copied / "src/stripes.rs"
    text = path.read_text()
    old = "assert_eq!(edge_weight(dx, dy, ax, ay), original);"
    replacement = """let actual = edge_weight(dx, dy, ax, ay);
                    assert_eq!(actual.is_some(), original.is_some());
                    if let (Some(actual), Some(expected)) = (actual, original) {
                        assert!((actual - expected).abs()
                            <= 2. * f64::EPSILON * expected.abs().max(1.));
                    }"""
    if text.count(old) != 1:
        msg = "Unexpected imported edge-weight test"
        raise RuntimeError(msg)
    path.write_text(text.replace(old, replacement))
    (out / "test-adapter.json").write_text(
        json.dumps(
            {
                "change": "Test-only libm tolerance; decisions remain exact",
                "upstreamSha256": hashlib.sha256(text.encode()).hexdigest(),
                "testSha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            },
            indent=2,
        )
        + "\n"
    )
    return copied / "Cargo.toml"


def resume_core(out: Path, recipe: str) -> None:
    """Verify and finish an existing core build without trusting stale artifacts."""
    manifest = json.loads((ROOT / f"core/experiments/{recipe}.json").read_text())
    for base, hashes in [
        (ROOT / "core", manifest["baseHashes"]),
        (
            out / "temporarysource",
            dict(manifest["baseHashes"]) | manifest["targetHashes"],
        ),
    ]:
        for rel, expected in hashes.items():
            if hashlib.sha256((base / rel).read_bytes()).hexdigest() != expected:
                msg = f"Source hash mismatch: {base / rel}"
                raise RuntimeError(msg)
    env = dict(
        os.environ,
        CARGO_TARGET_DIR=str(out / "cargo-target"),
        CARGO_INCREMENTAL="0",
        RUSTUP_TOOLCHAIN="1.91.1",
    )
    env.pop("RUSTFLAGS", None)
    core_manifest = str(out / "temporarysource/Cargo.toml")
    subprocess.run(
        [
            "cargo",
            "test",
            "--offline",
            "--manifest-path",
            str(prepare_test_source(out)),
            "--all-targets",
            "--features",
            manifest.get("testFeature", "guarded-quality"),
            "--quiet",
        ],
        env=env,
        check=True,
    )
    env["RUSTFLAGS"] = wasm_flags()
    subprocess.run(
        [
            "cargo",
            "build",
            "--offline",
            "--release",
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            core_manifest,
            "--features",
            ",".join(manifest["expandedFeatures"]),
        ],
        env=env,
        check=True,
    )
    wasm = (
        out / "cargo-target/wasm32-unknown-unknown/release/barcode_research_core.wasm"
    )
    if hashlib.sha256(wasm.read_bytes()).hexdigest() != distribution_hash(recipe):
        msg = "Resumed WASM hash mismatch"
        raise RuntimeError(msg)
    shutil.copy2(wasm, out / f"{recipe}.wasm")


def main() -> None:
    """Build the requested pinned scanner mode."""
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument(
        "--resume",
        action="store_true",
        help="Verify and finish an existing prepared build",
    )
    args = parser.parse_args()
    recipe, tag = MODES[args.mode]
    out = ROOT / "build" / args.mode
    command = [
        sys.executable,
        str(ROOT / "core/experiments/build_guarded.py"),
        "--recipe",
        recipe,
        "--out",
        str(out),
    ]
    command.append("--prepare-only")
    if not args.resume:
        subprocess.run(command, check=True)
    if not args.prepare_only:
        resume_core(out, recipe)
    manifest = json.loads((ROOT / f"core/experiments/{recipe}.json").read_text())
    prepare_native_source(out)
    prepare_recovery_source(out)
    sdk = out / "rust"
    shutil.copytree(ROOT / "bindings/rust/src", sdk / "src", dirs_exist_ok=True)
    shutil.copytree(
        ROOT / "bindings/rust/examples", sdk / "examples", dirs_exist_ok=True
    )
    (sdk / "Cargo.toml").write_text(facade_manifest(args.mode, manifest))
    if args.prepare_only:
        print(f"Prepared {tag}; Rust facade: {sdk}")
        return
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    env["CARGO_TARGET_DIR"] = str(out / "cargo-target")
    env["CARGO_INCREMENTAL"] = "0"
    subprocess.run(
        ["cargo", "test", "--offline", "--manifest-path", str(sdk / "Cargo.toml")],
        env=env,
        check=True,
    )
    subprocess.run(
        [
            "cargo",
            "build",
            "--offline",
            "--release",
            "--manifest-path",
            str(sdk / "Cargo.toml"),
            "--example",
            "scan_raw",
        ],
        env=env,
        check=True,
    )
    assets = ROOT / "bindings/javascript/wasm"
    assets.mkdir(exist_ok=True)
    source = out / f"{recipe}.wasm"
    actual = hashlib.sha256(source.read_bytes()).hexdigest()
    if actual != distribution_hash(recipe):
        msg = "Unexpected WASM hash"
        raise RuntimeError(msg)
    shutil.copy2(source, assets / f"{tag}.wasm")
    print(f"Built and verified {tag}: {actual}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Build mode-specific WASM adapters around the public Rust Scanner API."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

from prepare_rust import prepare

from build import MODES, ROOT, wasm_flags

MANIFEST = ROOT / json.loads((ROOT / "provenance/modes.json").read_text())["apiWasm"]
PACKAGE = ROOT / "build/crates/tapirscan"


def source_files() -> dict[str, str]:
    """Hash shared API, core and adapter inputs, independent of JS hosts."""
    imported = json.loads((ROOT / "provenance/import.json").read_text())
    selected = dict(imported["files"])
    if revision := imported.get("releaseRevision"):
        selected.update(json.loads((ROOT / revision).read_text())["targetHashes"])
    paths = {name for name in selected if name.startswith("multiformat/")}
    for folder in (
        "core/src",
        "bindings/rust",
        "bindings/wasm",
        "tools/package-source",
    ):
        paths.update(
            p.relative_to(ROOT).as_posix()
            for p in (ROOT / folder).rglob("*")
            if p.is_file() and p.suffix in {".rs", ".toml", ".in", ".lock"}
        )
    paths.update(
        [
            "core/Cargo.toml",
            "core/Cargo.lock",
            "scripts/build.py",
            "scripts/build_support.py",
            "scripts/prepare_rust.py",
            "scripts/build_wasm.py",
            "scripts/wasm_rustc.py",
            "config/formats.json",
        ]
    )
    return {
        name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
        for name in sorted(paths)
    }


def build_environment(root: Path = ROOT) -> dict[str, str]:
    """Use stable crate identities and paths, with cache keys tied to the wrapper."""
    env = dict(os.environ, RUSTUP_TOOLCHAIN="1.91.1", CARGO_INCREMENTAL="0")
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    env.pop("RUSTFLAGS", None)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(
        [
            *wasm_flags(),
            f"--remap-path-prefix={root}=/tapirscan",
            f"--remap-path-prefix={cargo_home}=/cargo",
        ]
    )
    source = Path(__file__).with_name("wasm_rustc.py")
    wrapper_hash = hashlib.sha256(source.read_bytes()).hexdigest()[:16]
    wrapper = root / f"build/wasm-rustc/{wrapper_hash}.py"
    wrapper.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, wrapper)
    wrapper.chmod(0o755)
    if os.name == "nt":
        launcher = wrapper.with_suffix(".cmd")
        launcher.write_text(f'@"{sys.executable}" "{wrapper}" %*\n')
        wrapper = launcher
    env["RUSTC_WRAPPER"] = str(wrapper)
    env["CARGO_TARGET_DIR"] = str(root / "build/wasm-target")
    return env


def main() -> None:
    """Build and verify distribution assets; record new identities explicitly."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("modes", nargs="*", metavar="MODE")
    parser.add_argument(
        "--record",
        action="store_true",
        help="Record new artifact identities for a reviewed source revision",
    )
    args = parser.parse_args()
    args.modes = args.modes or list(MODES)
    if set(args.modes) - set(MODES):
        parser.error("modes must be low, medium, high or very-high")
    subprocess.run(
        ["python3", str(ROOT / "scripts/verify_import.py"), "--historical-only"],
        check=True,
    )
    prepare(PACKAGE, refresh=PACKAGE.exists())
    inputs = source_files()
    digest = hashlib.sha256(
        json.dumps(sorted(inputs.items()), separators=(",", ":")).encode()
    ).hexdigest()
    previous = json.loads(MANIFEST.read_text()) if MANIFEST.exists() else {}
    if not args.record and previous.get("sourceDigest") != digest:
        msg = "WASM source identity changed; validate then use --record"
        raise SystemExit(msg)
    records = {entry["mode"]: entry for entry in previous.get("modes", [])}
    env = build_environment()
    assets = ROOT / "bindings/javascript/wasm"
    assets.mkdir(parents=True, exist_ok=True)
    for mode in args.modes:
        out = ROOT / "build/wasm" / mode
        shutil.copytree(ROOT / "bindings/wasm/src", out / "src", dirs_exist_ok=True)
        template = (ROOT / "bindings/wasm/Cargo.toml.in").read_text()
        (out / "Cargo.toml").write_text(
            template.replace("@PUBLIC_CRATE@", PACKAGE.as_posix())
            .replace("@MODE@", mode)
            .replace("@MODE_ID@", str(list(MODES).index(mode)))
        )
        shutil.copy2(PACKAGE / "Cargo.lock", out / "Cargo.lock")
        subprocess.run(
            [
                "cargo",
                "build",
                "--offline",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
                "--manifest-path",
                str(out / "Cargo.toml"),
            ],
            env=env,
            check=True,
        )
        binary = (
            Path(env["CARGO_TARGET_DIR"])
            / "wasm32-unknown-unknown/release/tapirscan_wasm.wasm"
        )
        filename = f"{MODES[mode][1]}.wasm"
        actual = hashlib.sha256(binary.read_bytes()).hexdigest()
        if not args.record and records.get(mode, {}).get("sha256") != actual:
            msg = f"WASM reproducibility mismatch: {mode}"
            raise SystemExit(msg)
        shutil.copy2(binary, assets / filename)
        records[mode] = {
            "mode": mode,
            "file": filename,
            "sha256": actual,
            "bytes": binary.stat().st_size,
        }
        print(f"Built public Rust API WASM {mode}: {actual}", flush=True)
    if args.record:
        if previous.get("sourceDigest") not in (None, digest) and set(
            args.modes
        ) != set(MODES):
            msg = "Changed source requires rebuilding every mode before recording"
            raise SystemExit(msg)
        MANIFEST.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "apiVersion": 1,
                    "sourceDigest": digest,
                    "sourceFiles": inputs,
                    "modes": [records[mode] for mode in MODES if mode in records],
                },
                indent=2,
            )
            + "\n"
        )
    shutil.copy2(
        ROOT / "multiformat/THIRD_PARTY_NOTICES.md",
        ROOT / "bindings/javascript/THIRD_PARTY_NOTICES.md",
    )


if __name__ == "__main__":
    main()

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
from typing import TypedDict

from build_turbo import environment as turbo_environment
from prepare_rust import prepared_package, sync_tree, write_changed

from build import MODES, ROOT, wasm_flags

MANIFEST = ROOT / json.loads((ROOT / "provenance/modes.json").read_text())["apiWasm"]
PACKAGE = ROOT / "build/crates/tapirscan"


def source_paths(folder: Path) -> list[Path]:
    """Exclude Cargo output when collecting maintained Rust source inputs."""
    return [
        path
        for path in folder.rglob("*")
        if path.is_file()
        and path.suffix in {".rs", ".toml", ".in", ".lock"}
        and "target" not in path.relative_to(folder).parts
    ]


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
            p.relative_to(ROOT).as_posix() for p in source_paths(ROOT / folder)
        )
    paths.update(
        [
            "core/Cargo.toml",
            "core/Cargo.lock",
            "scripts/build.py",
            "scripts/prepare_rust.py",
            "scripts/build_wasm.py",
            "scripts/build_turbo.py",
            "scripts/wasm_rustc.py",
            "config/formats.json",
        ]
    )
    # modes.json selects output names and provenance records, not Rust source.
    # Its runtimeRevision changes on documentation-only snapshots. The package
    # verifier checks selected modes/presets against the built artifact records.
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


def arguments() -> argparse.Namespace:
    """Separate private development builds from immutable release recording."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("modes", nargs="*", metavar="MODE")
    parser.add_argument(
        "--record",
        action="store_true",
        help="Record new artifact identities for a reviewed source revision",
    )
    parser.add_argument(
        "--development",
        action="store_true",
        help="Build under build/ without modifying release identities",
    )
    args = parser.parse_args()
    if not args.development and any(
        key == "TAPIRSCAN_LOW_CLASSIC"
        or key.startswith(
            ("TAPIRSCAN_TURBO_", "TAPIRSCAN_EXPERIMENT_", "TAPIRSCAN_EXPERIMENTAL_")
        )
        for key in os.environ
    ):
        parser.error("private policy overrides require --development")
    if args.record and args.development:
        parser.error("choose release recording or development assets")
    args.modes = args.modes or list(MODES)
    if set(args.modes) - set(MODES):
        parser.error("modes must be low, medium, high or very-high")
    return args


class Variant(TypedDict):
    """One built WASM asset as recorded in an identity manifest."""

    mode: str
    file: str
    sha256: str
    bytes: int


def build_variant(
    mode: str,
    identity: str,
    destination: Path,
    env: dict[str, str],
    expected_hash: str | None,
) -> Variant:
    """Compile one isolated stable mode or experimental preset."""
    env["CARGO_TARGET_DIR"] = str(ROOT / "build/wasm-target" / identity)
    out = ROOT / "build/wasm" / identity
    sync_tree(ROOT / "bindings/wasm/src", out / "src")
    template = (ROOT / "bindings/wasm/Cargo.toml.in").read_text()
    write_changed(
        out / "Cargo.toml",
        (
            template.replace("@PUBLIC_CRATE@", PACKAGE.as_posix())
            .replace("@MODE@", mode)
            .replace("@MODE_ID@", str(list(MODES).index(mode)))
        ).encode(),
    )
    write_changed(out / "Cargo.lock", (PACKAGE / "Cargo.lock").read_bytes())
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
    actual = hashlib.sha256(binary.read_bytes()).hexdigest()
    if expected_hash is not None and expected_hash != actual:
        msg = f"WASM reproducibility mismatch: {mode}"
        raise SystemExit(msg)
    shutil.copy2(binary, destination)
    result: Variant = {
        "mode": mode,
        "file": destination.name,
        "sha256": actual,
        "bytes": binary.stat().st_size,
    }
    print(f"Built public Rust API WASM {destination.name}: {actual}", flush=True)
    return result


def main() -> None:
    """Build and verify distribution assets; record new identities explicitly."""
    args = arguments()
    manifest = (
        ROOT / "build/wasm-development/manifest.json" if args.development else MANIFEST
    )
    record = args.record or args.development
    with prepared_package(PACKAGE, refresh=PACKAGE.exists()):
        inputs = source_files()
        digest = hashlib.sha256(
            json.dumps(sorted(inputs.items()), separators=(",", ":")).encode()
        ).hexdigest()
        previous = json.loads(manifest.read_text()) if manifest.exists() else {}
        if not record and previous.get("sourceDigest") != digest:
            msg = "WASM source identity changed; validate then use --record"
            raise SystemExit(msg)
        changed = previous.get("sourceDigest") not in (None, digest)
        records = {entry["mode"]: entry for entry in previous.get("modes", [])}
        if args.development and changed:
            # Do not label other modes built from older source as current.
            records = {}
        env = build_environment()
        assets = (
            ROOT / "build/wasm-development/assets"
            if args.development
            else ROOT / "bindings/javascript/wasm"
        )
        assets.mkdir(parents=True, exist_ok=True)
        for mode in args.modes:
            filename = f"{mode}.wasm" if args.development else f"{MODES[mode][1]}.wasm"
            records[mode] = build_variant(
                mode,
                mode,
                assets / filename,
                env.copy(),
                None if record else records.get(mode, {}).get("sha256", ""),
            )
        experimental = previous.get("experimentalTurbo", [])
        if not args.development:
            selection = json.loads((ROOT / "provenance/modes.json").read_text())
            expected = {entry["preset"]: entry for entry in experimental}
            experimental = []
            for entry in selection["experimentalTurbo"]:
                tier = entry["preset"]
                variant_env = env.copy()
                policy = {
                    key: value
                    for key, value in turbo_environment(str(tier)).items()
                    if key.startswith("TAPIRSCAN_")
                }
                variant_env.update(policy)
                result = build_variant(
                    "low",
                    f"turbo{tier}",
                    assets / f"{entry['tag']}.wasm",
                    variant_env,
                    None if record else expected.get(tier, {}).get("sha256", ""),
                )
                experimental.append({**result, "preset": tier, "environment": policy})
        if record:
            if not args.development and changed and set(args.modes) != set(MODES):
                msg = "Changed source requires rebuilding every mode before recording"
                raise SystemExit(msg)
            manifest.parent.mkdir(parents=True, exist_ok=True)
            manifest.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "apiVersion": 2,
                        "sourceDigest": digest,
                        "sourceFiles": inputs,
                        "modes": [records[mode] for mode in MODES if mode in records],
                        **(
                            {"experimentalTurbo": experimental}
                            if not args.development
                            else {}
                        ),
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

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

from build import MODES, ROOT, TURBO_PRESETS, wasm_flags

MANIFEST = ROOT / "bindings/javascript/wasm/build.json"
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
    """Hash the Rust, build-script and configuration inputs of the WASM files."""
    paths: set[str] = set()
    for folder in (
        "core/src",
        "multiformat",
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
            # Mode order determines the adapters' mode IDs.
            "config/modes.json",
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


def arguments() -> argparse.Namespace:
    """Separate ordinary package builds from private development builds."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("modes", nargs="*", metavar="MODE")
    parser.add_argument(
        "--development",
        action="store_true",
        help="Build the selected modes under build/wasm-development without a manifest",
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
    if set(args.modes) - set(MODES):
        parser.error("modes must be low, medium, high or very-high")
    return args


class Variant(TypedDict):
    """One built WASM asset as recorded in the build manifest."""

    mode: str
    file: str
    sha256: str
    bytes: int


def build_variant(
    mode: str,
    identity: str,
    destination: Path,
    env: dict[str, str],
    expected_hash: str | None = None,
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


def source_digest(inputs: dict[str, str]) -> str:
    """Return the digest that the package verifier recomputes from the tree."""
    return hashlib.sha256(
        json.dumps(sorted(inputs.items()), separators=(",", ":")).encode()
    ).hexdigest()


def main() -> None:
    """Build the package's WASM files and describe them in ``build.json``."""
    args = arguments()
    with prepared_package(PACKAGE, refresh=PACKAGE.exists()):
        inputs = source_files()
        digest = source_digest(inputs)
        env = build_environment()
        if args.development:
            assets = ROOT / "build/wasm-development/assets"
            assets.mkdir(parents=True, exist_ok=True)
            for mode in args.modes or MODES:
                build_variant(mode, mode, assets / f"{mode}.wasm", env.copy())
        else:
            assets = MANIFEST.parent
            assets.mkdir(parents=True, exist_ok=True)
            previous = json.loads(MANIFEST.read_text()) if MANIFEST.exists() else {}
            # Records for other source revisions would mislabel older binaries.
            known = (
                previous.get("files", {})
                if previous.get("sourceDigest") == digest
                else {}
            )
            files = dict(known)
            targets = [(mode, mode, None) for mode in args.modes or MODES]
            if not args.modes:
                targets += [("low", f"turbo{p}", p) for p in TURBO_PRESETS]
            for mode, identity, preset in targets:
                name = mode if preset is None else f"experimental-turbo{preset}"
                variant_env = env.copy()
                entry: dict[str, object] = {"mode": mode}
                if preset is not None:
                    policy = {
                        key: value
                        for key, value in turbo_environment(str(preset)).items()
                        if key.startswith("TAPIRSCAN_")
                    }
                    variant_env.update(policy)
                    entry = {"mode": mode, "preset": preset, "environment": policy}
                result = build_variant(
                    mode,
                    identity,
                    assets / f"{name}.wasm",
                    variant_env,
                    known.get(f"{name}.wasm", {}).get("sha256"),
                )
                files[f"{name}.wasm"] = {
                    **entry,
                    "sha256": result["sha256"],
                    "bytes": result["bytes"],
                }
            MANIFEST.write_text(
                json.dumps(
                    {
                        "schema": 2,
                        "rustToolchain": "1.91.1",
                        "sourceDigest": digest,
                        "sourceFiles": inputs,
                        "files": dict(sorted(files.items())),
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

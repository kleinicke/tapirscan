#!/usr/bin/env python3
"""Assemble a self-contained Cargo package from maintained production sources."""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import cast

from build import MODE_CONFIG, ROOT


@dataclass
class Module:
    """Namespace and compile-time selection for one embedded crate."""

    namespace: str
    features: set[str]
    aliases: dict[str, str]


def copy_module(
    source: Path,
    destination: Path,
    module: Module,
    *,
    rewriter: Path,
) -> dict[str, bool]:
    """Relocate crate paths and select mode features; keep target cfgs intact."""
    files = {
        str(path.relative_to(source)): path.read_text()
        for path in sorted(source.rglob("*.rs"))
        if "bin" not in path.relative_to(source).parts
    }
    result = json.loads(
        subprocess.check_output(
            [str(rewriter)],
            input=json.dumps(
                {
                    "namespace": module.namespace,
                    "features": sorted(module.features),
                    "aliases": module.aliases,
                    "files": files,
                }
            ),
            text=True,
        )
    )
    destination.mkdir(parents=True)
    for relative, text in result["files"].items():
        target = destination / ("mod.rs" if relative == "lib.rs" else relative)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    fixtures = source.parent / "fixtures"
    if fixtures.exists():
        shutil.copytree(fixtures, destination.parent / "fixtures", dirs_exist_ok=True)

    return cast("dict[str, bool]", result["flags"])


def source_rewriter() -> Path:
    """Build the pinned token-aware module relocation tool once per preparation."""
    target = ROOT / "build/source-tool"
    subprocess.run(
        [
            "cargo",
            "+1.91.1",
            "build",
            "--offline",
            "--locked",
            "--manifest-path",
            str(ROOT / "tools/package-source/Cargo.toml"),
            "--target-dir",
            str(target),
        ],
        check=True,
    )
    suffix = ".exe" if sys.platform == "win32" else ""
    return target / "debug" / f"tapirscan-package-source{suffix}"


def assemble(destination: Path) -> None:
    """Generate a complete package before updating the live Cargo inputs."""
    destination.mkdir(parents=True, exist_ok=True)
    rewriter = source_rewriter()
    binding = ROOT / "bindings/rust"
    for name in ["Cargo.toml", "README.md"]:
        shutil.copy2(binding / name, destination / name)
    if (binding / "Cargo.lock").exists():
        shutil.copy2(binding / "Cargo.lock", destination / "Cargo.lock")
    shutil.copy2(ROOT / "LICENSE", destination / "LICENSE")
    shutil.copytree(binding / "api", destination / "api")
    shutil.copytree(binding / "tests", destination / "tests")
    shutil.copytree(binding / "examples", destination / "examples")
    generated = destination / "generated"
    modules = []
    flags = {}
    for mode in MODE_CONFIG:
        name = mode["mode"].replace("-", "_")
        features = {f"mode-{mode['mode']}"}
        core = f"core_{name}"
        flags.update(
            copy_module(
                ROOT / "core/src",
                generated / core,
                Module(core, features, {"barcode_multiformat": "multiformat"}),
                rewriter=rewriter,
            )
        )
        flags.update(
            copy_module(
                binding / "src",
                generated / name,
                Module(
                    name,
                    {mode["mode"]},
                    {
                        "barcode_research_core": core,
                        "recovery_core": "core_low",
                        "barcode_multiformat": "multiformat",
                        "scanner_types": "crate::types",
                        "scanner_timer": "crate::timer",
                    },
                ),
                rewriter=rewriter,
            )
        )
        gate = f"#[cfg(tapirscan_mode_{name})]"
        modules.extend(
            [
                f"pub(crate) mod {core};"
                if name == "low"
                else f"{gate}\npub(crate) mod {core};",
                f"{gate}\npub(crate) mod {name};",
            ]
        )
    flags.update(
        copy_module(
            ROOT / "multiformat/src",
            generated / "multiformat",
            Module("multiformat", set(), {}),
            rewriter=rewriter,
        )
    )
    modules.append("pub(crate) mod multiformat;")
    (generated / "mod.rs").write_text("\n".join(modules) + "\n")
    entries = [
        f'    ("{name}", {str(enabled).lower()}),'
        for name, enabled in sorted(flags.items())
    ]
    mode_entries = [
        f'    ("tapirscan_mode_{m["mode"].replace("-", "_")}", '
        f'"CARGO_FEATURE_MODE_{m["mode"].replace("-", "_").upper()}"),'
        for m in MODE_CONFIG
    ]
    (destination / "build.rs").write_text(
        "const FLAGS: &[(&str, bool)] = &[\n"
        + "\n".join(entries)
        + "\n];\n"
        + "const MODES: &[(&str, &str)] = &[\n"
        + "\n".join(mode_entries)
        + "\n];\n"
        + "fn main() {\n"
        + "    let explicit = MODES.iter()\n"
        + "        .any(|(_, feature)| std::env::var_os(feature).is_some());\n"
        + "    for (name, feature) in MODES {\n"
        + '        println!("cargo::rustc-check-cfg=cfg({name})");\n'
        + '        println!("cargo::rerun-if-env-changed={feature}");\n'
        + "        if !explicit || std::env::var_os(feature).is_some() {\n"
        + '            println!("cargo::rustc-cfg={name}");\n'
        + "        }\n    }\n"
        + '    println!("cargo::rerun-if-changed=build.rs");\n'
        + "    for (name, enabled) in FLAGS {\n"
        + '        println!("cargo::rustc-check-cfg=cfg({name})");\n'
        + '        if *enabled { println!("cargo::rustc-cfg={name}"); }\n'
        + "    }\n}\n"
    )
    provenance = {
        "modes": MODE_CONFIG,
        "adaptations": [
            "crate paths relocated into private modules",
            "mode features fixed as private namespaced cfg flags",
            "C exports use Rust symbol mangling",
            (
                "edge-weight test preserves exact decisions with "
                "4-epsilon relative weight tolerance across libm implementations"
            ),
            "low mode reused for recovery from the same maintained core",
        ],
        "sourceHashes": {
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for folder in [
                binding / "src",
                binding / "api",
                binding / "tests",
                binding / "examples",
                ROOT / "multiformat/src",
                ROOT / "core/src",
            ]
            for p in sorted(folder.rglob("*.rs"))
        },
        "packagingSourceHashes": {
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in [
                Path(__file__),
                binding / "Cargo.toml",
                binding / "Cargo.lock",
                binding / "README.md",
                ROOT / "LICENSE",
                ROOT / "tools/package-source/Cargo.toml",
                ROOT / "tools/package-source/Cargo.lock",
                ROOT / "tools/package-source/src/main.rs",
            ]
        },
        "buildScriptSha256": hashlib.sha256(
            (destination / "build.rs").read_bytes()
        ).hexdigest(),
        "generatedHashes": {
            str(p.relative_to(destination)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(generated.rglob("*"))
            if p.is_file()
        },
    }
    (destination / "provenance.json").write_text(
        json.dumps(provenance, indent=2) + "\n"
    )


def write_changed(path: Path, data: bytes) -> None:
    """Preserve Cargo input timestamps when generated content is unchanged."""
    if not path.exists() or path.read_bytes() != data:
        path.write_bytes(data)


def sync_tree(source: Path, destination: Path) -> None:
    """Update changed bytes only, removing obsolete generated files."""
    destination.mkdir(parents=True, exist_ok=True)
    for old in destination.iterdir():
        if not (source / old.name).exists():
            if old.is_dir():
                shutil.rmtree(old)
            else:
                old.unlink()
    for item in source.iterdir():
        target = destination / item.name
        if item.is_dir():
            sync_tree(item, target)
        elif not target.exists() or item.read_bytes() != target.read_bytes():
            shutil.copy2(item, target)


@contextmanager
def prepared_package(destination: Path, *, refresh: bool = False) -> Iterator[Path]:
    """Hold an exclusive package lock through preparation and its consuming build."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    lock = destination.with_name(destination.name + ".lock")
    try:
        lock.mkdir()
    except FileExistsError as error:
        msg = (
            f"Package is in use: {lock}. After interruption, "
            "verify its owner stopped before removing the lock."
        )
        raise RuntimeError(msg) from error
    try:
        (lock / "owner").write_text(f"{os.getpid()}\n")
        if destination.exists() and not refresh:
            raise FileExistsError(destination)
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/verify_import.py"),
                "--historical-only",
            ],
            check=True,
        )
        with tempfile.TemporaryDirectory(
            prefix="prepare-", dir=destination.parent
        ) as temporary:
            staged = Path(temporary)
            assemble(staged)
            destination.mkdir(parents=True, exist_ok=True)
            for item in staged.iterdir():
                target = destination / item.name
                if item.is_dir():
                    sync_tree(item, target)
                elif not target.exists() or item.read_bytes() != target.read_bytes():
                    shutil.copy2(item, target)
        print(destination)
        yield destination
    finally:
        shutil.rmtree(lock)


def prepare(destination: Path, *, refresh: bool = False) -> None:
    """Refresh generated inputs without touching unchanged files or Cargo caches."""
    with prepared_package(destination, refresh=refresh):
        pass


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "destination", type=Path, nargs="?", default=ROOT / "build/crates/tapirscan"
    )
    parser.add_argument(
        "--refresh",
        action="store_true",
        help="Verify history and refresh sources, keeping Cargo caches",
    )
    args = parser.parse_args()
    prepare(args.destination.resolve(), refresh=args.refresh)

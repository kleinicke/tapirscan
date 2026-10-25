#!/usr/bin/env python3
"""Assemble a self-contained Cargo package from maintained production sources."""

import argparse
import errno
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import time
from collections.abc import Iterator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO, cast

from build import MODES, ROOT, rust_toolchain

if sys.platform == "win32":
    import msvcrt
else:
    import fcntl

# Embedded copies of the core and multiformat crates keep their full library APIs;
# each mode's engine uses only part of them. Engine modules are linted normally.
LIBRARY_LINTS = "#[allow(dead_code, unused_imports)]"
# Seconds between attempts to take the package lock, and between the messages
# that say a build is waiting for it.
POLL_INTERVAL = 0.5
WAIT_NOTICE = 60.0
# Locks this process holds: a second acquisition would wait for itself.
HELD: set[Path] = set()
# Errors that mean another process holds the lock; others are raised.
BUSY = {errno.EACCES, errno.EAGAIN, errno.EWOULDBLOCK, errno.EDEADLK}


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
        str(path.relative_to(source)): path.read_text(encoding="utf-8")
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
        target.write_text(text, encoding="utf-8")
    fixtures = source.parent / "fixtures"
    if fixtures.exists():
        shutil.copytree(fixtures, destination.parent / "fixtures", dirs_exist_ok=True)

    return cast("dict[str, bool]", result["flags"])


def source_rewriter() -> Path:
    """Build the pinned token-aware module relocation tool once per preparation."""
    target = ROOT / "build/source-tool"
    toolchain = rust_toolchain()
    version = subprocess.check_output(
        ["rustc", f"+{toolchain}", "-vV"], cwd=ROOT, text=True
    )
    host = next(
        (
            line.removeprefix("host: ").strip()
            for line in version.splitlines()
            if line.startswith("host: ")
        ),
        None,
    )
    if not host:
        msg = f"`rustc +{toolchain} -vV` reported no host target:\n{version}"
        raise ValueError(msg)
    # An explicit host target and target directory fix the output path, even
    # when the caller's Cargo configuration selects another target (WASM).
    subprocess.run(
        [
            "cargo",
            f"+{toolchain}",
            "build",
            "--target",
            host,
            "--offline",
            "--locked",
            "--manifest-path",
            str(ROOT / "tools/package-source/Cargo.toml"),
            "--target-dir",
            str(target),
        ],
        cwd=ROOT,
        check=True,
    )
    suffix = ".exe" if sys.platform == "win32" else ""
    return target / host / "debug" / f"tapirscan-package-source{suffix}"


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
    # The crate embeds the multiformat readers, so it carries their notices.
    shutil.copy2(
        ROOT / "multiformat/THIRD_PARTY_NOTICES.md",
        destination / "THIRD_PARTY_NOTICES.md",
    )
    shutil.copytree(binding / "api", destination / "api")
    shutil.copytree(binding / "tests", destination / "tests")
    shutil.copytree(binding / "examples", destination / "examples")
    generated = destination / "generated"
    modules = []
    flags = {}
    for mode in MODES:
        name = mode.replace("-", "_")
        features = {f"mode-{mode}"}
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
                    {mode},
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
                f"{LIBRARY_LINTS}\npub(crate) mod {core};"
                if name == "low"
                else f"{gate}\n{LIBRARY_LINTS}\npub(crate) mod {core};",
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
    modules.append(f"{LIBRARY_LINTS}\npub(crate) mod multiformat;")
    (generated / "mod.rs").write_text("\n".join(modules) + "\n", encoding="utf-8")
    entries = [
        f'    ("{name}", {str(enabled).lower()}),'
        for name, enabled in sorted(flags.items())
    ]
    mode_entries = [
        f'    ("tapirscan_mode_{m.replace("-", "_")}", '
        f'"CARGO_FEATURE_MODE_{m.replace("-", "_").upper()}"),'
        for m in MODES
    ]
    (destination / "build.rs").write_text(
        "const FLAGS: &[(&str, bool)] = &[\n"
        + "\n".join(entries)
        + "\n];\n"
        + "const MODES: &[(&str, &str)] = &[\n"
        + "\n".join(mode_entries)
        + "\n];\n"
        + "fn main() {\n"
        + "    for (name, feature) in MODES {\n"
        + '        println!("cargo::rustc-check-cfg=cfg({name})");\n'
        + '        println!("cargo::rerun-if-env-changed={feature}");\n'
        + "        if std::env::var_os(feature).is_some() {\n"
        + '            println!("cargo::rustc-cfg={name}");\n'
        + "        }\n    }\n"
        + '    println!("cargo::rerun-if-changed=build.rs");\n'
        + "    for (name, enabled) in FLAGS {\n"
        + '        println!("cargo::rustc-check-cfg=cfg({name})");\n'
        + '        if *enabled { println!("cargo::rustc-cfg={name}"); }\n'
        + "    }\n}\n",
        encoding="utf-8",
    )
    provenance = {
        "modes": list(MODES),
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
        json.dumps(provenance, indent=2) + "\n", encoding="utf-8"
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


def try_lock(handle: BinaryIO) -> bool:
    """Take the exclusive lock on an open file without blocking."""
    handle.seek(0)
    try:
        if sys.platform == "win32":
            msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError as error:
        if error.errno in BUSY:
            return False
        raise
    return True


def unlock(handle: BinaryIO) -> None:
    """Release a lock taken by try_lock; closing the file would also release it."""
    handle.seek(0)
    if sys.platform == "win32":
        msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
    else:
        fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


@contextmanager
def exclusive(lock: Path, *, notice: float = WAIT_NOTICE) -> Iterator[None]:
    """Hold an OS file lock, waiting as long as another build holds it.

    The operating system releases the lock when its holder exits, however it
    exits, so a lock cannot go stale and the wait has no timeout: it only ever
    waits for a build that is still running. The lock file itself stays.
    """
    if lock in HELD:
        msg = f"This process already holds {lock}"
        raise RuntimeError(msg)
    with lock.open("a+b") as handle:
        announced = None
        while not try_lock(handle):
            now = time.monotonic()
            if announced is None or now - announced >= notice:
                print(
                    f"Waiting for another build to release {lock}",
                    file=sys.stderr,
                    flush=True,
                )
                announced = now
            time.sleep(POLL_INTERVAL)
        HELD.add(lock)
        try:
            yield
        finally:
            HELD.discard(lock)
            unlock(handle)


@contextmanager
def prepared_package(destination: Path, *, refresh: bool = False) -> Iterator[Path]:
    """Hold an exclusive package lock through preparation and its consuming build."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    with exclusive(destination.with_name(destination.name + ".build-lock")):
        if destination.exists() and not refresh:
            raise FileExistsError(destination)
        subprocess.run(
            [sys.executable, str(ROOT / "scripts/verify_sources.py")],
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

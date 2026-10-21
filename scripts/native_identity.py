"""Content identity for prebuilt native libraries; verification never loads code."""

import hashlib
import json
import re
from pathlib import Path

SOURCE_ROOTS = (
    "core/src",
    "multiformat/src",
    "bindings/rust/src",
    "bindings/rust/api",
    "bindings/c/src",
    "tools/package-source/src",
)
SOURCE_FILES = (
    "core/Cargo.toml",
    "core/Cargo.lock",
    "multiformat/Cargo.toml",
    "multiformat/Cargo.lock",
    "bindings/rust/Cargo.toml",
    "bindings/rust/Cargo.lock",
    "bindings/c/Cargo.toml.in",
    "bindings/rust/README.md",
    "tools/package-source/Cargo.toml",
    "tools/package-source/Cargo.lock",
    "scripts/build.py",
    "scripts/build_native.py",
    "scripts/build_turbo.py",
    "scripts/prepare_rust.py",
    "scripts/native_identity.py",
    "config/formats.json",
    "config/modes.json",
    "rust-toolchain.toml",
)
MANIFEST = "native-build.json"
BUILD_SCOPE = "requested recipe/environment; effective compiler and target not attested"


def sha256(path: Path) -> str:
    """Hash one input without executing or loading it."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def scalar(path: Path, pattern: str) -> str:
    """Read a required scalar from a maintained source/configuration file."""
    match = re.search(pattern, path.read_text(), re.MULTILINE)
    if match is None:
        message = f"Missing identity field in {path}"
        raise ValueError(message)
    return str(match.group(1))


def source_identity(root: Path) -> dict[str, object]:
    """Identify maintained native inputs by relative names and content."""
    # Same relative-path/content-hash model as WASM build.json. Enumerate anew
    # on verification: additions and deletions must invalidate the artifact too.
    paths = set(SOURCE_FILES)
    for directory in SOURCE_ROOTS:
        folder = root / directory
        if not folder.is_dir():
            message = f"Missing source directory: {folder}"
            raise ValueError(message)
        paths.update(p.relative_to(root).as_posix() for p in folder.rglob("*.rs"))
    # Cargo is invoked from ROOT: its repository-level config is an input too.
    paths.update(
        name
        for name in (".cargo/config", ".cargo/config.toml")
        if (root / name).is_file()
    )
    files = {name: sha256(root / name) for name in sorted(paths)}
    version = scalar(root / "bindings/rust/Cargo.toml", r'^version\s*=\s*"([^"]+)"')
    python_version = scalar(
        root / "bindings/python/pyproject.toml", r'^version\s*=\s*"([^"]+)"'
    )
    if version != python_version:
        message = "Rust and Python package versions differ"
        raise ValueError(message)
    return {
        "version": version,
        "abi": int(
            scalar(root / "bindings/c/src/lib.rs", r"^const ABI_VERSION: u32 = (\d+);")
        ),
        "toolchain": scalar(root / "rust-toolchain.toml", r'^channel\s*=\s*"([^"]+)"'),
        "sourceFiles": files,
        "sourceDigest": hashlib.sha256(
            json.dumps(files, sort_keys=True).encode()
        ).hexdigest(),
    }


def cargo_artifact(output: str, native: Path, name: str) -> Path:
    """Select this invocation's native cdylib, never guess an output directory."""
    matches: set[Path] = set()
    for line in output.splitlines():
        message = json.loads(line)
        if message.get("reason") != "compiler-artifact":
            continue
        if (
            Path(message.get("manifest_path", "")).resolve()
            != (native / "Cargo.toml").resolve()
        ):
            continue
        target = message.get("target", {})
        if target.get("name") != "tapirscan" or "cdylib" not in target.get(
            "crate_types", []
        ):
            continue
        matches.update(
            Path(filename)
            for filename in message.get("filenames", [])
            if Path(filename).name == name
        )
    if len(matches) != 1:
        message = "Cargo did not report exactly one matching native cdylib"
        raise ValueError(message)
    return matches.pop()


def build_metadata(
    identity: dict[str, object], env: dict[str, str]
) -> dict[str, object]:
    """Record requested build settings without claiming compiler attestation."""
    # Cargo configuration and wrappers can select/replace the compiler. Do not
    # mislabel a separate `rustc -vV` probe as the compiler that emitted this file.
    return {
        "scope": BUILD_SCOPE,
        "requestedRustupToolchain": identity["toolchain"],
        "environment": {
            key: env[key]
            for key in (
                "RUSTUP_TOOLCHAIN",
                "RUSTC",
                "CARGO_BUILD_RUSTC",
                "RUSTC_WRAPPER",
                "CARGO_BUILD_RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_TARGET",
                "CARGO_ENCODED_RUSTFLAGS",
                "MACOSX_DEPLOYMENT_TARGET",
            )
            if key in env
        },
    }


def record(
    root: Path,
    library: Path,
    identity: dict[str, object],
    preset: str | None,
    build: dict[str, object],
) -> None:
    """Bind the copied artifact to unchanged source inputs and its recipe."""
    if source_identity(root) != identity:
        message = "Native inputs changed during build; rebuild before packaging"
        raise ValueError(message)
    manifest = {
        "schema": 1,
        "identity": identity,
        "preset": preset,
        "library": {"name": library.name, "sha256": sha256(library)},
        "build": build,
    }
    destination = library.parent / MANIFEST
    temporary = destination.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(manifest, indent=2) + "\n")
    temporary.replace(destination)


def verify(root: Path, library: Path) -> str:
    """Require the current ordinary native artifact, including external directories."""
    try:
        return _verify_manifest(root, library)
    except (OSError, ValueError, KeyError, TypeError) as error:
        message = (
            f"Cannot package {library}: {error}. "
            "Run scripts/build_native.py for this checkout, or select a matching "
            "ordinary build with TAPIRSCAN_NATIVE_DIR."
        )
        raise RuntimeError(message) from error


def _verify_manifest(root: Path, library: Path) -> str:
    """Validate manifest fields before adding caller-facing rebuild guidance."""
    manifest = json.loads((library.parent / MANIFEST).read_text())
    if manifest["schema"] != 1 or manifest["preset"] is not None:
        message = "Unsupported or private native build"
        raise ValueError(message)
    if manifest["identity"] != source_identity(root):
        message = "Native source identity differs from this checkout"
        raise ValueError(message)
    if manifest["library"] != {"name": library.name, "sha256": sha256(library)}:
        message = "Native library content differs from its build manifest"
        raise ValueError(message)
    build = manifest["build"]
    if (
        build["requestedRustupToolchain"] != manifest["identity"]["toolchain"]
        or build["scope"] != BUILD_SCOPE
        or not isinstance(build["environment"], dict)
    ):
        message = "Native build identity is incomplete"
        raise ValueError(message)
    return str(manifest["library"]["sha256"])

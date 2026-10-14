#!/usr/bin/env python3
"""Keep release WASM symbol identities independent of the checkout directory."""

import hashlib
import json
import os
import shutil
import sys
from collections.abc import Mapping
from pathlib import Path


def compiler_args(args: list[str], environ: Mapping[str, str]) -> list[str]:
    """Replace Cargo's path-dependent metadata; preserve every other compiler flag."""
    filtered: list[str] = []
    index = 0
    while index < len(args):
        arg = args[index]
        if (
            arg == "-C"
            and index + 1 < len(args)
            and args[index + 1].startswith("metadata=")
        ):
            index += 2
        elif arg.startswith("-Cmetadata="):
            index += 1
        else:
            filtered.append(arg)
            index += 1
    if len(filtered) == len(args):
        return filtered
    identity = {
        "package": environ["CARGO_PKG_NAME"],
        "version": environ["CARGO_PKG_VERSION"],
        **{
            key: sorted(
                filtered[i + 1] for i, arg in enumerate(filtered[:-1]) if arg == key
            )
            for key in ("--crate-name", "--target", "--cfg", "--crate-type")
        },
    }
    digest = hashlib.sha256(
        json.dumps(identity, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()[:16]
    return [*filtered, "-C", f"metadata={digest}"]


def linker_environment(compiler: str, environ: Mapping[str, str]) -> dict[str, str]:
    """Restore the toolchain library path that macOS strips from this wrapper.

    rust-lld loads the toolchain's libLLVM.dylib through the dyld fallback path
    rustup sets; System Integrity Protection removes it when `/usr/bin/env`
    starts this script (rust-lang/rust#163947).
    """
    env = dict(environ)
    resolved = shutil.which(compiler)
    if sys.platform == "darwin" and resolved:
        lib = Path(resolved).resolve().parent.parent / "lib"
        if (lib / "libLLVM.dylib").exists():
            fallback = env.get("DYLD_FALLBACK_LIBRARY_PATH")
            env["DYLD_FALLBACK_LIBRARY_PATH"] = (
                f"{lib}{os.pathsep}{fallback}" if fallback else str(lib)
            )
    return env


if __name__ == "__main__":
    command = [sys.argv[1], *compiler_args(sys.argv[2:], os.environ)]
    os.environ.update(linker_environment(sys.argv[1], os.environ))
    # Cargo supplies the compiler; retain its exit and signal behavior.
    os.execvp(sys.argv[1], command)  # noqa: S606

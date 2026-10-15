"""Locate and version-check the native library shared by scanners and parsing."""

from __future__ import annotations

import ctypes as c
import os
import sys
from pathlib import Path

ABI_VERSION = 7

_ERROR_CODES = {1: "invalid_input", 2: "disposed", 3: "engine", 4: "engine"}


class ScannerError(RuntimeError):
    """A native scanner failure; `code` matches the JavaScript error codes."""

    def __init__(self, status: int, message: str) -> None:
        """Keep the native status and name its error code."""
        self.status = status
        self.code = _ERROR_CODES.get(status, "engine")
        super().__init__(f"{message} (status {status})")


def library_path(library_dir: str | os.PathLike[str] | None) -> Path:
    """Resolve the native library from the argument, environment or wheel."""
    directory = (
        library_dir
        if library_dir is not None
        else os.environ.get("TAPIRSCAN_LIBRARY_DIR")
    )
    if directory is None:
        directory = Path(__file__).resolve().parent / "_native"
        if not directory.is_dir():
            msg = (
                "This installation has no bundled native library. "
                "Install a platform wheel, or set library_dir / "
                "TAPIRSCAN_LIBRARY_DIR for a source checkout."
            )
            raise ValueError(msg)
    prefix, suffix = (
        ("", ".dll")
        if sys.platform == "win32"
        else ("lib", ".dylib" if sys.platform == "darwin" else ".so")
    )
    return Path(directory).expanduser().resolve() / f"{prefix}tapirscan{suffix}"


def load(path: Path) -> c.CDLL:
    """Load the library and check its version before resolving other symbols."""
    lib = c.CDLL(str(path))
    lib.tapirscan_abi_version.argtypes = []
    lib.tapirscan_abi_version.restype = c.c_uint32
    actual_abi = lib.tapirscan_abi_version()
    if actual_abi != ABI_VERSION:
        msg = (
            f"Native library ABI mismatch: expected {ABI_VERSION}, "
            f"got {actual_abi} from {path}. "
            "Rebuild the native library or reinstall a matching wheel."
        )
        raise RuntimeError(msg)
    return lib

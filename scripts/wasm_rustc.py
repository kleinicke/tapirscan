#!/usr/bin/env python3
"""Keep release WASM symbol identities independent of the checkout directory."""

import hashlib
import json
import os
import sys
from collections.abc import Mapping


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


if __name__ == "__main__":
    command = [sys.argv[1], *compiler_args(sys.argv[2:], os.environ)]
    # Cargo supplies the compiler; retain its exit and signal behavior.
    os.execvp(sys.argv[1], command)  # noqa: S606

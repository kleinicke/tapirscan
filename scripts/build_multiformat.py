"""Build the pinned opt-in readers with verified source provenance."""

import hashlib
import json
import os
import shutil
import subprocess
from pathlib import Path

from build import wasm_flags

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    """Verify source provenance, test, and build the independent reader module."""
    subprocess.run(["python3", str(ROOT / "scripts/verify_import.py")], check=True)
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "build/multiformat-target"))
    manifest = str(ROOT / "multiformat/Cargo.toml")
    subprocess.run(
        ["cargo", "+1.91.1", "test", "--offline", "--manifest-path", manifest],
        env=env,
        check=True,
    )
    env["RUSTFLAGS"] = wasm_flags()
    subprocess.run(
        [
            "cargo",
            "+1.91.1",
            "build",
            "--offline",
            "--release",
            "--lib",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            manifest,
        ],
        env=env,
        check=True,
    )
    source = (
        ROOT
        / "build/multiformat-target/wasm32-unknown-unknown/release"
        / "barcode_multiformat.wasm"
    )
    dest = ROOT / "bindings/javascript/wasm"
    dest.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, dest / "multiformat.wasm")
    shutil.copy2(
        ROOT / "multiformat/THIRD_PARTY_NOTICES.md",
        ROOT / "bindings/javascript/THIRD_PARTY_NOTICES.md",
    )
    imported = json.loads((ROOT / "provenance/import.json").read_text())
    files = imported["files"]
    if revision := imported.get("releaseRevision"):
        files.update(json.loads((ROOT / revision).read_text())["targetHashes"])
    source_digest = hashlib.sha256(
        json.dumps(
            sorted(
                (name, digest)
                for name, digest in files.items()
                if name.startswith("multiformat/")
            ),
            separators=(",", ":"),
        ).encode()
    ).hexdigest()
    (dest / "multiformat.json").write_text(
        json.dumps(
            {
                "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
                "sourceManifest": "provenance/import.json",
                "sourceDigest": source_digest,
                "sourceDigestScope": "multiformat-files-v1",
                "rustToolchain": "1.91.1",
                "rustflags": env["RUSTFLAGS"],
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    main()

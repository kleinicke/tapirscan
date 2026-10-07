"""Validate artifacts downloaded after this run's build jobs have succeeded."""

import hashlib
import json
import shutil
import tarfile
import zipfile
from email.parser import BytesParser
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parents[1]
WHEEL_FILENAME_PARTS = 5
PLATFORMS = {
    "macosx_11_0_arm64",
    "macosx_11_0_x86_64",
    "win_amd64",
    "manylinux_2_28_x86_64",
    "manylinux_2_28_aarch64",
}


def fail(message: str) -> NoReturn:
    """Stop without staging or publishing an invalid release."""
    raise SystemExit(message)


def wheel_platform(wheel: Path, version: str) -> str:
    """Check wheel identity and ensure all four native libraries are bundled."""
    parts = wheel.stem.split("-")
    if len(parts) != WHEEL_FILENAME_PARTS or parts[:4] != [
        "tapirscan",
        version,
        "py3",
        "none",
    ]:
        fail(f"Unexpected wheel: {wheel.name}")
    matches = PLATFORMS.intersection(parts[4].split("."))
    if len(matches) != 1:
        fail(f"Unexpected platform: {wheel.name}")
    with zipfile.ZipFile(wheel) as archive:
        metadata = BytesParser().parsebytes(
            archive.read(f"tapirscan-{version}.dist-info/METADATA"),
        )
        if metadata["Name"] != "tapirscan" or metadata["Version"] != version:
            fail(f"Wheel metadata mismatch: {wheel.name}")
        native = {Path(name).name for name in archive.namelist() if "/_native/" in name}
        if not native.intersection(
            {"libtapirscan.so", "libtapirscan.dylib", "tapirscan.dll"}
        ):
            fail(f"Missing native library: {wheel.name}")
    return matches.pop()


def write_bundle(
    destination: Path, wheels: list[Path], tarball: Path, crate: Path
) -> None:
    """Copy only the validated packages and record their checksums."""
    for folder, artifacts in (
        ("wheels", wheels),
        ("npm", [tarball]),
        ("crates", [crate]),
    ):
        (destination / folder).mkdir(parents=True)
        for artifact in artifacts:
            shutil.copy2(artifact, destination / folder / artifact.name)
    checksums = []
    for path in sorted(destination.rglob("*")):
        if path.is_file():
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            checksums.append(f"{digest}  {path.relative_to(destination)}")
    (destination / "SHA256SUMS").write_text("\n".join(checksums) + "\n")


def main() -> None:
    """Reject incomplete platform coverage and mismatched package versions."""
    package = json.loads((ROOT / "bindings/javascript/package.json").read_text())
    version = package["version"]
    staging = ROOT / "build/release-input"
    destination = ROOT / "build/release"
    if destination.exists():
        fail("Release destination must be empty")
    if not staging.is_dir():
        fail("Missing artifacts from this run's successful validation and wheel jobs")
    wheels = sorted((staging / "WHEELS_RUN").rglob("*.whl"))
    platforms = {wheel_platform(wheel, version) for wheel in wheels}
    if platforms != PLATFORMS or len(wheels) != len(PLATFORMS):
        fail(f"Incomplete or duplicate wheel set: {sorted(platforms)}")
    tarballs = list((staging / "CI_RUN").rglob("*.tgz"))
    if len(tarballs) != 1 or tarballs[0].name != f"tapirscan-{version}.tgz":
        fail("Expected exactly one matching npm tarball")
    with tarfile.open(tarballs[0]) as archive:
        manifest = archive.extractfile("package/package.json")
        if manifest is None or json.load(manifest) != package:
            fail("npm package metadata does not match this commit")
    crates = list((staging / "CI_RUN").rglob("*.crate"))
    if len(crates) != 1 or crates[0].name != f"tapirscan-{version}.crate":
        fail("Expected exactly one matching Rust crate")
    with tarfile.open(crates[0]) as archive:
        names = set(archive.getnames())
    for required in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        if f"tapirscan-{version}/{required}" not in names:
            fail(f"The Rust crate is missing {required}")
    write_bundle(destination, wheels, tarballs[0], crates[0])
    print(f"Prepared {version}: five platform wheels, one npm package and one crate")


if __name__ == "__main__":
    main()

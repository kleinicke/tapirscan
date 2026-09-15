"""Collect successful same-commit CI artifacts and validate the release set."""

import hashlib
import json
import os
import shutil
import subprocess
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


def download(staging: Path, variable: str, workflow: str, pattern: str) -> None:
    """Require successful trusted workflow runs for this exact source commit."""
    run_id = os.environ[variable]
    repository = os.environ["GITHUB_REPOSITORY"]
    if not run_id.isdecimal():
        fail("Workflow run IDs must be numeric")
    run = json.loads(
        subprocess.check_output(
            ["gh", "api", f"repos/{repository}/actions/runs/{run_id}"],
        )
    )
    if (
        run["conclusion"] != "success"
        or run["head_sha"] != os.environ["GITHUB_SHA"]
        or run["path"] != workflow
        or run["event"] not in {"push", "workflow_dispatch"}
    ):
        fail(f"Run {run_id} is not a successful {workflow} build of this commit")
    subprocess.run(
        [
            "gh",
            "run",
            "download",
            run_id,
            "--repo",
            repository,
            "--pattern",
            pattern,
            "--dir",
            str(staging / variable),
        ],
        check=True,
    )


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
        for mode in ("low", "medium", "high", "very_high"):
            names = {
                f"libtapirscan_{mode}.so",
                f"libtapirscan_{mode}.dylib",
                f"tapirscan_{mode}.dll",
            }
            if not native.intersection(names):
                fail(f"Missing {mode} native library: {wheel.name}")
    return matches.pop()


def write_bundle(destination: Path, wheels: list[Path], tarball: Path) -> None:
    """Copy only the validated packages and record their checksums."""
    for folder, artifacts in (("wheels", wheels), ("npm", [tarball])):
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
    """Reject mixed commits, incomplete platform coverage and mismatched versions."""
    package = json.loads((ROOT / "bindings/javascript/package.json").read_text())
    version = package["version"]
    if (
        os.environ["PUBLISH"] != "none"
        and os.environ["GITHUB_REF"] != f"refs/tags/v{version}"
    ):
        fail(f"Publish only from the v{version} tag")
    staging = ROOT / "build/release-input"
    destination = ROOT / "build/release"
    if staging.exists() or destination.exists():
        fail("Release staging directories must be empty")
    staging.mkdir(parents=True)
    download(staging, "CI_RUN", ".github/workflows/ci.yml", "packages-ubuntu-24.04")
    download(staging, "WHEELS_RUN", ".github/workflows/wheels.yml", "wheel-*")
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
    write_bundle(destination, wheels, tarballs[0])
    print(f"Prepared {version}: five tested platform wheels and one npm package")


if __name__ == "__main__":
    main()

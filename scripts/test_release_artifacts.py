"""Exercise rejection of mixed builds and incomplete release wheels."""

import hashlib
import io
import json
import shutil
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from collect_release import (
    DESKTOP,
    HEADERS,
    MOBILE,
    NOTICES,
    PLATFORMS,
    check_crate,
    main,
    wheel_platform,
)


def library(platform: str) -> tuple[str, bytes]:
    """Return a library name and a minimal header of the platform's binary format."""
    system, arch = platform.split("-")[:2]
    if system in ("linux", "android"):
        machine = {"arm64": 0xB7, "x64": 0x3E}[arch]
        return "libtapirscan.so", b"\x7fELF" + bytes(14) + machine.to_bytes(2, "little")
    if system == "windows":
        machine = {"arm64": 0xAA64, "x64": 0x8664}[arch]
        pe = b"\x40\0\0\0PE\0\0" + machine.to_bytes(2, "little")
        return "tapirscan.dll", b"MZ" + bytes(58) + pe
    cpu = {"arm64": 0x0100000C, "x64": 0x01000007}[arch]
    return "libtapirscan.dylib", b"\xcf\xfa\xed\xfe" + cpu.to_bytes(4, "little")


def write_desktop(wheels: Path, native: Path, platform: str, data: bytes) -> Path:
    """Write a wheel and its job's native artifact holding the same library."""
    name = library(DESKTOP[platform])[0]
    path = wheels / f"tapirscan-1.1.0-py3-none-{platform}.whl"
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr(
            "tapirscan-1.1.0.dist-info/METADATA",
            "Name: tapirscan\nVersion: 1.1.0\n",
        )
        archive.writestr(f"tapirscan/_native/{name}", data)
    built = native / f"native-{DESKTOP[platform]}"
    built.mkdir(parents=True, exist_ok=True)
    (built / name).write_bytes(data)
    return built


def write_native_inputs(root: Path, wheels: Path, native: Path) -> None:
    """Write sources, wheels and native CI artifacts that form a complete set."""
    for source in (*HEADERS, *NOTICES):
        path = root / source / "tapirscan.h" if "include" in source else root / source
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(source)
    for platform in PLATFORMS:
        name, data = library(DESKTOP[platform])
        built = write_desktop(wheels, native, platform, data)
        if name == "tapirscan.dll":
            (built / "tapirscan.dll.lib").write_bytes(b"!<arch>\n")
    for target, platform in MOBILE.items():
        name, data = library(platform)
        path = native / "native-mobile/flutter-native" / target / name
        path.parent.mkdir(parents=True)
        path.write_bytes(data)
    xcframework = native / "native-apple/TapirscanC.xcframework"
    xcframework.mkdir(parents=True)
    (xcframework / "Info.plist").write_text("plist")
    (native / "native-mobile/tapirscan-android.aar").write_bytes(b"aar")


class ReleaseArtifacts(unittest.TestCase):
    """Keep release validation strict while accepting repaired wheel directories."""

    def test_wheel_contents(self) -> None:
        """A directory entry is harmless; a missing native library is not."""
        with tempfile.TemporaryDirectory() as temporary:
            wheel = (
                Path(temporary) / "tapirscan-1.1.0-py3-none-manylinux_2_28_x86_64.whl"
            )
            for bundled in (True, False):
                with zipfile.ZipFile(wheel, "w") as archive:
                    archive.writestr("tapirscan/_native/", "")
                    archive.writestr(
                        "tapirscan-1.1.0.dist-info/METADATA",
                        "Name: tapirscan\nVersion: 1.1.0\n",
                    )
                    if bundled:
                        archive.writestr("tapirscan/_native/libtapirscan.so", b"")
                if not bundled:
                    with self.assertRaisesRegex(SystemExit, "Missing native library"):
                        wheel_platform(wheel, "1.1.0")
                else:
                    self.assertEqual(
                        wheel_platform(wheel, "1.1.0"), "manylinux_2_28_x86_64"
                    )
                    with self.assertRaisesRegex(SystemExit, "Unexpected wheel"):
                        wheel_platform(wheel, "1.0.1")

    def test_crate_without_notices_is_rejected(self) -> None:
        """A crate must carry the license and the third-party notices."""
        with tempfile.TemporaryDirectory() as temporary:
            crate = Path(temporary) / "tapirscan-1.1.0.crate"
            with tarfile.open(crate, "w:gz") as archive:
                info = tarfile.TarInfo("tapirscan-1.1.0/LICENSE")
                info.size = 1
                archive.addfile(info, io.BytesIO(b"L"))
            with self.assertRaisesRegex(SystemExit, "THIRD_PARTY_NOTICES"):
                check_crate(crate, "1.1.0")

    def test_complete_bundle_and_missing_platform(self) -> None:
        """Only a complete same-version bundle is accepted; hashes cover its files."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            package = {"name": "tapirscan", "version": "1.1.0"}
            manifest = root / "bindings/javascript/package.json"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps(package))
            staging = root / "build/release-input"
            ci = staging / "CI_RUN"
            wheels = staging / "WHEELS_RUN"
            ci.mkdir(parents=True)
            wheels.mkdir()
            native = staging / "NATIVE_RUN"
            write_native_inputs(root, wheels, native)
            with tarfile.open(ci / "tapirscan-1.1.0.tgz", "w:gz") as archive:
                data = json.dumps(package).encode()
                info = tarfile.TarInfo("package/package.json")
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
            with tarfile.open(ci / "tapirscan-1.1.0.crate", "w:gz") as archive:
                for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
                    info = tarfile.TarInfo(f"tapirscan-1.1.0/{name}")
                    info.size = len(name)
                    archive.addfile(info, io.BytesIO(name.encode()))
            with patch("collect_release.ROOT", root):
                main()
                bundle = root / "build/release"
                entries = (bundle / "SHA256SUMS").read_text().splitlines()
                # Six wheels, npm, crate, eleven library archives, AAR,
                # XCFramework and the release assets' own checksums.
                self.assertEqual(len(entries), 22)
                for entry in entries:
                    digest, filename = entry.split("  ")
                    self.assertEqual(
                        digest,
                        hashlib.sha256((bundle / filename).read_bytes()).hexdigest(),
                    )

                assets = (bundle / "native/SHA256SUMS").read_text()
                self.assertIn("  tapirscan-1.1.0-ios-arm64-simulator.zip\n", assets)
                with zipfile.ZipFile(
                    bundle / "native/tapirscan-1.1.0-windows-x64.zip"
                ) as archive:
                    self.assertIn(
                        "tapirscan-1.1.0-windows-x64/lib/tapirscan.dll.lib",
                        archive.namelist(),
                    )
                with zipfile.ZipFile(
                    bundle / "native/tapirscan-1.1.0-windows-arm64.zip"
                ) as archive:
                    self.assertIn(
                        "tapirscan-1.1.0-windows-arm64/lib/tapirscan.dll",
                        archive.namelist(),
                    )

                shutil.rmtree(bundle)
                # An x64 library in the ARM64 wheel and its native artifact.
                x64 = library("windows-x64")[1]
                write_desktop(wheels, native, "win_arm64", x64)
                with self.assertRaisesRegex(
                    SystemExit, r"windows-arm64 library is built for \['x64'\]"
                ):
                    main()
                arm64 = library("windows-arm64")[1]
                write_desktop(wheels, native, "win_arm64", arm64)

                android = native / "native-mobile/flutter-native/x86_64-linux-android"
                (android / "libtapirscan.so").write_bytes(library("android-arm64")[1])
                with self.assertRaisesRegex(SystemExit, "android-x64 library is built"):
                    main()

                next(wheels.glob("*.whl")).unlink()
                with self.assertRaisesRegex(SystemExit, "Incomplete"):
                    main()


if __name__ == "__main__":
    unittest.main()

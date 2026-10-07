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

from collect_release import PLATFORMS, main, wheel_platform


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
            for platform in PLATFORMS:
                path = wheels / f"tapirscan-1.1.0-py3-none-{platform}.whl"
                with zipfile.ZipFile(path, "w") as archive:
                    archive.writestr(
                        "tapirscan-1.1.0.dist-info/METADATA",
                        "Name: tapirscan\nVersion: 1.1.0\n",
                    )
                    archive.writestr("tapirscan/_native/libtapirscan.so", b"native")
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
                self.assertEqual(len(entries), 7)
                for entry in entries:
                    digest, filename = entry.split("  ")
                    self.assertEqual(
                        digest,
                        hashlib.sha256((bundle / filename).read_bytes()).hexdigest(),
                    )

                shutil.rmtree(bundle)
                next(wheels.glob("*.whl")).unlink()
                with self.assertRaisesRegex(SystemExit, "Incomplete"):
                    main()


if __name__ == "__main__":
    unittest.main()

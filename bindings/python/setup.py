"""Build platform wheels containing the ctypes native library, without a Python ABI."""

import os
import platform as host_platform
import shutil
import sys
from pathlib import Path

from setuptools import Distribution, setup
from setuptools.command.bdist_wheel import bdist_wheel
from setuptools.command.build_py import build_py


class NativeDistribution(Distribution):
    """Mark wheels as platform-specific even though the Python API uses ctypes."""

    def has_ext_modules(self) -> bool:
        """Ensure the wheel is never labelled platform-independent."""
        return True


class BuildWithLibraries(build_py):
    """Copy prebuilt, validated libraries and license notices into the package."""

    def run(self) -> None:
        """Require and bundle the native library, which contains every mode."""
        source = Path(
            os.environ.get(
                "TAPIRSCAN_NATIVE_DIR",
                Path(__file__).resolve().parents[2] / "build/native",
            )
        )
        name = (
            "tapirscan.dll"
            if sys.platform == "win32"
            else "libtapirscan.dylib"
            if sys.platform == "darwin"
            else "libtapirscan.so"
        )
        # Wheel builds require the full checkout; Python-only sdists are unsupported.
        root = Path(__file__).resolve().parents[2]
        sys.path.insert(0, str(root / "scripts"))
        try:
            # Metadata queries need not import checkout-only build helpers.
            from native_identity import sha256, verify  # noqa: PLC0415

            expected_digest = verify(root, source / name)
        finally:
            sys.path.pop(0)
        super().run()
        package = Path(self.build_lib) / "tapirscan"
        shutil.copy2(
            Path(__file__).resolve().parents[2] / "multiformat/THIRD_PARTY_NOTICES.md",
            package / "THIRD_PARTY_NOTICES.md",
        )
        destination = package / "_native"
        # Reused build directories may hold libraries from earlier builds.
        shutil.rmtree(destination, ignore_errors=True)
        destination.mkdir(parents=True)
        if not (source / name).is_file():
            msg = (
                f"Missing {source / name}. "
                "Build the native library or set TAPIRSCAN_NATIVE_DIR."
            )
            raise RuntimeError(msg)
        copied = destination / name
        shutil.copy2(source / name, copied)
        if sha256(copied) != expected_digest:
            copied.unlink()
            msg = "Native library changed while packaging; rebuild and retry the wheel"
            raise RuntimeError(msg)


class CtypesWheel(bdist_wheel):
    """The native C ABI works across supported Python 3 versions."""

    def get_tag(self) -> tuple[str, str, str]:
        """Use a Python-independent tag and the native build's deployment baseline."""
        _, _, platform = super().get_tag()
        target = os.environ.get("MACOSX_DEPLOYMENT_TARGET")
        if sys.platform == "darwin" and target:
            platform = f"macosx_{target.replace('.', '_')}_{host_platform.machine()}"
        return "py3", "none", platform


setup(
    distclass=NativeDistribution,
    cmdclass={"build_py": BuildWithLibraries, "bdist_wheel": CtypesWheel},
)

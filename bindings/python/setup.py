"""Build platform wheels containing all four ctypes libraries, without a Python ABI."""

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
    """Copy prebuilt, validated libraries into the installed package."""

    def run(self) -> None:
        """Require and bundle every effort mode."""
        super().run()
        source = Path(
            os.environ.get(
                "TAPIRSCAN_NATIVE_DIR",
                Path(__file__).resolve().parents[2] / "build/native",
            )
        )
        prefix, suffix = (
            ("", ".dll")
            if sys.platform == "win32"
            else ("lib", ".dylib" if sys.platform == "darwin" else ".so")
        )
        destination = Path(self.build_lib) / "tapirscan" / "_native"
        destination.mkdir(parents=True, exist_ok=True)
        for mode in ("low", "medium", "high", "very_high"):
            name = f"{prefix}tapirscan_{mode}{suffix}"
            if not (source / name).is_file():
                msg = (
                    f"Missing {source / name}. "
                    "Build all native modes or set TAPIRSCAN_NATIVE_DIR."
                )
                raise RuntimeError(msg)
            shutil.copy2(source / name, destination / name)


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

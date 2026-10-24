"""Validate artifacts downloaded after this run's build jobs have succeeded."""

import hashlib
import io
import json
import shutil
import tarfile
import zipfile
from email.parser import BytesParser
from pathlib import Path
from typing import NoReturn

ROOT = Path(__file__).resolve().parents[1]
WHEEL_FILENAME_PARTS = 5
# Wheel platform tag and the native asset platform, named like the Dart build
# hook's targets: <os>-<architecture>[-simulator].
DESKTOP = {
    "macosx_11_0_arm64": "macos-arm64",
    "macosx_11_0_x86_64": "macos-x64",
    "win_amd64": "windows-x64",
    "win_arm64": "windows-arm64",
    "manylinux_2_28_x86_64": "linux-x64",
    "manylinux_2_28_aarch64": "linux-arm64",
}
PLATFORMS = set(DESKTOP)
# Rust targets built only for the native assets, by the Android and Swift CI jobs.
MOBILE = {
    "aarch64-linux-android": "android-arm64",
    "x86_64-linux-android": "android-x64",
    "aarch64-apple-ios": "ios-arm64",
    "aarch64-apple-ios-sim": "ios-arm64-simulator",
    "x86_64-apple-ios": "ios-x64-simulator",
}
LIBRARIES = ("libtapirscan.so", "libtapirscan.dylib", "tapirscan.dll")
HEADERS = ("bindings/c/include", "bindings/cpp/include")
NOTICES = (
    "LICENSE",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "multiformat/THIRD_PARTY_NOTICES.md",
)
# Mirrors bindings/cpp/TapirscanConfig.cmake.in for the archive's install tree.
CMAKE_CONFIG = """\
# Tapirscan {version} for {platform}; find_package(Tapirscan CONFIG) with this
# archive's directory in CMAKE_PREFIX_PATH.
get_filename_component(_tapirscan_prefix "${{CMAKE_CURRENT_LIST_DIR}}/../../.."
  ABSOLUTE)
if(NOT TARGET tapirscan::native)
  add_library(tapirscan::native SHARED IMPORTED)
  set_target_properties(tapirscan::native PROPERTIES
    IMPORTED_LOCATION "${{_tapirscan_prefix}}/lib/{library}"
    INTERFACE_INCLUDE_DIRECTORIES "${{_tapirscan_prefix}}/include")
  if(UNIX)
    set_target_properties(tapirscan::native PROPERTIES IMPORTED_SONAME "{soname}")
  endif()
  if(WIN32)
    set_target_properties(tapirscan::native PROPERTIES
      IMPORTED_IMPLIB "${{_tapirscan_prefix}}/lib/tapirscan.dll.lib")
  endif()
endif()
if(NOT TARGET tapirscan::cpp)
  add_library(tapirscan::cpp INTERFACE IMPORTED)
  set_target_properties(tapirscan::cpp PROPERTIES
    INTERFACE_COMPILE_FEATURES cxx_std_17
    INTERFACE_INCLUDE_DIRECTORIES "${{_tapirscan_prefix}}/include"
    INTERFACE_LINK_LIBRARIES tapirscan::native)
endif()
unset(_tapirscan_prefix)
"""
CMAKE_VERSION = """\
set(PACKAGE_VERSION "{version}")
if(PACKAGE_FIND_VERSION_MAJOR STREQUAL "{major}"
   AND NOT PACKAGE_VERSION VERSION_LESS PACKAGE_FIND_VERSION)
  set(PACKAGE_VERSION_COMPATIBLE TRUE)
  if(PACKAGE_FIND_VERSION STREQUAL PACKAGE_VERSION)
    set(PACKAGE_VERSION_EXACT TRUE)
  endif()
endif()
"""


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


def check_crate(crate: Path, version: str) -> None:
    """Require the license and third-party notices inside the packaged crate."""
    with tarfile.open(crate) as archive:
        names = set(archive.getnames())
    for required in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
        if f"tapirscan-{version}/{required}" not in names:
            fail(f"The Rust crate is missing {required}")


def wheel_library(wheel: Path) -> tuple[str, bytes]:
    """Return the name and content of the native library a wheel bundles."""
    with zipfile.ZipFile(wheel) as archive:
        for name in archive.namelist():
            if "/_native/" in name and Path(name).name in LIBRARIES:
                return Path(name).name, archive.read(name)
    fail(f"Missing native library: {wheel.name}")


def architecture(data: bytes) -> str | None:
    """Return the CPU architecture of an ELF, Mach-O or PE library."""
    if data[:4] == b"\x7fELF":
        machine = int.from_bytes(data[18:20], "little")
        return {0xB7: "arm64", 0x3E: "x64"}.get(machine)
    if data[:4] == b"\xcf\xfa\xed\xfe":
        cpu = int.from_bytes(data[4:8], "little")
        return {0x0100000C: "arm64", 0x01000007: "x64"}.get(cpu)
    if data[:2] == b"MZ":
        header = int.from_bytes(data[0x3C:0x40], "little")
        machine = int.from_bytes(data[header + 4 : header + 6], "little")
        return {0x8664: "x64", 0xAA64: "arm64"}.get(machine)
    return None


def zip_archive(files: dict[str, bytes]) -> bytes:
    """Return a reproducible archive: sorted entries with a fixed timestamp."""
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, data in sorted(files.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            executable = Path(name).name in LIBRARIES
            info.external_attr = (0o755 if executable else 0o644) << 16
            archive.writestr(info, data)
    return buffer.getvalue()


def library_archive(
    version: str, platform: str, libraries: dict[str, bytes]
) -> dict[str, bytes]:
    """Lay out one platform like `cmake --install`: headers, library, notices."""
    root = f"tapirscan-{version}-{platform}"
    files = {f"{root}/lib/{name}": data for name, data in libraries.items()}
    for directory in HEADERS:
        for header in sorted((ROOT / directory).rglob("*.h*")):
            relative = header.relative_to(ROOT / directory).as_posix()
            files[f"{root}/include/{relative}"] = header.read_bytes()
    for notice in NOTICES:
        name = Path(notice).name
        files[f"{root}/share/licenses/tapirscan/{name}"] = (ROOT / notice).read_bytes()
    library = next(name for name in libraries if name in LIBRARIES)
    soname = f"@rpath/{library}" if library.endswith(".dylib") else library
    cmake = f"{root}/lib/cmake/Tapirscan"
    files[f"{cmake}/TapirscanConfig.cmake"] = CMAKE_CONFIG.format(
        version=version, platform=platform, library=library, soname=soname
    ).encode()
    files[f"{cmake}/TapirscanConfigVersion.cmake"] = CMAKE_VERSION.format(
        version=version, major=version.partition(".")[0]
    ).encode()
    return files


def single(paths: list[Path], description: str) -> Path:
    """Require exactly one artifact."""
    if len(paths) != 1:
        fail(f"Expected exactly one {description}, found {len(paths)}")
    return paths[0]


def native_assets(staging: Path, wheels: list[Path], version: str) -> dict[str, bytes]:
    """Collect the GitHub release assets: libraries per platform, AAR, XCFramework.

    Desktop libraries are the ones inside the tested wheels; the native
    artifact of the same wheel job must hold the identical library and adds the
    Windows import library. Android and iOS libraries come from the Android and
    Swift CI jobs, which build them for the Dart package.
    """
    native = staging / "NATIVE_RUN"
    assets: dict[str, dict[str, bytes]] = {}
    for wheel in wheels:
        platform = DESKTOP[wheel_platform(wheel, version)]
        name, data = wheel_library(wheel)
        built = single(
            list((native / f"native-{platform}").rglob(name)), f"{platform} library"
        )
        if built.read_bytes() != data:
            fail(f"The {platform} native artifact differs from its wheel's library")
        libraries = {name: data}
        if name == "tapirscan.dll":
            implib = single(
                list(built.parent.glob("tapirscan.dll.lib")), "DLL import library"
            )
            libraries[implib.name] = implib.read_bytes()
        assets[platform] = libraries
    for target, platform in MOBILE.items():
        name = "libtapirscan.so" if "android" in target else "libtapirscan.dylib"
        built = single(
            [path for path in native.rglob(name) if path.parent.name == target],
            f"{target} library",
        )
        assets[platform] = {name: built.read_bytes()}
    for platform, libraries in assets.items():
        found = {
            architecture(data) for name, data in libraries.items() if name in LIBRARIES
        }
        if found != {platform.split("-")[1]}:
            fail(f"The {platform} library is built for {sorted(map(str, found))}")
    files = {
        f"tapirscan-{version}-{platform}.zip": zip_archive(
            library_archive(version, platform, libraries)
        )
        for platform, libraries in assets.items()
    }
    xcframework = single(list(native.rglob("TapirscanC.xcframework")), "XCFramework")
    files[f"TapirscanC-{version}.xcframework.zip"] = zip_archive(
        {
            f"TapirscanC.xcframework/{path.relative_to(xcframework).as_posix()}": (
                path.read_bytes()
            )
            for path in xcframework.rglob("*")
            if path.is_file()
        }
    )
    aar = single(list(native.rglob("tapirscan-android.aar")), "Android AAR")
    files[f"tapirscan-android-{version}.aar"] = aar.read_bytes()
    return files


def checksums(directory: Path) -> str:
    """Return sha256sum lines for every file below `directory`."""
    lines = []
    for path in sorted(directory.rglob("*")):
        if path.is_file():
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            lines.append(f"{digest}  {path.relative_to(directory).as_posix()}")
    return "\n".join(lines) + "\n"


def write_bundle(
    destination: Path,
    wheels: list[Path],
    tarball: Path,
    crate: Path,
    native: dict[str, bytes],
) -> None:
    """Copy only the validated packages and record their checksums.

    `native/` holds the GitHub release assets with their own SHA256SUMS, which
    the Dart package ships to verify its downloads.
    """
    for folder, artifacts in (
        ("wheels", wheels),
        ("npm", [tarball]),
        ("crates", [crate]),
    ):
        (destination / folder).mkdir(parents=True)
        for artifact in artifacts:
            shutil.copy2(artifact, destination / folder / artifact.name)
    assets = destination / "native"
    assets.mkdir()
    for name, data in native.items():
        (assets / name).write_bytes(data)
    (assets / "SHA256SUMS").write_text(checksums(assets))
    (destination / "SHA256SUMS").write_text(checksums(destination))


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
    check_crate(crates[0], version)
    native = native_assets(staging, wheels, version)
    write_bundle(destination, wheels, tarballs[0], crates[0], native)
    print(
        f"Prepared {version}: {len(wheels)} platform wheels, one npm package, "
        f"one crate and {len(native)} native release assets"
    )


if __name__ == "__main__":
    main()

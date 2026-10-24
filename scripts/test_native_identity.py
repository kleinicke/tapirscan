"""Freshness checks use synthetic bytes: no compiler or native loading required.

Needs setuptools (the wheel-packaging test imports bindings/python/setup.py) and
typing_extensions (`@override` for the Python 3.10 type-check target).
"""

import json
import runpy
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import native_identity as identity
from setuptools import Distribution
from setuptools.command.build_py import build_py
from typing_extensions import override

BUILD_ENV = {"RUSTUP_TOOLCHAIN": "1.99.0", "RUSTC_WRAPPER": "sccache"}


class NativeIdentityTests(unittest.TestCase):
    """Reject stale inputs while preserving relocated and external builds."""

    @override
    def setUp(self) -> None:
        """Create a small source checkout and non-executable native artifact."""
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / "checkout"
        for name in identity.SOURCE_FILES:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n")
        for name in identity.SOURCE_ROOTS:
            path = self.root / name
            path.mkdir(parents=True, exist_ok=True)
            (path / "lib.rs").write_text("// source\n")
        (self.root / "bindings/rust/Cargo.toml").write_text('version = "1.4.0"\n')
        python = self.root / "bindings/python/pyproject.toml"
        python.parent.mkdir(parents=True)
        python.write_text('version = "1.4.0"\n')
        (self.root / "bindings/c/src/lib.rs").write_text(
            "const ABI_VERSION: u32 = 7;\n"
        )
        (self.root / "rust-toolchain.toml").write_text('channel = "1.99.0"\n')
        self.library = self.base / "external-native/libtapirscan.dylib"
        self.library.parent.mkdir()
        self.library.write_bytes(b"not executable native fixture")
        self.record()

    def test_packaging_rejects_replacement_after_verification(self) -> None:
        """Remove a copied replacement instead of completing a mismatched wheel."""
        setup_path = Path(__file__).resolve().parents[1] / "bindings/python/setup.py"
        with patch("setuptools.setup"):
            namespace = runpy.run_path(str(setup_path))
        command_type = namespace["BuildWithLibraries"]
        command_type.run.__globals__["__file__"] = str(
            self.root / "bindings/python/setup.py"
        )
        notices = self.root / "multiformat/THIRD_PARTY_NOTICES.md"
        notices.write_text("fixture license")
        command = command_type(Distribution())
        command.build_lib = str(self.base / "wheel")
        copied = Path(command.build_lib) / "tapirscan/_native" / self.library.name

        def replace_during_python_copy(command: build_py) -> None:
            (Path(command.build_lib) / "tapirscan").mkdir(parents=True)
            self.library.write_bytes(b"replacement from overlapping native build")

        with (
            patch.dict(
                "os.environ", {"TAPIRSCAN_NATIVE_DIR": str(self.library.parent)}
            ),
            patch("sys.platform", "darwin"),
            patch.object(build_py, "run", replace_during_python_copy),
            self.assertRaisesRegex(RuntimeError, "changed while packaging"),
        ):
            command.run()
        self.assertFalse(copied.exists())

    def record(self, preset: str | None = None) -> None:
        """Write a manifest for the current fixture inputs."""
        identity.record(
            self.root,
            self.library,
            identity.source_identity(self.root),
            preset,
            identity.build_metadata(BUILD_ENV),
        )

    def reject(self) -> None:
        """Require a packaging failure with a concrete rebuild instruction."""
        with self.assertRaisesRegex(RuntimeError, "build_native.py"):
            identity.verify(self.root, self.library)

    def test_external_directory_and_relocated_checkout(self) -> None:
        """Accept identical relocated sources, but reject changed library bytes."""
        identity.verify(self.root, self.library)
        relocated = self.base / "relocated"
        shutil.copytree(self.root, relocated)
        identity.verify(relocated, self.library)
        self.library.write_bytes(b"changed artifact")
        self.reject()

    def test_missing_manifest_library_and_turbo_presets(self) -> None:
        """Reject incomplete artifacts, malformed manifests and Turbo preset builds."""
        manifest = self.library.parent / identity.MANIFEST
        original = manifest.read_bytes()
        manifest.unlink()
        self.reject()
        manifest.write_bytes(b"not json")
        self.reject()
        manifest.write_bytes(original)
        original_library = self.library.read_bytes()
        self.library.unlink()
        self.reject()
        self.library.write_bytes(original_library)
        for preset in ("turbo2", "turbo4", "turbo8", "turbo16"):
            with self.subTest(preset=preset):
                self.record(preset)
                self.reject()

    def test_inventory_addition_deletion_and_content_change(self) -> None:
        """Require a rebuild after source files are added, removed or modified."""
        path = self.root / "core/src/new.rs"
        path.write_text("// added\n")
        self.reject()
        path.unlink()
        identity.verify(self.root, self.library)
        path = self.root / "core/src/lib.rs"
        original = path.read_bytes()
        path.unlink()
        self.reject()
        path.write_bytes(original + b"// changed\n")
        self.reject()

    def test_abi_versions_toolchain_and_recipe(self) -> None:
        """Reject builds made for different ABI, versions, toolchain or recipes."""
        for name in (
            "bindings/c/src/lib.rs",
            "bindings/rust/Cargo.toml",
            "bindings/python/pyproject.toml",
            "rust-toolchain.toml",
            "scripts/build_native.py",
            "scripts/prepare_rust.py",
        ):
            path = self.root / name
            original = path.read_text()
            with self.subTest(name=name):
                path.write_text(
                    original.replace("7;", "8;")
                    .replace("1.4.0", "1.4.1")
                    .replace("1.99.0", "1.100.0")
                    + "# changed\n"
                )
                self.reject()
            path.write_text(original)
        identity.verify(self.root, self.library)

    def test_manifest_identity_corruption(self) -> None:
        """Reject a manifest whose recorded ABI no longer matches the source."""
        path = self.library.parent / identity.MANIFEST
        manifest = json.loads(path.read_text())
        manifest["identity"]["abi"] = 8
        path.write_text(json.dumps(manifest))
        self.reject()

    def test_source_change_during_build(self) -> None:
        """Refuse to record an artifact when inputs changed during compilation."""
        before = identity.source_identity(self.root)
        (self.root / "core/src/lib.rs").write_text("// changed during build\n")
        with self.assertRaisesRegex(ValueError, "changed during build"):
            identity.record(
                self.root,
                self.library,
                before,
                None,
                identity.build_metadata(BUILD_ENV),
            )

    def test_config_inventory_and_requested_compiler_scope(self) -> None:
        """Track Cargo config changes and preserve descriptive compiler requests."""
        config = self.root / ".cargo/config.toml"
        config.parent.mkdir()
        config.write_text('[build]\ntarget = "aarch64-apple-darwin"\n')
        self.reject()
        self.record()
        identity.verify(self.root, self.library)
        config.unlink()
        self.reject()
        env = dict(
            BUILD_ENV,
            RUSTC="/custom/same-version-rustc",
            CARGO_BUILD_TARGET="custom-target",
        )
        metadata = identity.build_metadata(env)
        self.assertEqual(metadata["environment"], env)

    def test_reported_target_artifact_instead_of_stale_default(self) -> None:
        """Select Cargo's matching output and reject missing or ambiguous artifacts."""
        native = self.root / "build/native-crate"
        actual = self.base / "configured-target/release" / self.library.name
        message = {
            "reason": "compiler-artifact",
            "manifest_path": str(native / "Cargo.toml"),
            "target": {"name": "tapirscan", "crate_types": ["cdylib"]},
            "filenames": [str(actual)],
            "fresh": True,
        }
        # An existing default-directory library must not influence selection.
        self.assertEqual(
            identity.cargo_artifact(json.dumps(message), native, self.library.name),
            actual,
        )
        with self.assertRaises(ValueError):
            identity.cargo_artifact("", native, self.library.name)
        other = dict(
            message, manifest_path=str(native.parent / "dependency/Cargo.toml")
        )
        with self.assertRaises(ValueError):
            identity.cargo_artifact(json.dumps(other), native, self.library.name)
        duplicate = dict(message, filenames=[str(self.library)])
        with self.assertRaises(ValueError):
            identity.cargo_artifact(
                json.dumps(message) + "\n" + json.dumps(duplicate),
                native,
                self.library.name,
            )
        wrong_kind = dict(
            message, target={"name": "tapirscan", "crate_types": ["rlib"]}
        )
        with self.assertRaises(ValueError):
            identity.cargo_artifact(json.dumps(wrong_kind), native, self.library.name)


if __name__ == "__main__":
    unittest.main()

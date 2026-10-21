"""Require Cargo-reported source-tool outputs instead of stale default paths."""

import contextlib
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import prepare_rust


class SourceToolArtifacts(unittest.TestCase):
    """Check configured-target selection without compiling or executing a tool."""

    def test_matching_configured_output_and_failures(self) -> None:
        """Ignore stale default output and fail closed without a unique artifact."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stale = root / "build/source-tool/debug/tapirscan-package-source"
            actual = root / "configured/debug/tapirscan-package-source"
            for file, content in [(stale, "stale"), (actual, "current")]:
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text(content)
            message = {
                "reason": "compiler-artifact",
                "manifest_path": str(root / "tools/package-source/Cargo.toml"),
                "target": {"name": "tapirscan-package-source", "kind": ["bin"]},
                "executable": str(actual),
                "fresh": True,
            }
            dependency = dict(
                message, manifest_path=str(root / "dependency/Cargo.toml")
            )
            ambiguous = dict(message, executable=str(stale))
            scenarios = [
                ([dependency, message], True),
                ([], False),
                ([dependency], False),
                ([message, ambiguous], False),
            ]
            for messages, accepted in scenarios:
                completed = subprocess.CompletedProcess(
                    ["cargo"], 0, stdout="\n".join(map(json.dumps, messages))
                )
                with (
                    self.subTest(messages=messages),
                    patch.object(prepare_rust, "ROOT", root),
                    patch.object(prepare_rust, "rust_toolchain", return_value="1.99.0"),
                    patch.dict(
                        os.environ, {"CARGO_BUILD_TARGET": "wasm32-unknown-unknown"}
                    ),
                    patch.object(
                        prepare_rust.subprocess,
                        "check_output",
                        return_value="rustc 1.99.0\nhost: aarch64-apple-darwin\n",
                    ) as compiler,
                    patch.object(
                        prepare_rust.subprocess, "run", return_value=completed
                    ) as run,
                ):
                    if accepted:
                        self.assertEqual(prepare_rust.source_rewriter(), actual)
                        self.assertEqual(run.call_args.kwargs["cwd"], root)
                        self.assertIn("--message-format=json", run.call_args.args[0])
                        command = run.call_args.args[0]
                        self.assertEqual(
                            command[command.index("--target") + 1],
                            "aarch64-apple-darwin",
                        )
                        compiler.assert_called_once_with(
                            ["rustc", "+1.99.0", "-vV"], cwd=root, text=True
                        )
                    else:
                        with self.assertRaisesRegex(ValueError, "exactly one"):
                            prepare_rust.source_rewriter()

    def test_missing_or_ambiguous_host_stops_before_build(self) -> None:
        """Never fall back to the configured library target without a known host."""
        for version in ["rustc 1.99.0\n", "host: \n", "host: first\nhost: second\n"]:
            with (
                self.subTest(version=version),
                patch.object(
                    prepare_rust.subprocess, "check_output", return_value=version
                ),
                patch.object(prepare_rust.subprocess, "run") as run,
            ):
                with self.assertRaisesRegex(ValueError, "exactly one host"):
                    prepare_rust.source_rewriter()
                run.assert_not_called()

    def test_failed_build_renders_diagnostics_before_raising(self) -> None:
        """Show the compiler message instead of returning an older executable."""
        completed = subprocess.CompletedProcess(
            ["cargo"],
            1,
            stdout=json.dumps(
                {
                    "reason": "compiler-message",
                    "message": {"rendered": "compiler failure\n"},
                }
            ),
        )
        stderr = io.StringIO()
        with (
            patch.object(
                prepare_rust.subprocess,
                "check_output",
                return_value="host: aarch64-apple-darwin\n",
            ),
            patch.object(prepare_rust.subprocess, "run", return_value=completed),
            contextlib.redirect_stderr(stderr),
            self.assertRaises(subprocess.CalledProcessError),
        ):
            prepare_rust.source_rewriter()
        self.assertEqual(stderr.getvalue(), "compiler failure\n")


if __name__ == "__main__":
    unittest.main()

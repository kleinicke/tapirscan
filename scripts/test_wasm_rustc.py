"""Prove release crate identities distinguish versions/features, not local paths."""

import unittest

from wasm_rustc import compiler_args


class WasmCompiler(unittest.TestCase):
    """Check metadata normalization without changing compilation semantics."""

    def test_paths_and_cargo_hashes_do_not_change_identity(self) -> None:
        """Two checkouts retain distinct I/O arguments but identical crate metadata."""
        env = {"CARGO_PKG_NAME": "scanner", "CARGO_PKG_VERSION": "1.2.1"}
        common = ["--crate-name", "scanner", "--cfg", 'feature="mode-low"']
        left = compiler_args([*common, "/first/lib.rs", "-C", "metadata=abc"], env)
        right = compiler_args([*common, "/second/lib.rs", "-Cmetadata=def"], env)
        self.assertEqual(left[-1], right[-1])
        self.assertEqual(left[:-2], [*common, "/first/lib.rs"])
        self.assertEqual(right[:-2], [*common, "/second/lib.rs"])
        changed = compiler_args(
            [*common, "-Cmetadata=abc"], {**env, "CARGO_PKG_VERSION": "2"}
        )
        self.assertNotEqual(left[-1], changed[-1])
        changed = compiler_args([*common, "--cfg", "extra", "-Cmetadata=abc"], env)
        self.assertNotEqual(left[-1], changed[-1])

    def test_compiler_queries_are_unchanged(self) -> None:
        """Cargo can query the real compiler before package variables are available."""
        self.assertEqual(compiler_args(["-vV"], {}), ["-vV"])


if __name__ == "__main__":
    unittest.main()

"""Generate supplements and check Python/JS expectations and native/WASM parity."""

import argparse
import json
import os
import subprocess
import sys
import unittest
from pathlib import Path
from typing import Any

from supplement_fixtures import generate

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(
    0, os.environ.get("BARCODE_PYTHON_PACKAGE", str(ROOT / "bindings/python/src"))
)
from test_api_consumer import check  # noqa: E402 - select source or installed package


def compare(native: list[dict[str, Any]], wasm: list[dict[str, Any]]) -> None:
    """Require equal metadata and geometry within floating-point tolerance."""
    checks = unittest.TestCase()
    checks.assertEqual(len(native), len(wasm))
    for first, second in zip(native, wasm, strict=True):
        a, b = dict(first), dict(second)
        left, right = a.pop("barcodes"), b.pop("barcodes")
        checks.assertEqual(a, b)
        checks.assertEqual(len(left), len(right), a)
        for native_read, wasm_read in zip(left, right, strict=True):
            x, y = dict(native_read), dict(wasm_read)
            px, py = x.pop("polygon"), y.pop("polygon")
            checks.assertEqual(x, y, a)
            for p, q in zip(px, py, strict=True):
                for c, d in zip(p, q, strict=True):
                    checks.assertAlmostEqual(c, d, delta=0.0001, msg=str(a))


def main() -> None:
    """Run reproducible supplement integration checks from one command."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--encoder", default=os.environ.get("ZINT", "zint"))
    parser.add_argument(
        "--output", type=Path, default=ROOT / "build/supplement-fixtures"
    )
    parser.add_argument("--library-dir")
    args = parser.parse_args()
    manifest = generate(args.output, args.encoder)
    native = check(manifest, args.library_dir)
    wasm = json.loads(
        subprocess.check_output(
            [
                "node",
                str(ROOT / "bindings/javascript/test/supplements.mjs"),
                str(manifest),
            ],
            text=True,
        )
    )
    compare(native, wasm)
    print(f"Supplement parity passed: {len(native)} mode/policy cases, debug on/off.")


if __name__ == "__main__":
    main()

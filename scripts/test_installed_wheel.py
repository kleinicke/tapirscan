"""Install a wheel in an isolated environment and decode with bundled libraries."""

import argparse
import json
import os
import subprocess
import tempfile
import venv
import zipfile
from pathlib import Path

from fixture_data import TEXT, fixtures

MODE_COUNT = 4

SMOKE = """
import importlib.util, json, pathlib, sys
import tapirscan
assert importlib.util.find_spec("tapirscan.pyzbar") is None
pixels = pathlib.Path(sys.argv[1]).read_bytes()
for mode in ('low', 'medium', 'high', 'very-high'):
    with tapirscan.Scanner(mode) as scanner:
        result = scanner.scan(tapirscan.PixelImage(pixels, width=480, height=180))
        if result.values != [sys.argv[2]]:
            raise AssertionError((mode, result.values))
print(json.dumps({'package': tapirscan.__file__, 'modes': 4, 'bundled': True}))
"""


def main() -> None:
    """Reject pure wheels and prove no explicit native path is required."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("wheel", type=Path)
    parser.add_argument("--fixtures", type=Path)
    args = parser.parse_args()
    wheel = args.wheel.resolve()
    with zipfile.ZipFile(wheel) as archive:
        native = [
            entry.filename
            for entry in archive.infolist()
            if "/_native/" in entry.filename and not entry.is_dir()
        ]
        if len(native) != MODE_COUNT:
            msg = f"Expected four bundled libraries, found {native}"
            raise RuntimeError(msg)
        metadata = next(n for n in archive.namelist() if n.endswith(".dist-info/WHEEL"))
        if b"Root-Is-Purelib: false" not in archive.read(metadata):
            msg = "Native wheels must not be labelled pure Python"
            raise RuntimeError(msg)
    with tempfile.TemporaryDirectory(prefix="tapirscan-wheel-") as temporary:
        root = Path(temporary)
        venv.EnvBuilder(with_pip=True).create(root / "venv")
        python = (
            root / "venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
        )
        subprocess.run([str(python), "-m", "pip", "install", str(wheel)], check=True)
        image = root / "barcode.raw"
        image.write_bytes(next(fixtures())[1])
        env = os.environ.copy()
        env.pop("TAPIRSCAN_LIBRARY_DIR", None)
        env.pop("PYTHONPATH", None)
        output = subprocess.check_output(
            [str(python), "-I", "-c", SMOKE, str(image), TEXT],
            cwd=root,
            env=env,
            text=True,
        )
        result = json.loads(output)
        if (
            not Path(result["package"])
            .resolve()
            .is_relative_to((root / "venv").resolve())
        ):
            msg = "Smoke test imported outside the isolated environment"
            raise RuntimeError(msg)
        print(output.strip())
        if args.fixtures:
            consumer = root / "consumer.py"
            consumer.write_bytes(
                (Path(__file__).parent / "test_api_consumer.py").read_bytes()
            )
            subprocess.run(
                [str(python), "-I", str(consumer), str(args.fixtures.resolve())],
                cwd=root,
                env=env,
                check=True,
            )


if __name__ == "__main__":
    main()

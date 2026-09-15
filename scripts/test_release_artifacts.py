"""Exercise rejection of mixed builds and incomplete release wheels."""

import json
import os
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

from collect_release import download, wheel_platform


class ReleaseArtifacts(unittest.TestCase):
    """Keep release validation strict while accepting repaired wheel directories."""

    def test_wheel_contents(self) -> None:
        """A directory entry is harmless; a missing native library is not."""
        with tempfile.TemporaryDirectory() as temporary:
            wheel = (
                Path(temporary) / "tapirscan-1.0.0-py3-none-manylinux_2_28_x86_64.whl"
            )
            for modes in (("low", "medium", "high", "very_high"), ("low",)):
                with zipfile.ZipFile(wheel, "w") as archive:
                    archive.writestr("tapirscan/_native/", "")
                    archive.writestr(
                        "tapirscan-1.0.0.dist-info/METADATA",
                        "Name: tapirscan\nVersion: 1.0.0\n",
                    )
                    for mode in modes:
                        archive.writestr(
                            f"tapirscan/_native/libtapirscan_{mode}.so", b""
                        )
                if len(modes) == 1:
                    with self.assertRaisesRegex(SystemExit, "Missing medium"):
                        wheel_platform(wheel, "1.0.0")
                else:
                    self.assertEqual(
                        wheel_platform(wheel, "1.0.0"), "manylinux_2_28_x86_64"
                    )
                    with self.assertRaisesRegex(SystemExit, "Unexpected wheel"):
                        wheel_platform(wheel, "1.0.1")

    def test_reject_untrusted_build(self) -> None:
        """Do not download a failed, PR, wrong-workflow or different-commit build."""
        good = {
            "conclusion": "success",
            "head_sha": "expected",
            "path": ".github/workflows/ci.yml",
            "event": "push",
        }
        with patch.dict(
            os.environ,
            {
                "CI_RUN": "123",
                "GITHUB_REPOSITORY": "owner/repo",
                "GITHUB_SHA": "expected",
            },
        ):
            for key, value in (
                ("conclusion", "failure"),
                ("head_sha", "other"),
                ("path", ".github/workflows/unrelated.yml"),
                ("event", "pull_request"),
            ):
                with (
                    patch(
                        "collect_release.subprocess.check_output",
                        return_value=json.dumps(good | {key: value}).encode(),
                    ),
                    patch("collect_release.subprocess.run") as run,
                    self.assertRaises(SystemExit),
                ):
                    download(Path("unused"), "CI_RUN", ".github/workflows/ci.yml", "*")
                run.assert_not_called()


if __name__ == "__main__":
    unittest.main()

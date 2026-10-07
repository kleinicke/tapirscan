"""Check immutable release identity without pushing anything."""

import json
import unittest
from unittest.mock import patch

from release_tag import check


class ReleaseTag(unittest.TestCase):
    """Reject incorrect versions, checkout identities and conflicting tags."""

    def test_identity(self) -> None:
        """Existing lightweight and annotated tags must resolve to this commit."""
        with patch(
            "release_tag.Path.read_text", return_value=json.dumps({"version": "1.3.0"})
        ):
            for refs in (
                "",
                "sha refs/tags/v1.3.0",
                "object refs/tags/v1.3.0\nsha refs/tags/v1.3.0^{}",
            ):
                with patch(
                    "release_tag.subprocess.check_output", side_effect=["sha\n", refs]
                ):
                    self.assertEqual(check("1.3.0", "sha"), ("v1.3.0", bool(refs)))
            for outputs in (["wrong\n"], ["sha\n", "other refs/tags/v1.3.0"]):
                with (
                    patch("release_tag.subprocess.check_output", side_effect=outputs),
                    self.assertRaises(SystemExit),
                ):
                    check("1.3.0", "sha")
            for version in ("1.2.2", "v1.3.0", "1.3.0;bad"):
                with self.assertRaises(SystemExit):
                    check(version, "sha")


if __name__ == "__main__":
    unittest.main()

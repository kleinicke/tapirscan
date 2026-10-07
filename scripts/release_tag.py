"""Check release identity and create an immutable tag after approval."""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

from collect_release import fail

ROOT = Path(__file__).resolve().parents[1]


def check(version: str, commit: str) -> tuple[str, bool]:
    """Reject mismatched versions, checkouts and existing tags."""
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        fail("Release version must be X.Y.Z without a v prefix")
    package = json.loads((ROOT / "bindings/javascript/package.json").read_text())
    if version != package["version"]:
        fail("Release version does not match package manifests")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if head != commit:
        fail("Checkout does not match the workflow commit")
    tag = f"v{version}"
    refs = subprocess.check_output(
        ["git", "ls-remote", "origin", f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"],
        text=True,
    )
    remote = dict(line.split()[::-1] for line in refs.splitlines())
    target = remote.get(f"refs/tags/{tag}^{{}}", remote.get(f"refs/tags/{tag}"))
    if target is not None and target != commit:
        fail(f"Tag {tag} already points to a different commit")
    return tag, target is not None


def main() -> None:
    """Only the approval job may push the tag; preflight just checks it."""
    if sys.argv[1:] not in (["check"], ["create"]):
        fail("Usage: release_tag.py check|create")
    commit = os.environ["GITHUB_SHA"]
    tag, exists = check(os.environ["RELEASE_VERSION"], commit)
    if sys.argv[1] == "create" and not exists:
        # No force: a racing or conflicting tag creation must fail safely.
        subprocess.run(
            ["git", "push", "origin", f"{commit}:refs/tags/{tag}"], check=True
        )
    print(f"Release {tag}: {commit}")


if __name__ == "__main__":
    main()

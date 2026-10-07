"""Keep research archives and dated reports out of the release tree."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RESEARCH_ROOTS = (
    "historical",
    "experiments",
    "retained",
    "runs",
    "datasets",
    "core/experiments",
)


def violations(root: Path) -> list[str]:
    """Check actual inputs, including newly added files before they are tracked."""
    found = [name for name in RESEARCH_ROOTS if (root / name).exists()]
    found.extend(
        str(path.relative_to(root))
        for path in (root / "docs").glob("*.md")
        if re.search(r"_20\d{6}\.md$", path.name)
    )
    return sorted(found)


def main() -> None:
    """Fail with exact paths; Turbo variants and production diagnostics are allowed."""
    if found := violations(ROOT):
        raise SystemExit("Research-only files in release tree: " + ", ".join(found))
    print("Production/research boundary verified")


if __name__ == "__main__":
    main()

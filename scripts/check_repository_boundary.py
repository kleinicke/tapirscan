"""Keep research archives and known unused prototypes out of the release tree."""

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
UNUSED_MODULES = (
    "neural_input",
    "sampling_abi",
    "region_abi",
    "row_scan",
    "row_group",
    "band_association",
    "run_continuity",
    "orientation",
    "localize",
    "preprocess",
    "pyramid",
    "rgba",
    "enhance",
    "warp",
)


def violations(root: Path) -> list[str]:
    """Check actual inputs, including newly added files before they are tracked."""
    found = [name for name in RESEARCH_ROOTS if (root / name).exists()]
    found.extend(
        f"core/src/{name}.rs"
        for name in UNUSED_MODULES
        if (root / f"core/src/{name}.rs").exists()
    )
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

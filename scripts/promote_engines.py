#!/usr/bin/env python3
"""Select a validated scanner revision under a new, unused engine tag.

Usage: promote_engines.py TAG --description TEXT
           [--demo-version V --demo-label L] [--dry-run]

TAG is the shared suffix, for example ``all-1d-20261006``; engines become
``<mode>-TAG.wasm`` and ``experimental-turbo<n>-TAG.wasm``. The script refuses any
name already used by a recorded WASM manifest, the demo registry or a local asset,
renames the selection in provenance and the JavaScript package, records the WASM
identities (``build_wasm.py --record``) and writes a runtime source snapshot. With
``--demo-version`` it also registers the build as the demo's next readers.
Run the quality gate and commit afterwards.
"""

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
# Tracked runtime sources added to the snapshot when new files appear there.
RUNTIME_DIRS = (
    "core/src",
    "bindings/rust/src",
    "bindings/rust/api",
    "bindings/c/src",
    "bindings/wasm/src",
)
# Files that name the selected engines.
PACKAGE_FILES = (
    "bindings/javascript/src/index.ts",
    "bindings/javascript/src/browser-worker.ts",
    "bindings/javascript/package.json",
)


def engine_names(selection: dict[str, Any], tag: str) -> list[str]:
    """Return every engine file name the selection would use under ``tag``."""
    names = [f"{entry['mode']}-{tag}.wasm" for entry in selection["modes"]]
    names += [
        f"experimental-turbo{entry['preset']}-{tag}.wasm"
        for entry in selection["experimentalTurbo"]
    ]
    return names


def used_names(root: Path) -> set[str]:
    """Collect engine names recorded anywhere: manifests, demo registry, assets."""
    used = {path.name for path in (root / "bindings/javascript/wasm").glob("*.wasm")}
    for manifest in (root / "provenance").glob("wasm-*.json"):
        data = json.loads(manifest.read_text())
        for entry in data.get("modes", []) + data.get("experimentalTurbo", []):
            used.add(entry["file"])
    demo = root / "demo/src/lib"
    registry = demo / "scanner-versions.json"
    if registry.exists():
        for version in json.loads(registry.read_text())["versions"]:
            used.update(entry["file"] for entry in version["modes"])
    turbo = demo / "turbo.json"
    if turbo.exists():
        data = json.loads(turbo.read_text())
        for entry in [data, *data.get("previous", []), *data.get("variants", [])]:
            used.add(entry["file"])
    return used


def collisions(root: Path, selection: dict[str, Any], tag: str) -> list[str]:
    """Return reasons ``tag`` cannot be used; empty when it is new everywhere."""
    found = sorted(set(engine_names(selection, tag)) & used_names(root))
    problems = [f"engine name already used: {name}" for name in found]
    problems.extend(
        f"provenance file exists: {path}"
        for path in (f"provenance/{tag}.json", f"provenance/wasm-{tag}.json")
        if (root / path).exists()
    )
    return problems


def current_suffix(selection: dict[str, Any]) -> str:
    """Return the shared tag suffix of the selected engines."""
    suffixes: set[str] = {
        entry["tag"].removeprefix(f"{entry['mode']}-") for entry in selection["modes"]
    }
    suffixes |= {
        entry["tag"].removeprefix(f"experimental-turbo{entry['preset']}-")
        for entry in selection["experimentalTurbo"]
    }
    if len(suffixes) != 1:
        msg = f"Selected engines do not share one tag suffix: {sorted(suffixes)}"
        raise SystemExit(msg)
    return suffixes.pop()


def runtime_snapshot(
    root: Path, previous: dict[str, Any], description: str
) -> dict[str, Any]:
    """Hash the previous snapshot's files plus new tracked runtime sources."""
    tracked = subprocess.run(
        ["git", "ls-files"], cwd=root, capture_output=True, text=True, check=True
    ).stdout.split()
    files = {path for path in previous["files"] if path in tracked}
    files |= {path for path in tracked if path.startswith(RUNTIME_DIRS)}
    head = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    return {
        "schema": 1,
        "baselineCommit": head,
        "description": description,
        "files": {
            path: hashlib.sha256((root / path).read_bytes()).hexdigest()
            for path in sorted(files)
        },
    }


def register_demo(
    root: Path, manifest: dict[str, Any], version: str, label: str
) -> None:
    """Add the recorded build to the demo registry and select it as the next readers."""
    path = root / "demo/src/lib/scanner-versions.json"
    registry = json.loads(path.read_text())
    if any(entry["version"] == version for entry in registry["versions"]):
        msg = f"Demo version already registered: {version}"
        raise SystemExit(msg)
    registry["versions"].insert(
        0,
        {
            "version": version,
            "label": label,
            "sourceDigest": manifest["sourceDigest"],
            "modes": manifest["modes"],
        },
    )
    path.write_text(json.dumps(registry, indent=2, ensure_ascii=False) + "\n")
    comparison = root / "demo/src/lib/comparison.ts"
    text, count = re.subn(
        r'const nextRelease = "[^"]*";',
        f'const nextRelease = "{version}";',
        comparison.read_text(),
    )
    if count != 1:
        msg = "Cannot find nextRelease in demo/src/lib/comparison.ts"
        raise SystemExit(msg)
    comparison.write_text(text)


def main() -> None:
    """Rename, record and snapshot the selected engines under a new tag."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("--description", required=True)
    parser.add_argument("--demo-version")
    parser.add_argument("--demo-label")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9]+(-[a-z0-9]+)*-20\d{6}", args.tag):
        parser.error("TAG must look like name-yyyymmdd (lowercase)")
    if bool(args.demo_version) != bool(args.demo_label):
        parser.error("--demo-version and --demo-label go together")

    selection_path = ROOT / "provenance/modes.json"
    selection = json.loads(selection_path.read_text())
    if problems := collisions(ROOT, selection, args.tag):
        raise SystemExit("\n".join(problems))
    old = current_suffix(selection)
    print(f"Renaming selected engines {old} -> {args.tag}")
    if args.dry_run:
        print("\n".join(engine_names(selection, args.tag)))
        return

    for relative in PACKAGE_FILES:
        path = ROOT / relative
        text = path.read_text()
        if old not in text:
            msg = f"{relative} does not name the selected engines ({old})"
            raise SystemExit(msg)
        path.write_text(text.replace(old, args.tag))
    previous = json.loads((ROOT / selection["runtimeRevision"]).read_text())
    for entry in selection["modes"] + selection["experimentalTurbo"]:
        entry["tag"] = entry["tag"].replace(old, args.tag)
    selection["apiWasm"] = f"provenance/wasm-{args.tag}.json"
    selection["runtimeRevision"] = f"provenance/{args.tag}.json"
    selection_path.write_text(json.dumps(selection, indent=2) + "\n")

    subprocess.run(
        [sys.executable, str(ROOT / "scripts/build_wasm.py"), "--record"],
        cwd=ROOT,
        check=True,
    )
    snapshot = runtime_snapshot(ROOT, previous, args.description)
    (ROOT / selection["runtimeRevision"]).write_text(
        json.dumps(snapshot, indent=2) + "\n"
    )
    print(f"Recorded {selection['apiWasm']} and {selection['runtimeRevision']}")
    if args.demo_version:
        manifest = json.loads((ROOT / selection["apiWasm"]).read_text())
        register_demo(ROOT, manifest, args.demo_version, args.demo_label)
        print(f"Demo next readers: {args.demo_version}")
    print("Next: node tools/quality/all.mjs, then commit. Deploying is separate.")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Assemble a self-contained Cargo package from verified scanner recipes."""

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

from build_support import verify_source_hashes

from build import MODE_CONFIG, ROOT, prepare_native_source, recipe_manifest


def copy_module(
    source: Path,
    destination: Path,
    namespace: str,
    features: set[str],
    aliases: dict[str, str],
) -> dict[str, bool]:
    """Relocate crate paths and freeze recipe features; keep target cfgs intact."""
    flags: dict[str, bool] = {}

    def flag(match: re.Match[str]) -> str:
        name = f"tapirscan_{namespace}_{match[1].replace('-', '_')}"
        flags[name] = match[1] in features
        return name

    destination.mkdir(parents=True)
    for path in sorted(source.rglob("*.rs")):
        if "bin" in path.relative_to(source).parts:
            continue
        text = path.read_text().replace("crate::", f"crate::engine::{namespace}::")
        for name, target in aliases.items():
            replacement = (
                target if target.startswith("crate::") else f"crate::engine::{target}"
            )
            text = text.replace(f"{name}::", f"{replacement}::")
        text = re.sub(
            r'\bfeature\s*=\s*"([^"]+)"',
            flag,
            text,
        )
        # Rust-only modules must not export duplicate ABI symbols.
        text = re.sub(r"(?m)^[ \t]*#\[no_mangle\]\n", "", text)
        # The pinned test compares hypot with sqrt(x*x + y*y), whose final bit
        # can differ across libm implementations. Preserve exact accept/reject
        # decisions and permit only rounding error in accepted weights.
        if namespace == "core_very_high" and path.name == "stripes.rs":
            original = "assert_eq!(edge_weight(dx, dy, ax, ay), original);"
            if text.count(original) != 1:
                msg = (
                    "Pinned edge-weight test changed; review its portability adaptation"
                )
                raise ValueError(msg)
            text = text.replace(
                original,
                """let actual = edge_weight(dx, dy, ax, ay);
                    assert_eq!(actual.is_some(), original.is_some());
                    if let (Some(actual), Some(expected)) = (actual, original) {
                        assert!(
                            (actual - expected).abs() <= 4. * f64::EPSILON * expected
                        );
                    }""",
            )
        target = destination / (
            "mod.rs" if path == source / "lib.rs" else path.relative_to(source)
        )
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
    fixtures = source.parent / "fixtures"
    if fixtures.exists():
        shutil.copytree(fixtures, destination.parent / "fixtures", dirs_exist_ok=True)

    return flags


def prepare(destination: Path, *, refresh: bool = False) -> None:
    """Reproduce sources afresh rather than trusting an existing build directory."""
    if destination.exists() and not refresh:
        raise FileExistsError(destination)
    subprocess.run([sys.executable, str(ROOT / "scripts/verify_import.py")], check=True)
    destination.mkdir(parents=True, exist_ok=True)
    for name in ("api", "tests", "examples", "generated"):
        existing = destination / name
        if existing.exists():
            shutil.rmtree(existing)
    binding = ROOT / "bindings/rust"
    for name in ["Cargo.toml", "README.md"]:
        shutil.copy2(binding / name, destination / name)
    if (binding / "Cargo.lock").exists():
        shutil.copy2(binding / "Cargo.lock", destination / "Cargo.lock")
    shutil.copy2(ROOT / "LICENSE", destination / "LICENSE")
    shutil.copytree(binding / "api", destination / "api")
    shutil.copytree(binding / "tests", destination / "tests")
    shutil.copytree(binding / "examples", destination / "examples")
    generated = destination / "generated"
    recipes = destination.parent / (destination.name + "-recipes")
    modules = []
    flags = {}
    for mode in MODE_CONFIG:
        name = mode["mode"].replace("-", "_")
        out = recipes / name
        recipe = recipe_manifest(mode["recipe"])
        verify_source_hashes(ROOT / "core", recipe["baseHashes"])
        verify_source_hashes(ROOT, recipe.get("externalHashes", {}))
        if not out.exists():
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "core/experiments/build_guarded.py"),
                    "--recipe",
                    mode["recipe"],
                    "--out",
                    str(out),
                    "--prepare-only",
                ],
                check=True,
            )
        verify_source_hashes(
            out / "temporarysource", recipe["baseHashes"] | recipe["targetHashes"]
        )
        prepare_native_source(out)
        features = set(recipe["expandedFeatures"])
        metadata = json.loads(
            subprocess.check_output(
                [
                    "cargo",
                    "metadata",
                    "--offline",
                    "--no-deps",
                    "--format-version",
                    "1",
                    "--manifest-path",
                    str(out / "native-core/Cargo.toml"),
                ],
                text=True,
            )
        )
        definitions = metadata["packages"][0]["features"]
        while (
            dependencies := {
                child for feature in features for child in definitions[feature]
            }
            - features
        ):
            features.update(dependencies)
        core = f"core_{name}"
        flags.update(
            copy_module(
                out / "native-core/src",
                generated / core,
                core,
                features,
                {"barcode_multiformat": "multiformat"},
            )
        )
        flags.update(
            copy_module(
                binding / "src",
                generated / name,
                name,
                {mode["mode"]},
                {
                    "barcode_research_core": core,
                    "recovery_core": "core_low",
                    "barcode_multiformat": "multiformat",
                    "scanner_types": "crate::types",
                    "scanner_timer": "crate::timer",
                },
            )
        )
        gate = f"#[cfg(tapirscan_mode_{name})]"
        modules.extend(
            [
                f"pub(crate) mod {core};"
                if name == "low"
                else f"{gate}\npub(crate) mod {core};",
                f"{gate}\npub(crate) mod {name};",
            ]
        )
    flags.update(
        copy_module(
            ROOT / "multiformat/src",
            generated / "multiformat",
            "multiformat",
            set(),
            {},
        )
    )
    modules.append("pub(crate) mod multiformat;")
    (generated / "mod.rs").write_text("\n".join(modules) + "\n")
    entries = [
        f'    ("{name}", {str(enabled).lower()}),'
        for name, enabled in sorted(flags.items())
    ]
    mode_entries = [
        f'    ("tapirscan_mode_{m["mode"].replace("-", "_")}", '
        f'"CARGO_FEATURE_MODE_{m["mode"].replace("-", "_").upper()}"),'
        for m in MODE_CONFIG
    ]
    (destination / "build.rs").write_text(
        "const FLAGS: &[(&str, bool)] = &[\n"
        + "\n".join(entries)
        + "\n];\n"
        + "const MODES: &[(&str, &str)] = &[\n"
        + "\n".join(mode_entries)
        + "\n];\n"
        + "fn main() {\n"
        + "    let explicit = MODES.iter()\n"
        + "        .any(|(_, feature)| std::env::var_os(feature).is_some());\n"
        + "    for (name, feature) in MODES {\n"
        + '        println!("cargo::rustc-check-cfg=cfg({name})");\n'
        + '        println!("cargo::rerun-if-env-changed={feature}");\n'
        + "        if !explicit || std::env::var_os(feature).is_some() {\n"
        + '            println!("cargo::rustc-cfg={name}");\n'
        + "        }\n    }\n"
        + '    println!("cargo::rerun-if-changed=build.rs");\n'
        + "    for (name, enabled) in FLAGS {\n"
        + '        println!("cargo::rustc-check-cfg=cfg({name})");\n'
        + '        if *enabled { println!("cargo::rustc-cfg={name}"); }\n'
        + "    }\n}\n"
    )
    provenance = {
        "modes": MODE_CONFIG,
        "adaptations": [
            "crate paths relocated into private modules",
            "recipe features fixed as private namespaced cfg flags",
            "C exports use Rust symbol mangling",
            (
                "edge-weight test preserves exact decisions with "
                "4-epsilon relative weight tolerance across libm implementations"
            ),
            "low core reused for recovery; secondary detector visibility widened",
        ],
        "sourceHashes": {
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for folder in [
                binding / "src",
                binding / "api",
                binding / "tests",
                binding / "examples",
                ROOT / "multiformat/src",
            ]
            for p in sorted(folder.rglob("*.rs"))
        },
        "packagingSourceHashes": {
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in [
                Path(__file__),
                binding / "Cargo.toml",
                binding / "Cargo.lock",
                binding / "README.md",
                ROOT / "LICENSE",
            ]
        },
        "buildScriptSha256": hashlib.sha256(
            (destination / "build.rs").read_bytes()
        ).hexdigest(),
        "generatedHashes": {
            str(p.relative_to(destination)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(generated.rglob("*"))
            if p.is_file()
        },
    }
    (destination / "provenance.json").write_text(
        json.dumps(provenance, indent=2) + "\n"
    )
    print(destination)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "destination", type=Path, nargs="?", default=ROOT / "build/crates/tapirscan"
    )
    parser.add_argument(
        "--refresh",
        action="store_true",
        help="Reverify recipes and refresh sources, keeping Cargo build caches",
    )
    args = parser.parse_args()
    prepare(args.destination.resolve(), refresh=args.refresh)

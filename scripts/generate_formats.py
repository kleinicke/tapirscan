#!/usr/bin/env python3
"""Generate synchronized format and content-kind declarations for every binding."""
# ruff: noqa: E501

import argparse
import difflib
import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / "config/formats.json"
OUTPUTS = {
    ROOT / "multiformat/src/formats.rs": "rust_bits",
    ROOT / "bindings/rust/src/format_registry.rs": "rust_registry",
    ROOT / "bindings/rust/api/format.rs": "rust_format",
    ROOT / "bindings/javascript/src/multiformat/format-registry.ts": "typescript",
    ROOT / "bindings/python/src/tapirscan/_format_registry.py": "python",
    ROOT / "bindings/c/include/tapirscan_formats.h": "c",
    ROOT / "bindings/cpp/include/tapirscan/format.hpp": "cpp",
    ROOT / "bindings/java/src/main/java/org/tapirscan/Format.java": "java",
    ROOT
    / "bindings/android/src/main/java/org/tapirscan/android/Format.java": "android",
    ROOT / "bindings/swift/Sources/Tapirscan/Format.swift": "swift",
}
CONTENT_CONFIG = ROOT / "config/content_kinds.json"
CONTENT_OUTPUTS = {
    ROOT / "bindings/rust/api/content_kind.rs": "rust",
    ROOT / "bindings/javascript/src/content-kind.ts": "typescript",
    ROOT / "bindings/python/src/tapirscan/_content_registry.py": "python",
    ROOT / "bindings/c/include/tapirscan_content_kinds.h": "c",
    ROOT / "bindings/cpp/include/tapirscan/content_kind.hpp": "cpp",
    ROOT / "bindings/java/src/main/java/org/tapirscan/ContentKind.java": "java",
    ROOT
    / "bindings/android/src/main/java/org/tapirscan/android/ContentKind.java": "android",
    ROOT / "bindings/swift/Sources/Tapirscan/ContentKind.swift": "swift",
}
EXPERIMENTAL = "Experimental: may change in minor releases."
PRESETS = (
    ("retail", "RETAIL", "EAN-13, UPC-A, EAN-8 and UPC-E."),
    (
        "common1D",
        "COMMON_1D",
        "Retail formats plus Code 128, Code 39 and ITF.",
    ),
    ("common", "COMMON", "Common linear formats plus QR Code and Data Matrix."),
    (
        "1D",
        "LINEAR",
        "All linear formats except the opt-in ones, including DataBar variants.",
    ),
    ("2D", "MATRIX", "QR Code, Data Matrix, PDF417, Aztec and MaxiCode."),
)


def validate_registry(registry: dict[str, Any]) -> None:  # noqa: C901
    """Reject ambiguous or incomplete registry values."""
    formats = registry["formats"]
    names = [item[0] for item in formats]
    variants = [item[1] for item in formats]
    bits = [item[2] for item in formats]
    if (
        not formats
        or len(set(names)) != len(formats)
        or len(set(variants)) != len(formats)
    ):
        msg = "Format names and Rust variants must be unique"
        raise ValueError(msg)
    if any(not isinstance(bit, int) or bit <= 0 or bit & (bit - 1) for bit in bits):
        msg = "Format bits must be positive, distinct powers of two"
        raise ValueError(msg)
    if len(set(bits)) != len(bits):
        msg = "Format bits must be unique"
        raise ValueError(msg)
    presets = registry["presets"]
    if set(presets) != {"retail", "common1D", "common", "1D", "2D"}:
        msg = "Format presets must match the public selector names"
        raise ValueError(msg)
    for preset, members in presets.items():
        if (
            not members
            or len(members) != len(set(members))
            or any(n not in names for n in members)
        ):
            msg = f"Invalid format preset: {preset}"
            raise ValueError(msg)
    for child, parent in [
        ("common1D", "retail"),
        ("common", "common1D"),
        ("1D", "common1D"),
    ]:
        if presets[child][: len(presets[parent])] != presets[parent]:
            msg = f"{parent} must be an ordered prefix of {child}"
            raise ValueError(msg)
    flags = list(registry["engineFlags"].values())
    if (
        len(flags) != len(set(flags))
        or any(
            not isinstance(flag, int) or flag <= 0 or flag & (flag - 1)
            for flag in flags
        )
        or any(flag in bits for flag in flags)
    ):
        msg = "Engine flags must be distinct from formats and each other"
        raise ValueError(msg)
    linear = set(registry["presets"]["1D"])
    matrix = set(registry["presets"]["2D"])
    opt_in = registry["optIn"]
    if len(opt_in) != len(set(opt_in)) or any(n not in names for n in opt_in):
        msg = "Invalid opt-in format list"
        raise ValueError(msg)
    engine_opt_in = registry["engineOptIn"]
    if any(n not in opt_in for n in engine_opt_in):
        msg = "Engine opt-in formats must be opt-in formats"
        raise ValueError(msg)
    if (
        linear & matrix
        or (linear | matrix) & set(opt_in)
        or linear | matrix | set(opt_in) != set(names)
    ):
        msg = "1D, 2D and opt-in formats must partition every format"
        raise ValueError(msg)


def load_registry(path: Path = CONFIG) -> dict[str, Any]:
    """Load and validate the compact registry schema."""
    registry = json.loads(path.read_text())
    validate_registry(registry)
    return registry


def format_bits(registry: dict[str, Any]) -> dict[str, int]:
    """Map each format name to its native bit."""
    return {str(name): int(bit) for name, _, bit, _ in registry["formats"]}


def mask(registry: dict[str, Any], names: list[str]) -> int:
    """Combine the bits of the named formats."""
    bits = format_bits(registry)
    return sum(bits[name] for name in names)


def rust_registry_source(registry: dict[str, Any]) -> str:
    """Render the masks consumed by the private facade."""
    presets = registry["presets"]
    # The engine reads the 1D preset plus the opt-in linear formats with their own
    # reader; the public API translates the other opt-in formats before scanning.
    linear = presets["1D"] + registry["engineOptIn"]
    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.

pub const LINEAR_MASK: u32 = {mask(registry, linear):_};
pub const ALL_FORMATS_MASK: u32 = {mask(registry, linear + presets["2D"]):_};
/// Opt-in linear formats with their own reader, read only when selected.
pub const ENGINE_OPT_IN_MASK: u32 = {mask(registry, registry["engineOptIn"]):_};
"""


def rust_bits_source(registry: dict[str, Any]) -> str:
    """Render the per-format bits and preset masks named by the readers."""
    presets = registry["presets"]
    flags = registry["engineFlags"]
    formats = "\n".join(
        f"/// {text}\npub const {constant(variant)}: u32 = {bit:_};"
        for _, variant, bit, text in registry["formats"]
    )
    linear = presets["1D"] + registry["engineOptIn"]
    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.
//! Native format-mask bits and the preset masks the readers test.

{formats}

/// EAN-13, UPC-A, EAN-8 and UPC-E.
pub const RETAIL_MASK: u32 = {mask(registry, presets["retail"]):_};
/// Retail formats plus Code 128, Code 39 and ITF.
pub const COMMON_LINEAR_MASK: u32 = {mask(registry, presets["common1D"]):_};
/// QR Code, Data Matrix, PDF417, Aztec and `MaxiCode`.
pub const MATRIX_MASK: u32 = {mask(registry, presets["2D"]):_};
/// Every linear format the engine reads: the 1D preset plus the opt-in formats
/// with their own reader.
pub const ENGINE_LINEAR_MASK: u32 = {mask(registry, linear):_};
/// Attach a confirmed EAN/UPC add-on when one is found.
pub const EAN_ADDON_READ_FLAG: u32 = {flags["eanAddOnRead"]:_};
/// Accept EAN/UPC reads only with a confirmed add-on.
pub const EAN_ADDON_REQUIRE_FLAG: u32 = {flags["eanAddOnRequire"]:_};
"""


def rust_format_source(registry: dict[str, Any]) -> str:
    """Render the documented public Rust format enum."""
    presets = registry["presets"]
    variants = []
    names = []
    every = "".join(f"\n        Self::{item[1]}," for item in registry["formats"])
    for name, variant, bit, description in registry["formats"]:
        rename = f'    #[serde(rename = "{name}")]\n' if name != variant else ""
        variants.append(f"{rename}    /// {description}\n    {variant} = {bit:_},")
        names.append(f'            Self::{variant} => "{name}",')
    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.

/// A supported barcode symbology. Discriminants are native format-mask bits.
///
/// New formats may be added in minor releases.
#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize, Eq, PartialEq)]
#[non_exhaustive]
#[repr(u32)]
pub enum Format {{
{chr(10).join(variants)}
}}

impl Format {{
    /// Every format in native bit order, including the opt-in formats.
    pub const ALL: &[Self] = &[{every}
    ];

    /// Stable schema name used by language adapters.
    #[must_use]
    pub const fn as_str(self) -> &'static str {{
        match self {{
{chr(10).join(names)}
        }}
    }}
}}

impl std::fmt::Display for Format {{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        formatter.write_str(self.as_str())
    }}
}}

pub(crate) const RETAIL_MASK: u32 = {mask(registry, presets["retail"]):_};
pub(crate) const COMMON_LINEAR_MASK: u32 = {mask(registry, presets["common1D"]):_};
pub(crate) const COMMON_MASK: u32 = {mask(registry, presets["common"]):_};
pub(crate) const LINEAR_MASK: u32 = {mask(registry, presets["1D"]):_};
pub(crate) const MATRIX_MASK: u32 = {mask(registry, presets["2D"]):_};
pub(crate) const ALL_FORMATS_MASK: u32 = LINEAR_MASK | MATRIX_MASK;
/// Opt-in formats: selected only by name, never by a preset or `ALL`.
pub(crate) const OPT_IN_MASK: u32 = {mask(registry, registry["optIn"]):_};
pub(crate) const SUPPORTED_MASK: u32 = ALL_FORMATS_MASK | OPT_IN_MASK;
"""


def typescript_source(registry: dict[str, Any]) -> str:
    """Render TypeScript declarations consumed by the public resolver."""
    formats = registry["formats"]
    presets = registry["presets"]
    flags = registry["engineFlags"]
    bits = "\n".join(f"  {name}: {bit}," for name, _, bit, _ in formats)

    def array(key: str) -> str:
        values = presets[key]
        if key == "retail":
            return f"Object.freeze({json.dumps(values, separators=(', ', ': '))})"
        rows = "\n".join(f'  "{name}",' for name in values)
        return f"Object.freeze([\n{rows}\n])"

    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.
// Frozen: these shared constants also back the scanner defaults.
export const formatBits = Object.freeze({{
{bits}
}} as const);
export type Format = keyof typeof formatBits;
export const retailFormats: readonly Format[] = {array("retail")};
export const commonLinearFormats: readonly Format[] = {array("common1D")};
export const commonFormats: readonly Format[] = {array("common")};
export const linearFormats: readonly Format[] = {array("1D")};
export const matrixFormats: readonly Format[] = {array("2D")};
export const eanAddOnReadFlag = {flags["eanAddOnRead"]};
export const eanAddOnRequireFlag = {flags["eanAddOnRequire"]};
"""


def python_source(registry: dict[str, Any]) -> str:
    """Render the Python format names, bits and preset tuples."""
    formats = registry["formats"]
    presets = registry["presets"]
    first_opt_in = registry["optIn"][0]
    names = "\n".join(f'    "{name}",' for name, _, _, _ in formats)
    bits = "\n".join(
        (
            "    # Opt-in: selected by name, in no preset.\n"
            if name == first_opt_in
            else ""
        )
        + f'    "{name}": {bit},'
        for name, _, bit, _ in formats
    )

    def preset(key: str) -> str:
        rows = "\n".join(f'    "{name}",' for name in presets[key])
        return f"(\n{rows}\n)"

    return f'''"""Generated by scripts/generate_formats.py from config/formats.json. Do not edit."""

from typing import Literal, TypeAlias

Format: TypeAlias = Literal[
{names}
]
FORMAT_BITS: dict[str, int] = {{
{bits}
}}
retail_formats: tuple[Format, ...] = {preset("retail")}
common_linear_formats: tuple[Format, ...] = {preset("common1D")}
common_formats: tuple[Format, ...] = {preset("common")}
linear_formats: tuple[Format, ...] = {preset("1D")}
matrix_formats: tuple[Format, ...] = {preset("2D")}
'''


def constant(variant: str) -> str:
    """Convert a Rust variant such as `QrCode` to `QR_CODE`."""
    return "".join(
        f"_{char}"
        if char.isupper() and index and variant[index - 1].islower()
        else char
        for index, char in enumerate(variant)
    ).upper()


def preset_masks(registry: dict[str, Any]) -> list[tuple[str, int, str]]:
    """Return each named preset with its mask and description."""
    presets = registry["presets"]
    masks = [(name, mask(registry, presets[key]), text) for key, name, text in PRESETS]
    every = mask(registry, presets["1D"] + presets["2D"])
    return [*masks, ("ALL", every, "Every format except the opt-in formats.")]


def c_source(registry: dict[str, Any]) -> str:
    """Render C format bits and preset masks."""
    formats = "\n".join(
        f"#define TAPIRSCAN_FORMAT_{constant(variant)} {bit}u /* {text} */"
        for _, variant, bit, text in registry["formats"]
    )
    presets = "\n".join(
        f"#define TAPIRSCAN_FORMATS_{name} {value}u /* {text} */"
        for name, value, text in preset_masks(registry)
    )
    opt_in = mask(registry, registry["optIn"])
    return f"""/* Generated by scripts/generate_formats.py from config/formats.json. Do not edit. */
#ifndef TAPIRSCAN_FORMATS_H
#define TAPIRSCAN_FORMATS_H
{formats}

{presets}
/* Opt-in formats ({", ".join(registry["optIn"])}): selected only by name, never by a preset or ALL. */
#define TAPIRSCAN_FORMATS_OPT_IN {opt_in}u
#endif
"""


def cpp_source(registry: dict[str, Any]) -> str:
    """Render the C++ format enum."""
    variants = "\n".join(
        f"    {variant} = TAPIRSCAN_FORMAT_{constant(variant)},"
        for _, variant, _, _ in registry["formats"]
    )
    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.
#pragma once
#include <tapirscan.h>
#include <cstdint>

namespace tapirscan {{
/// A supported barcode symbology. Values are native format-mask bits.
enum class Format : std::uint32_t {{
{variants}
}};
}}  // namespace tapirscan
"""


def java_source(registry: dict[str, Any], package: str = "org.tapirscan") -> str:
    """Render the Java format enum and preset sets for a package."""
    constants = ",\n".join(
        f'    /** {text} */\n    {constant(variant)}({bit}, "{name}")'
        for name, variant, bit, text in registry["formats"]
    )
    by_name = {
        str(name): constant(variant) for name, variant, _, _ in registry["formats"]
    }
    presets = []
    for key, preset, text in PRESETS:
        members = ", ".join(by_name[name] for name in registry["presets"][key])
        presets.append(
            f"    /** {text} */\n"
            f"    public static final Set<Format> {preset} =\n"
            f"        Collections.unmodifiableSet(EnumSet.of({members}));"
        )
    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.
package {package};

import java.util.Collections;
import java.util.EnumSet;
import java.util.Set;

/** A supported barcode symbology. */
public enum Format {{
{constants};

{chr(10).join(presets)}
    /** Every format except the opt-in formats, which are selected by name. */
    public static final Set<Format> ALL =
        Collections.unmodifiableSet(EnumSet.of({", ".join(by_name[n] for n in registry["presets"]["1D"] + registry["presets"]["2D"])}));

    private final int bit;
    private final String schemaName;

    Format(int bit, String schemaName) {{
        this.bit = bit;
        this.schemaName = schemaName;
    }}

    /** Native format-mask bit. */
    public int bit() {{
        return bit;
    }}

    /** Stable name shared with the other language bindings, such as {{@code "QRCode"}}. */
    @Override
    public String toString() {{
        return schemaName;
    }}

    static int mask(Set<Format> formats) {{
        int mask = 0;
        for (Format format : formats) mask |= format.bit;
        return mask;
    }}

    static Format fromBit(int bit) {{
        for (Format format : values()) if (format.bit == bit) return format;
        throw new IllegalStateException("Unknown native format " + bit);
    }}
}}
"""


SWIFT_CASES = {"Upca": "upcA", "Upce": "upcE"}


def lower_camel(variant: str) -> str:
    """Convert a Rust variant such as `QrCode` to the Swift case `qrCode`."""
    return SWIFT_CASES.get(variant, variant[0].lower() + variant[1:])


SWIFT_PRESETS = {
    "RETAIL": "retail",
    "COMMON_1D": "common1D",
    "COMMON": "common",
    "LINEAR": "linear",
    "MATRIX": "matrix",
    "ALL": "all",
}


def swift_source(registry: dict[str, Any]) -> str:
    """Render the Swift `Format` enum and the `Formats` option set."""
    cases = "\n".join(
        f"    /// {text}\n    case {lower_camel(variant)} = {bit}"
        for _, variant, bit, text in registry["formats"]
    )
    names = "\n".join(
        f'        case .{lower_camel(variant)}: "{name}"'
        for name, variant, _, _ in registry["formats"]
    )
    members = "\n".join(
        f"    /// {text}\n    public static let {lower_camel(variant)} = Formats(.{lower_camel(variant)})"
        for _, variant, _, text in registry["formats"]
    )
    by_name = {
        str(name): lower_camel(variant) for name, variant, _, _ in registry["formats"]
    }
    every = registry["presets"]["1D"] + registry["presets"]["2D"]
    sets = {
        constant_name: [by_name[n] for n in registry["presets"][key]]
        for key, constant_name, _ in PRESETS
    }
    sets["ALL"] = [by_name[n] for n in every]
    texts = {constant_name: text for _, constant_name, text in PRESETS}
    texts["ALL"] = "Every format except the opt-in formats, which are selected by name."
    presets = "\n".join(
        f"    /// {texts[key]}\n"
        f"    public static let {SWIFT_PRESETS[key]}: Formats = [{', '.join('.' + m for m in sets[key])}]"
        for key in SWIFT_PRESETS
    )
    return f"""// Generated by scripts/generate_formats.py from config/formats.json. Do not edit.

/// A supported barcode symbology. Raw values are native format-mask bits.
public enum Format: UInt32, CaseIterable, Sendable, CustomStringConvertible {{
{cases}

    /// Stable name shared with the other language bindings, such as `"QRCode"`.
    public var name: String {{
        switch self {{
{names}
        }}
    }}

    public var description: String {{ name }}
}}

/// A set of barcode formats. Enable only the formats you need: each 2D format
/// searches the whole image.
public struct Formats: OptionSet, Sendable {{
    public let rawValue: UInt32
    public init(rawValue: UInt32) {{ self.rawValue = rawValue }}
    public init(_ format: Format) {{ self.init(rawValue: format.rawValue) }}

{members}

{presets}
}}
"""


def validate_content_kinds(kinds: list[dict[str, Any]]) -> None:
    """Reject ambiguous content kinds or a gap in their native numbers."""
    if [kind["code"] for kind in kinds] != list(range(1, len(kinds) + 1)):
        msg = "Content kind codes must count up from 1"
        raise ValueError(msg)
    for key in ("name", "rust", "swift"):
        if len({kind[key] for kind in kinds}) != len(kinds):
            msg = f"Content kind {key} identifiers must be unique"
            raise ValueError(msg)


def load_content_kinds(path: Path = CONTENT_CONFIG) -> list[dict[str, Any]]:
    """Load and validate the content kinds."""
    kinds: list[dict[str, Any]] = json.loads(path.read_text())["kinds"]
    validate_content_kinds(kinds)
    return kinds


def kind_doc(kind: dict[str, Any]) -> str:
    """Return a kind's description, marked when it is experimental."""
    text = str(kind["description"])
    return f"{text} {EXPERIMENTAL}" if kind.get("experimental") else text


def experimental_names(kinds: list[dict[str, Any]]) -> str:
    """List the experimental kinds' schema names in prose."""
    names = [f"`{kind['name']}`" for kind in kinds if kind.get("experimental")]
    return f"{', '.join(names[:-1])} and {names[-1]}"


def rust_content_source(kinds: list[dict[str, Any]]) -> str:
    """Render the public Rust `ContentKind` enum."""
    variants = []
    for kind in kinds:
        name, variant = kind["name"], kind["rust"]
        rename = f'    #[serde(rename = "{name}")]\n' if name != variant else ""
        variants.append(
            f"    /// {kind_doc(kind)}\n{rename}    {variant} = {kind['code']},"
        )
    every = "".join(f"\n        Self::{kind['rust']}," for kind in kinds)
    names = "\n".join(
        f'            Self::{kind["rust"]} => "{kind["name"]}",' for kind in kinds
    )
    return f"""// Generated by scripts/generate_formats.py from config/content_kinds.json. Do not edit.

/// Kind of structured payload recognized in decoded text.
///
/// New kinds may be added in minor releases.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, serde::Serialize)]
#[non_exhaustive]
#[repr(u32)]
pub enum ContentKind {{
{chr(10).join(variants)}
}}

impl ContentKind {{
    /// Every kind in numeric order.
    pub const ALL: &[Self] = &[{every}
    ];

    /// Stable name shared by every binding.
    #[must_use]
    pub const fn as_str(self) -> &'static str {{
        match self {{
{names}
        }}
    }}
}}

impl std::fmt::Display for ContentKind {{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        formatter.write_str(self.as_str())
    }}
}}
"""


def typescript_content_source(kinds: list[dict[str, Any]]) -> str:
    """Render the TypeScript `ContentKind` union."""
    names = "\n".join(f'  | "{kind["name"]}"' for kind in kinds)
    return f"""// Generated by scripts/generate_formats.py from config/content_kinds.json. Do not edit.
/**
 * Kind of structured payload recognized in decoded text. Experimental kinds,
 * which may change in minor releases: {experimental_names(kinds)}.
 */
export type ContentKind =
{names};
"""


def python_content_source(kinds: list[dict[str, Any]]) -> str:
    """Render the Python `ContentKind` literal and its native numbers."""
    stable = [kind for kind in kinds if not kind.get("experimental")]
    experimental = [kind for kind in kinds if kind.get("experimental")]
    rows = [f'    "{kind["name"]}",' for kind in stable]
    if experimental:
        rows.append(f"    # {EXPERIMENTAL}")
        rows += [f'    "{kind["name"]}",' for kind in experimental]
    codes = "\n".join(f'    {kind["code"]}: "{kind["name"]}",' for kind in kinds)
    return f'''"""Generated by scripts/generate_formats.py from config/content_kinds.json.

Do not edit.
"""

from typing import Literal, TypeAlias

ContentKind: TypeAlias = Literal[
{chr(10).join(rows)}
]
CONTENT_KINDS: dict[int, ContentKind] = {{
{codes}
}}
'''


def c_content_source(kinds: list[dict[str, Any]]) -> str:
    """Render the C `tapirscan_content_kind` enum."""
    values = "\n".join(
        f"    /* {kind_doc(kind)} */\n"
        f"    TAPIRSCAN_CONTENT_{constant(kind['rust'])} = {kind['code']}"
        + ("," if kind is not kinds[-1] else "")
        for kind in kinds
    )
    return f"""/* Generated by scripts/generate_formats.py from config/content_kinds.json. Do not edit. */
#ifndef TAPIRSCAN_CONTENT_KINDS_H
#define TAPIRSCAN_CONTENT_KINDS_H
/* Kind of structured content (docs/CONTENT.md). */
typedef enum tapirscan_content_kind {{
{values}
}} tapirscan_content_kind;
#endif
"""


def cpp_content_source(kinds: list[dict[str, Any]]) -> str:
    """Render the C++ `ContentKind` enum."""
    values = "\n".join(
        f"    /// {kind_doc(kind)}\n    {kind['rust']} = TAPIRSCAN_CONTENT_{constant(kind['rust'])},"
        for kind in kinds
    )
    return f"""// Generated by scripts/generate_formats.py from config/content_kinds.json. Do not edit.
#pragma once
#include <tapirscan.h>
#include <cstdint>

namespace tapirscan {{
/// Kind of structured payload recognized by `parse_content`.
enum class ContentKind : std::uint32_t {{
{values}
}};
}}  // namespace tapirscan
"""


def javadoc(text: str) -> str:
    """Convert Markdown code spans to Javadoc."""
    return re.sub(r"`([^`]+)`", r"{@code \1}", text)


def java_content_source(
    kinds: list[dict[str, Any]], package: str = "org.tapirscan"
) -> str:
    """Render the Java `ContentKind` enum for a package."""
    constants = ",\n".join(
        f"    /** {javadoc(kind_doc(kind))} */\n"
        f'    {constant(kind["rust"])}({kind["code"]}, "{kind["name"]}")'
        for kind in kinds
    )
    # The FFM binding reads native numbers; the JNI binding receives names.
    parameter, test = (
        ("int code", "kind.code == code")
        if package == "org.tapirscan"
        else ("String name", "kind.schemaName.equals(name)")
    )
    argument = parameter.split()[1]
    return f"""// Generated by scripts/generate_formats.py from config/content_kinds.json. Do not edit.
package {package};

/** Kind of structured payload recognized by {{@link Tapirscan#parseContent}}. */
public enum ContentKind {{
{constants};

    private final int code;
    private final String schemaName;

    ContentKind(int code, String schemaName) {{
        this.code = code;
        this.schemaName = schemaName;
    }}

    /** Native {{@code tapirscan_content_kind}} value. */
    public int code() {{
        return code;
    }}

    /** Stable name shared with the other language bindings, such as {{@code "WiFi"}}. */
    @Override
    public String toString() {{
        return schemaName;
    }}

    static ContentKind of({parameter}) {{
        for (ContentKind kind : values()) if ({test}) return kind;
        throw new IllegalStateException("Unknown native content kind " + {argument});
    }}
}}
"""


def swift_content_source(kinds: list[dict[str, Any]]) -> str:
    """Render the Swift `ContentKind` enum."""
    cases = "\n".join(
        f"    /// {kind_doc(kind)}\n    case {kind['swift']} = {kind['code']}"
        for kind in kinds
    )
    names = "\n".join(
        f'        case .{kind["swift"]}: "{kind["name"]}"' for kind in kinds
    )
    return f"""// Generated by scripts/generate_formats.py from config/content_kinds.json. Do not edit.

/// Kind of structured payload recognized by `parseContent`.
public enum ContentKind: UInt32, CaseIterable, Sendable, CustomStringConvertible {{
{cases}

    /// Stable name shared with the other bindings, such as `"WiFi"`.
    public var name: String {{
        switch self {{
{names}
        }}
    }}

    public var description: String {{ name }}
}}
"""


def generated_sources(
    registry: dict[str, Any], kinds: list[dict[str, Any]]
) -> dict[Path, str]:
    """Return every generated path and its deterministic contents."""
    renderers = {
        "rust_bits": rust_bits_source,
        "rust_registry": rust_registry_source,
        "rust_format": rust_format_source,
        "typescript": typescript_source,
        "python": python_source,
        "c": c_source,
        "cpp": cpp_source,
        "java": java_source,
        "android": lambda r: java_source(r, "org.tapirscan.android"),
        "swift": swift_source,
    }
    content_renderers = {
        "rust": rust_content_source,
        "typescript": typescript_content_source,
        "python": python_content_source,
        "c": c_content_source,
        "cpp": cpp_content_source,
        "java": java_content_source,
        "android": lambda k: java_content_source(k, "org.tapirscan.android"),
        "swift": swift_content_source,
    }
    sources = {path: renderers[kind](registry) for path, kind in OUTPUTS.items()}
    for path, kind in CONTENT_OUTPUTS.items():
        sources[path] = content_renderers[kind](kinds)
    return sources


def generated_format_docs(registry: dict[str, Any]) -> dict[Path, str]:
    """Replace marked tables while preserving the surrounding package guides."""
    scopes = json.loads((ROOT / "config/format_scopes.json").read_text())
    names = [item[0] for item in registry["formats"]]
    if set(scopes) != set(names):
        msg = "Format scope descriptions must match config/formats.json"
        raise ValueError(msg)
    groups = registry["presets"] | {
        "all": [name for name in names if name not in registry["optIn"]]
    }
    rows = [
        ["Format identifier", *(f"`{group}`" for group in groups), "Scope"],
        ["---", *(":---:" for _ in groups), "---"],
    ]
    rows.extend(
        [f"`{name}`"]
        + ["✓" if name in members else "—" for members in groups.values()]
        + [scopes[name]]
        for name in names
    )
    widths = [max(len(row[i]) for row in rows) for i in range(len(rows[0]))]
    table = "\n".join(
        "| "
        + " | ".join(cell.ljust(width) for cell, width in zip(row, widths, strict=True))
        + " |"
        for row in rows
    )
    begin = "<!-- BEGIN GENERATED FORMAT MATRIX -->"
    end = "<!-- END GENERATED FORMAT MATRIX -->"
    block = f"{begin}\n<!-- Generated by scripts/generate_formats.py; do not edit. -->\n<!-- prettier-ignore -->\n{table}\n\n{end}"
    sources = {}
    for filename in (
        "README.md",
        "docs/FORMATS.md",
        "bindings/javascript/README.md",
        "bindings/python/README.md",
    ):
        path = ROOT / filename
        actual = path.read_text()
        if actual.count(begin) != 1 or actual.count(end) != 1:
            msg = f"Expected exactly one format matrix marker pair in {filename}"
            raise ValueError(msg)
        before, rest = actual.split(begin)
        _, after = rest.split(end)
        sources[path] = before + block + after
    return sources


def main() -> int:
    """Write generated declarations or report drift without changing files."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    changed = False
    registry = load_registry()
    sources = generated_sources(registry, load_content_kinds())
    sources.update(generated_format_docs(registry))
    for path, expected in sources.items():
        actual = path.read_bytes().decode() if path.exists() else ""
        if actual == expected:
            continue
        changed = True
        if args.check:
            sys.stdout.writelines(
                difflib.unified_diff(
                    actual.splitlines(keepends=True),
                    expected.splitlines(keepends=True),
                    fromfile=str(path),
                    tofile=f"{path} (generated)",
                )
            )
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(expected.encode())
    return int(args.check and changed)


if __name__ == "__main__":
    raise SystemExit(main())

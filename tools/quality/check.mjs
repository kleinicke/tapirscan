import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root } from "./format.mjs";
const mode = process.argv[2] ?? "js";
const release = fs.existsSync(path.join(root, "bindings/javascript/tsconfig.json"));
const commands = [];
if (mode === "rust") {
  const matrix = release
    ? [
        ["core/Cargo.toml", "fast"],
        ["core/Cargo.toml", "quality"],
      ]
    : [
        ["rust/barcode-core/Cargo.toml", "fast"],
        ["rust/barcode-core/Cargo.toml", "quality"],
        ["rust/decoder-sprint/Cargo.toml", null],
      ];
  for (const [manifest, feature] of matrix)
    commands.push([
      "rustup",
      [
        "run",
        "1.91.1",
        "cargo",
        "clippy",
        "--offline",
        "--manifest-path",
        manifest,
        "--all-targets",
        ...(feature ? ["--features", feature] : []),
        "--",
        "-D",
        "warnings",
        "-W",
        "clippy::pedantic",
        "-W",
        "clippy::dbg_macro",
        "-W",
        "clippy::todo",
      ],
    ]);
} else if (mode === "js") {
  commands.push([
    process.execPath,
    [
      "tools/quality/node_modules/eslint/bin/eslint.js",
      "--config",
      "tools/quality/eslint.config.mjs",
      ...(release
        ? ["bindings/javascript/src", "bindings/javascript/test", "tools/quality"]
        : ["js", "nxing-js", "tools/quality"]),
    ],
  ]);
  commands.push([
    process.execPath,
    [
      "tools/quality/node_modules/typescript/bin/tsc",
      "--noEmit",
      "-p",
      release ? "bindings/javascript/tsconfig.json" : "tsconfig.quality.json",
    ],
  ]);
} else throw new Error("Use rust or js");
let failed = false;
for (const [command, args] of commands) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, CARGO_TARGET_DIR: path.join(root, ".quality-cache/cargo") },
  });
  if (result.error) console.error(result.error.message);
  failed ||= result.status !== 0;
}
process.exitCode = failed ? 1 : 0;

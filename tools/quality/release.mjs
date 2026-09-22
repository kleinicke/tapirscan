// Strict checks for maintained release bindings; frozen imports have a separate core audit.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root } from "./format.mjs";
import { qualityEnvironment, pythonExecutable } from "./environment.mjs";
const environment = qualityEnvironment(root);
const mode = process.argv[2] ?? "all";
const supported = ["all", "rust", "python", "native", "js"];
if (!supported.includes(mode)) throw new Error(`Use ${supported.join(", ")}`);
let failed = false;
function run(command, args, extraEnv = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: "inherit",
    env: { ...environment, ...extraEnv },
  });
  if (result.error) console.error(result.error.message);
  failed ||= result.status !== 0;
  return result.status === 0;
}
const selected = (name) => mode === "all" || mode === name;
const python = selected("python") || selected("rust") ? pythonExecutable(root, environment) : null;
if (selected("js")) run(process.execPath, ["tools/quality/check.mjs", "js"]);
if (selected("python")) {
  const bin = path.join(root, ".quality-tools/python/bin");
  run(path.join(bin, "ruff"), ["check", "."]);
  run(path.join(bin, "ruff"), ["format", "--check", "."]);
  run(path.join(bin, "ty"), ["check", "--python", python]);
  run(
    path.join(bin, "mypy"),
    [
      "--python-executable",
      python,
      "--config-file",
      "bindings/python/pyproject.toml",
      "bindings/python/src/tapirscan",
      "bindings/python/tests/typecheck_api.py",
    ],
    { MYPYPATH: "bindings/python/src" },
  );
}
fs.mkdirSync(path.join(root, ".quality-cache"), { recursive: true });
if (selected("native")) {
  const warnings = [
    "-Wall",
    "-Wextra",
    "-Wpedantic",
    "-Wconversion",
    "-Wsign-conversion",
    "-Werror",
  ];
  for (const [compiler, standard, source] of [
    [process.env.CC ?? "clang", "c11", "bindings/c/tests/smoke.c"],
    [process.env.CXX ?? "clang++", "c++17", "bindings/cpp/examples/scan_raw.cpp"],
  ])
    run(compiler, [
      `-std=${standard}`,
      ...warnings,
      "-fsyntax-only",
      "-Ibindings/c/include",
      "-Ibindings/cpp/include",
      source,
    ]);
  const java = environment.JAVA_HOME ? path.join(environment.JAVA_HOME, "bin/javac") : "javac";
  run(java, [
    "--release",
    "22",
    "-Xlint:all",
    "-Werror",
    "-d",
    ".quality-cache/java",
    "bindings/java/src/main/java/org/tapirscan/Tapirscan.java",
    "bindings/java/src/test/java/org/tapirscan/Smoke.java",
  ]);
}
if (selected("rust")) {
  const modes = JSON.parse(fs.readFileSync(path.join(root, "provenance/modes.json"), "utf8")).modes;
  const parent = fs.mkdtempSync(path.join(root, ".quality-cache/lint-"));
  const publicCrate = path.join(root, "build/crates/tapirscan");
  const cargoTarget = path.join(root, "build/native-target");
  const clippy = (manifest, extra = []) =>
    run(
      "rustup",
      [
        "run",
        "1.91.1",
        "cargo",
        "clippy",
        "--offline",
        "--locked",
        "--manifest-path",
        manifest,
        "--all-targets",
        ...extra,
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
      { CARGO_TARGET_DIR: cargoTarget },
    );
  try {
    if (!run(python, ["scripts/prepare_rust.py", publicCrate, "--refresh"])) {
      process.exitCode = 1;
    } else {
      clippy(path.join(publicCrate, "Cargo.toml"));
      for (const { mode: name } of modes) {
        for (const binding of ["c", "wasm"]) {
          const dest = path.join(parent, `${binding}-${name}`);
          fs.mkdirSync(dest);
          fs.cpSync(path.join(root, `bindings/${binding}/src`), path.join(dest, "src"), {
            recursive: true,
          });
          fs.copyFileSync(path.join(publicCrate, "Cargo.lock"), path.join(dest, "Cargo.lock"));
          const template = fs.readFileSync(
            path.join(root, `bindings/${binding}/Cargo.toml.in`),
            "utf8",
          );
          fs.writeFileSync(
            path.join(dest, "Cargo.toml"),
            template
              .replaceAll("@MODE@", name)
              .replaceAll("@MODE_ID@", String(modes.findIndex((mode) => mode.mode === name)))
              .replaceAll("@LIB_MODE@", name.replaceAll("-", "_"))
              .replaceAll("@PUBLIC_CRATE@", publicCrate),
          );
          run("rustup", [
            "run",
            "1.91.1",
            "cargo",
            "generate-lockfile",
            "--offline",
            "--manifest-path",
            path.join(dest, "Cargo.toml"),
          ]);
          clippy(path.join(dest, "Cargo.toml"));
        }
      }
    }
  } finally {
    fs.rmSync(parent, { recursive: true, force: true });
  }
}
process.exitCode = failed ? 1 : 0;

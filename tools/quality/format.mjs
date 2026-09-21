import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import * as prettier from "prettier";
import svelte from "prettier-plugin-svelte";

export const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const excluded = new Set([
  ".git",
  ".venv",
  ".quality-tools",
  ".quality-cache",
  "node_modules",
  "target",
  "build",
  "dist",
  "sources",
  "datasets",
  "papers",
  "models",
]);
const extensions = new Set([
  ".rs",
  ".py",
  ".js",
  ".mjs",
  ".cjs",
  ".ts",
  ".mts",
  ".cts",
  ".svelte",
  ".json",
  ".jsonc",
  ".css",
  ".html",
  ".md",
  ".yaml",
  ".yml",
]);
export const digest = (value) => createHash("sha256").update(value).digest("hex");
export function eligible(file) {
  const absolute = path.resolve(root, file);
  const relative = path.relative(root, absolute);
  if (
    !relative ||
    relative.startsWith("..") ||
    path.isAbsolute(relative) ||
    relative.split(path.sep).some((p) => excluded.has(p)) ||
    !extensions.has(path.extname(relative))
  )
    return false;
  try {
    return fs.lstatSync(absolute).isFile() && fs.realpathSync(absolute) === absolute;
  } catch {
    return false;
  }
}
export function files() {
  const result = spawnSync(
    "git",
    [
      "ls-files",
      "--cached",
      "--others",
      "--exclude-standard",
      "-z",
      "--",
      ".",
      ":!sources",
      ":!datasets",
      ":!papers",
    ],
    { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr);
  return [...new Set(result.stdout.split("\0").filter(Boolean))].filter(eligible);
}
export function protectedFiles() {
  const pinned = new Set();
  const provenance = path.join(root, "provenance/import.json");
  if (fs.existsSync(provenance))
    for (const file of Object.keys(JSON.parse(fs.readFileSync(provenance, "utf8")).files))
      pinned.add(file);
  const experiments = path.join(root, "rust/barcode-core/experiments");
  if (fs.existsSync(experiments))
    for (const file of fs.readdirSync(experiments).filter((f) => f.endsWith(".json"))) {
      const manifest = JSON.parse(fs.readFileSync(path.join(experiments, file), "utf8"));
      for (const source of Object.keys(manifest.baseHashes ?? {}))
        pinned.add(`rust/barcode-core/${source}`);
      pinned.add(`rust/barcode-core/experiments/${file}`);
    }
  // Historical recipes, patches, benchmark payloads and generated assets are not normal source edits.
  for (const file of files())
    if (
      /^(adapters\/retail-reader\/src\/lib\.rs|core\/|provenance\/|js\/camera-demo\/src\/vendor\/|rust\/barcode-core\/experiments\/|benchmark\/(results|reports)\/|.*\/experimental-built\/|.*\/public\/)/.test(
        file,
      )
    )
      pinned.add(file);
  return pinned;
}
export function run(command, args, input) {
  const result = spawnSync(command, args, {
    cwd: root,
    input,
    encoding: "utf8",
    maxBuffer: 32 * 1024 * 1024,
    timeout: 60000,
  });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(result.stderr || result.stdout || `${command} exited ${result.status}`);
  return result.stdout;
}
export async function formatted(file, text) {
  const ext = path.extname(file);
  if (ext === ".rs")
    return run(
      "rustup",
      [
        "run",
        "1.91.1",
        "rustfmt",
        "--edition",
        "2021",
        "--emit",
        "stdout",
        "--config",
        "skip_children=true",
      ],
      text,
    );
  if (ext === ".py")
    return run(
      path.join(root, ".quality-tools/python/bin/ruff"),
      ["format", "--stdin-filename", file, "-"],
      text,
    );
  return prettier.format(text, {
    ...(await prettier.resolveConfig(path.join(root, file))),
    filepath: file,
    plugins: [svelte],
  });
}
export async function formatFiles(paths, { check = false } = {}) {
  const pinned = protectedFiles();
  const changed = [];
  const skipped = [];
  for (const input of [...new Set(paths)]) {
    const file = path.relative(root, path.resolve(root, input));
    if (!eligible(file)) continue;
    if (pinned.has(file)) {
      skipped.push(file);
      continue;
    }
    const absolute = path.join(root, file);
    const before = fs.readFileSync(absolute, "utf8");
    const after = await formatted(file, before);
    if (before === after) continue;
    if (check) {
      changed.push(file);
      continue;
    }
    // Hooks run synchronously after edits. Refuse to replace a newer concurrent write.
    if (!eligible(file) || fs.readFileSync(absolute, "utf8") !== before)
      throw new Error(`Concurrent edit: ${file}; re-read and format again`);
    fs.writeFileSync(absolute, after);
    changed.push(file);
  }
  return { changed, skipped };
}

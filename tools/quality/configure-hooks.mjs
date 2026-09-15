import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const command = 'node "$(git rev-parse --show-toplevel)/tools/quality/hook.mjs"';
const groups = [{ hooks: [{ type: "command", command, timeout: 60 }] }];
for (const [directory, file] of [
  [".claude", "settings.local.json"],
  [".codex", "hooks.json"],
]) {
  const target = path.join(root, directory, file);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const config = fs.existsSync(target) ? JSON.parse(fs.readFileSync(target, "utf8")) : {};
  config.hooks ??= {};
  for (const event of ["PreToolUse", "PostToolUse"]) {
    const retained = (config.hooks[event] ?? []).filter(
      (group) => !group.hooks?.some((h) => h.command === command),
    );
    if (retained.length) config.hooks[event] = retained;
    else delete config.hooks[event];
  }
  for (const event of ["UserPromptSubmit", "Stop"]) {
    const existing = config.hooks[event] ?? [];
    config.hooks[event] = [
      ...existing.filter((group) => !group.hooks?.some((h) => h.command === command)),
      ...groups,
    ];
  }
  fs.writeFileSync(target, JSON.stringify(config, null, 2) + "\n");
}
const previous = spawnSync("git", ["config", "--get", "core.hooksPath"], {
  cwd: root,
  encoding: "utf8",
});
if (previous.status === 1 || previous.stdout.trim() === "tools/quality") {
  const result = spawnSync("git", ["config", "--local", "core.hooksPath", "tools/quality"], {
    cwd: root,
    stdio: "inherit",
  });
  if (result.status !== 0) process.exit(result.status ?? 1);
} else
  console.error(
    `Existing Git hooks preserved (${previous.stdout.trim()}); call tools/quality/pre-commit from that hook.`,
  );

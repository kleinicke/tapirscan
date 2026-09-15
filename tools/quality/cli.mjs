import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root, files, formatFiles } from "./format.mjs";
const [command = "check-format", ...args] = process.argv.slice(2);
let paths = args;
if (args.includes("--all")) paths = files();
if (!paths.length) {
  const result = spawnSync("git", ["diff", "--name-only", "HEAD", "--diff-filter=ACMR", "-z"], {
    cwd: root,
    encoding: "utf8",
  });
  paths = result.stdout.split("\0").filter(Boolean);
}
try {
  if (command === "format" || command === "check-format") {
    const result = await formatFiles(paths, { check: command === "check-format" });
    for (const file of result.changed)
      console.log(`${command === "format" ? "Formatted" : "Needs formatting"}: ${file}`);
    for (const file of result.skipped) console.log(`Protected snapshot: ${file}`);
    if (command === "check-format" && result.changed.length) process.exitCode = 1;
  } else if (command === "lint") {
    const selected = paths.filter(
      (p) => /\.(?:[cm]?[jt]s|svelte)$/.test(p) && fs.existsSync(path.resolve(root, p)),
    );
    if (selected.length)
      process.exitCode =
        spawnSync(
          process.execPath,
          [
            path.join(root, "tools/quality/node_modules/eslint/bin/eslint.js"),
            "--config",
            "tools/quality/eslint.config.mjs",
            ...selected,
          ],
          { cwd: root, stdio: "inherit" },
        ).status ?? 1;
  } else throw new Error(`Unknown command: ${command}`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}

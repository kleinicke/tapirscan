import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root, eligible, formatted, protectedFiles } from "./format.mjs";

function git(args, { input, index } = {}) {
  const result = spawnSync("git", args, {
    cwd: root,
    encoding: "utf8",
    input,
    maxBuffer: 64 * 1024 * 1024,
    env: index ? { ...process.env, GIT_INDEX_FILE: index } : process.env,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr || `git ${args[0]} failed`);
  return result.stdout;
}

// Honor alternate indexes used by partial commits. Hold the standard Git lock
// while preparing changes, then publish the complete index in one rename.
const index = path.resolve(
  root,
  process.env.GIT_INDEX_FILE || git(["rev-parse", "--git-path", "index"]).trim(),
);
const lock = index + ".lock";
let ownsLock = false;
try {
  const descriptor = fs.openSync(lock, "wx", 0o600);
  ownsLock = true;
  try {
    fs.writeFileSync(descriptor, fs.readFileSync(index));
  } finally {
    fs.closeSync(descriptor);
  }
  const pinned = protectedFiles();
  const files = git(["diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"], {
    index: lock,
  })
    .split("\0")
    .filter(Boolean);
  const changes = [];
  for (const file of files) {
    if (!eligible(file) || pinned.has(file)) continue;
    const entry = git(["ls-files", "--stage", "-z", "--", file], { index: lock });
    const match = /^(100644|100755) ([0-9a-f]+) 0\t/.exec(entry);
    if (!match) continue;
    const before = git(["show", `:${file}`], { index: lock });
    const after = await formatted(file, before);
    if (before === after) continue;
    const object = git(["hash-object", "-w", "--stdin"], { input: after }).trim();
    changes.push({ file, mode: match[1], object, before, after });
  }
  if (changes.length) {
    git(["update-index", "-z", "--index-info"], {
      index: lock,
      input: changes.map(({ file, mode, object }) => `${mode} ${object}\t${file}\0`).join(""),
    });
    fs.renameSync(lock, index);
    ownsLock = false;
    for (const { file, before, after } of changes) {
      // Never stage working-tree content. Only align a fully staged file whose
      // on-disk bytes still equal the original index; partial edits stay intact.
      const absolute = path.join(root, file);
      try {
        if (eligible(file) && fs.readFileSync(absolute, "utf8") === before) {
          fs.writeFileSync(absolute, after);
          console.error(`Formatted staged file: ${file}`);
        } else console.error(`Formatted staged file: ${file} (unstaged content preserved)`);
      } catch (error) {
        console.error(
          `Staged formatting saved; working file left for review: ${file}: ${error.message}`,
        );
      }
    }
  }
} catch (error) {
  console.error(`Commit formatting: ${error.message}`);
  process.exitCode = 1;
} finally {
  if (ownsLock) fs.unlinkSync(lock);
}

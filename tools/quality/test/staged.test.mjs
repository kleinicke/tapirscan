import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { root } from "../format.mjs";

function fixture(t) {
  const directory = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "barcode-staged-")));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  fs.mkdirSync(path.join(directory, "tools"));
  fs.cpSync(path.join(root, "tools/quality"), path.join(directory, "tools/quality"), {
    recursive: true,
    filter: (file) =>
      !file.includes("node_modules") && !file.includes(`${path.sep}test${path.sep}`),
  });
  fs.symlinkSync(
    path.join(root, "tools/quality/node_modules"),
    path.join(directory, "tools/quality/node_modules"),
  );
  fs.writeFileSync(path.join(directory, ".gitignore"), "node_modules/\n.quality-cache/\n");
  const cleanEnv = { ...process.env };
  delete cleanEnv.GIT_INDEX_FILE;
  const run = (args, { env = {}, ok = true } = {}) => {
    const result = spawnSync(args[0], args.slice(1), {
      cwd: directory,
      encoding: "utf8",
      env: { ...cleanEnv, ...env },
    });
    if (ok) assert.equal(result.status, 0, result.stderr);
    return result;
  };
  const git = (...args) => run(["git", ...args]).stdout;
  const write = (file, text) => fs.writeFileSync(path.join(directory, file), text);
  const read = (file) => fs.readFileSync(path.join(directory, file), "utf8");
  const bytes = (file) => fs.readFileSync(path.join(directory, file));
  const hook = (options) => run([process.execPath, "tools/quality/staged.mjs"], options);
  git("init", "-q");
  return { directory, run, git, write, read, bytes, hook };
}

test("commit formats fully staged files, retaining executable mode and untracked files", (t) => {
  const f = fixture(t);
  f.write("code.js", "const value={x:1}\n");
  f.write("untracked.js", "const untouched={x:9}\n");
  fs.chmodSync(path.join(f.directory, "code.js"), 0o755);
  f.git("add", "code.js");
  f.git("config", "core.hooksPath", "tools/quality");
  f.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture");
  assert.equal(f.git("show", "HEAD:code.js"), "const value = { x: 1 };\n");
  assert.equal(f.read("code.js"), f.git("show", ":code.js"));
  assert.match(f.git("ls-files", "--stage", "code.js"), /^100755 /);
  assert.equal(f.git("ls-files", "untracked.js"), "");
  assert.equal(f.read("untracked.js"), "const untouched={x:9}\n");
  assert.equal(f.hook().stderr, "");
});

test("partial staging formats only the index and never stages working edits", (t) => {
  const f = fixture(t);
  f.write("code.js", "const value={x:1}\n");
  f.git("add", "code.js");
  f.write("code.js", "const value={x:2}\n// unfinished working edit\n");
  const working = f.read("code.js");
  f.hook();
  assert.equal(f.git("show", ":code.js"), "const value = { x: 1 };\n");
  assert.equal(f.read("code.js"), working);
});

test("a syntax failure leaves the entire index and working files unchanged", (t) => {
  const f = fixture(t);
  f.write("a.js", "const value={x:1}\n");
  f.write("z.js", "const broken = {");
  f.git("add", "a.js", "z.js");
  const before = f.bytes(".git/index");
  assert.notEqual(f.hook({ ok: false }).status, 0);
  assert.deepEqual(f.bytes(".git/index"), before);
  assert.equal(f.read("a.js"), "const value={x:1}\n");
  assert.equal(fs.existsSync(path.join(f.directory, ".git/index.lock")), false);
});

test("alternate index formatting leaves the regular index untouched", (t) => {
  const f = fixture(t);
  f.write("code.js", "const value={x:1}\n");
  f.git("add", "code.js");
  const alternate = path.join(f.directory, ".git/alternate-index");
  fs.copyFileSync(path.join(f.directory, ".git/index"), alternate);
  const original = f.bytes(".git/index");
  f.hook({ env: { GIT_INDEX_FILE: alternate } });
  assert.deepEqual(f.bytes(".git/index"), original);
  assert.equal(
    f.run(["git", "show", ":code.js"], { env: { GIT_INDEX_FILE: alternate } }).stdout,
    "const value = { x: 1 };\n",
  );
});

test("pinned files and an existing Git lock are preserved", (t) => {
  const f = fixture(t);
  fs.mkdirSync(path.join(f.directory, "core"));
  f.write("core/frozen.js", "const pinned={x:1}\n");
  f.git("add", "core/frozen.js");
  fs.mkdirSync(path.join(f.directory, "js/camera-demo/src/vendor"), { recursive: true });
  f.write("js/camera-demo/src/vendor/frozen.ts", "const pinned={x:1}\n");
  f.git("add", "js/camera-demo/src/vendor/frozen.ts");
  f.hook();
  assert.equal(f.git("show", ":js/camera-demo/src/vendor/frozen.ts"), "const pinned={x:1}\n");
  assert.equal(f.git("show", ":core/frozen.js"), "const pinned={x:1}\n");
  f.write(".git/index.lock", "another Git operation");
  assert.notEqual(f.hook({ ok: false }).status, 0);
  assert.equal(f.read(".git/index.lock"), "another Git operation");
});

test("a path-only commit formats its temporary index without including other staged files", (t) => {
  const f = fixture(t);
  f.write("code.js", "const value = 0;\n");
  f.git("add", "code.js");
  f.git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "base");
  f.git("config", "core.hooksPath", "tools/quality");
  f.write("code.js", "const value={x:1}\n");
  f.write("other.js", "const other={x:2}\n");
  f.git("add", "other.js");
  f.git(
    "-c",
    "user.name=Test",
    "-c",
    "user.email=test@example.invalid",
    "commit",
    "-m",
    "selected",
    "--only",
    "code.js",
  );
  assert.equal(f.git("show", "HEAD:code.js"), "const value = { x: 1 };\n");
  assert.equal(f.git("ls-tree", "--name-only", "HEAD").trim(), "code.js");
  // Git retains its separate real-index snapshot for --only commits.
  assert.equal(f.git("show", ":code.js"), "const value={x:1}\n");
  assert.equal(f.git("show", ":other.js"), "const other={x:2}\n");
});

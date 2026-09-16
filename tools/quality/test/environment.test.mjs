import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { qualityEnvironment, pythonExecutable } from "../environment.mjs";

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "tapirscan-quality-env-"));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, ".quality-tools"));
  return root;
}

test("local tool paths are optional and explicit environment values take precedence", (t) => {
  const root = fixture(t);
  assert.deepEqual(qualityEnvironment(root, { PATH: "/bin" }), { PATH: "/bin" });
  fs.writeFileSync(
    path.join(root, ".quality-tools/environment.json"),
    JSON.stringify({ QUALITY_PYTHON: "/local/python", JAVA_HOME: "/local/jdk" }),
  );
  assert.deepEqual(qualityEnvironment(root, { QUALITY_PYTHON: "/ci/python", PATH: "/bin" }), {
    QUALITY_PYTHON: "/ci/python",
    JAVA_HOME: "/local/jdk",
    PATH: "/bin",
  });
});

test("local config rejects invalid values and unrelated environment settings", (t) => {
  const root = fixture(t);
  for (const value of [
    null,
    [],
    { QUALITY_PYTHON: false },
    { JAVA_HOME: "" },
    { PATH: "/other" },
  ]) {
    fs.writeFileSync(path.join(root, ".quality-tools/environment.json"), JSON.stringify(value));
    assert.throws(() => qualityEnvironment(root, {}));
  }
});

test("Python command names resolve to executable paths accepted by ty", (t) => {
  const root = fixture(t);
  const executable = pythonExecutable(root, { ...process.env, QUALITY_PYTHON: "python3" });
  assert.ok(path.isAbsolute(executable));
  assert.ok(fs.existsSync(executable));
  assert.throws(
    () => pythonExecutable(root, { ...process.env, QUALITY_PYTHON: path.join(root, "missing") }),
    /set QUALITY_PYTHON/,
  );
});

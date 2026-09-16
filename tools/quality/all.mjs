// Read-only source checks: retain each stage's output and report every failure.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { root } from "./format.mjs";
import { qualityEnvironment, pythonExecutable } from "./environment.mjs";

const args = process.argv.slice(2);
if (args.some((arg) => arg !== "--with-core")) {
  console.error("Usage: node tools/quality/all.mjs [--with-core]");
  process.exit(2);
}
const env = qualityEnvironment(root);
fs.mkdirSync(path.join(root, ".quality-cache"), { recursive: true });
const directory = fs.mkdtempSync(path.join(root, ".quality-cache", "check-"));
const results = [];
function check(name, command, commandArgs) {
  const log = path.join(directory, `${name}.log`);
  const fd = fs.openSync(log, "w");
  console.log(`Checking ${name}… (${path.relative(root, log)})`);
  let result;
  try {
    result = spawnSync(command, commandArgs, { cwd: root, env, stdio: ["ignore", fd, fd] });
  } finally {
    fs.closeSync(fd);
  }
  const passed = !result.error && result.status === 0;
  results.push({ name, passed, log });
  if (!passed) {
    if (result.error) console.error(result.error.message);
    console.error(fs.readFileSync(log, "utf8"));
  }
  console.log(`${passed ? "PASS" : "FAIL"} ${name}`);
}

check("format", process.execPath, ["tools/quality/cli.mjs", "check-format", "--all"]);
// Resolve command names to paths: ty requires an actual executable path.
try {
  env.QUALITY_PYTHON = pythonExecutable(root, env);
  check("provenance", env.QUALITY_PYTHON, ["scripts/verify_import.py"]);
} catch (error) {
  console.error(error.message);
  results.push({ name: "provenance", passed: false });
}
for (const group of ["js", "python", "native", "rust"])
  check(group, process.execPath, ["tools/quality/release.mjs", group]);
check("demo", "npm", ["run", "check", "--prefix", "demo"]);
check("tooling-tests", process.execPath, ["--test", "tools/quality/test/*.test.mjs"]);
check("imported-core", process.execPath, ["tools/quality/check.mjs", "rust"]);

console.log("\nQuality summary");
for (const { name, passed } of results) console.log(`${passed ? "PASS" : "FAIL"} ${name}`);
console.log(`Full logs: ${path.relative(root, directory)}`);
process.exitCode = results.some(({ passed }) => !passed) ? 1 : 0;

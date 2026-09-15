import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
if (Number(process.versions.node.split(".")[0]) < 24) {
  console.error("Use Node 24 or newer; with nvm, run nvm install && nvm use first.");
  process.exit(1);
}
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
function run(cmd, args) {
  const result = spawnSync(cmd, args, { cwd: root, stdio: "inherit" });
  if (result.status !== 0) process.exit(result.status ?? 1);
}
run("npm", ["ci", "--prefix", "tools/quality"]);
run("python3", ["-m", "venv", ".quality-tools/python"]);
run(path.join(root, ".quality-tools/python/bin/python"), [
  "-m",
  "pip",
  "install",
  "ruff==0.16.7",
  "ty==0.0.80",
  "mypy==2.3.1",
]);
run("rustup", [
  "toolchain",
  "install",
  "1.91.1",
  "--profile",
  "minimal",
  "--component",
  "rustfmt,clippy",
  "--target",
  "wasm32-unknown-unknown",
]);
run(process.execPath, ["tools/quality/configure-hooks.mjs"]);

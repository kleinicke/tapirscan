import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

// Machine-specific paths stay in an ignored file; CI/environment overrides win.
export function qualityEnvironment(root, env = process.env) {
  const file = path.join(root, ".quality-tools/environment.json");
  const local = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : {};
  if (!local || typeof local !== "object" || Array.isArray(local))
    throw new Error(`${file}: expected an object`);
  for (const [key, value] of Object.entries(local)) {
    if (!["QUALITY_PYTHON", "JAVA_HOME"].includes(key) || typeof value !== "string" || !value)
      throw new Error(`${file}: only nonempty QUALITY_PYTHON and JAVA_HOME strings are supported`);
  }
  return { ...local, ...env };
}

export function pythonExecutable(root, env) {
  const command = env.QUALITY_PYTHON ?? "python3";
  const result = spawnSync(command, ["-c", "import sys; print(sys.executable)"], {
    cwd: root,
    env,
    encoding: "utf8",
  });
  if (result.error || result.status !== 0 || !result.stdout.trim())
    throw new Error(`Cannot run ${command}; set QUALITY_PYTHON to a Python executable`);
  return result.stdout.trim();
}

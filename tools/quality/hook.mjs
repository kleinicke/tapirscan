import fs from "node:fs";
import path from "node:path";
import { root, files, digest, formatFiles } from "./format.mjs";
const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const event = JSON.parse(Buffer.concat(chunks).toString() || "{}");
const stateDir = path.join(root, ".quality-cache/hooks");
fs.mkdirSync(stateDir, { recursive: true });
const stateFile = path.join(stateDir, digest(event.session_id ?? "default") + ".json");
function snapshot() {
  return Object.fromEntries(files().map((f) => [f, digest(fs.readFileSync(path.join(root, f)))]));
}
try {
  if (event.hook_event_name === "UserPromptSubmit") {
    fs.writeFileSync(stateFile, JSON.stringify(snapshot()));
  } else if (
    event.hook_event_name === "Stop" &&
    !event.stop_hook_active &&
    fs.existsSync(stateFile)
  ) {
    const before = JSON.parse(fs.readFileSync(stateFile, "utf8"));
    const selected = Object.entries(snapshot())
      .filter(([file, hash]) => before[file] !== hash)
      .map(([file]) => file);
    const { changed } = await formatFiles(selected, { check: true });
    if (changed.length) {
      console.log(
        JSON.stringify({
          decision: "block",
          reason: `Format the files changed in this turn before finishing: ${changed.slice(0, 20).join(", ")}${changed.length > 20 ? ` (and ${changed.length - 20} more)` : ""}. Run tools/quality/cli.mjs format with explicit paths, then the relevant checks. Re-read affected sections only if further edits are needed.`,
        }),
      );
      process.exit(0);
    }
  }
  console.log("{}");
} catch (error) {
  console.error(`Formatting checkpoint: ${error.message}`);
  process.exitCode = 1;
}

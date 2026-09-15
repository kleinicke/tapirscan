// Read-only publication gate. These checks accept the deliberately simple package TOML layout.
import { readFileSync, existsSync } from "node:fs";
const root = new URL("../", import.meta.url);
const read = (path) => readFileSync(new URL(path, root), "utf8");
const npm = JSON.parse(read("bindings/javascript/package.json"));
const toml = read("bindings/python/pyproject.toml");
const section = (name) => toml.split(`[${name}]`)[1]?.split(/^\[/m)[0] ?? "";
const field = (text, name) => text.match(new RegExp(`^${name}\\s*=\\s*"([^"]+)"`, "m"))?.[1];
const project = section("project");
const issues = [
  [
    npm.name === "tapirscan" && field(project, "name") === "tapirscan",
    "Package names must both be tapirscan",
  ],
  [npm.version === field(project, "version"), "npm and Python versions differ"],
  [existsSync(new URL("LICENSE", root)), "Choose the project license and add LICENSE"],
  [
    npm.license && field(project, "license"),
    "Record the selected license in both package manifests",
  ],
  [
    npm.repository && field(section("project.urls"), "Repository"),
    "Set the public repository in both manifests",
  ],
  [npm.author && /^authors\s*=\s*\[/m.test(project), "Record the author/copyright identity"],
  [!npm.private, "Remove npm private:true once publication metadata is approved"],
  ...["bindings/javascript/LICENSE", "bindings/python/LICENSE"].map((path) => [
    existsSync(new URL(path, root)),
    `Include the project license in ${path}`,
  ]),
]
  .filter(([ok]) => !ok)
  .map(([, message]) => message);
for (const issue of issues) console.error(`BLOCKED: ${issue}`);
if (issues.length) process.exitCode = 1;
else console.log(`Publication metadata ready for tapirscan ${npm.version}`);

// Released Tapirscan versions come from package.json aliases of the form
// "tapirscan-1-2-2": "npm:tapirscan@1.2.2". Adding a newly published version
// is one new alias; the demo derives its readers, engine files and hosts.
import { readFileSync } from "node:fs";

const alias = /^tapirscan-(\d+)-(\d+)-(\d+)$/;

export function releasesFrom(dependencies) {
  const releases = [];
  for (const [name, spec] of Object.entries(dependencies)) {
    const match = alias.exec(name);
    if (!match) continue;
    const version = match.slice(1).join(".");
    if (spec !== `npm:tapirscan@${version}`)
      throw Error(`${name} must be "npm:tapirscan@${version}"`);
    releases.push({ version, alias: name });
  }
  const key = ({ version }) => version.split(".").map(Number);
  releases.sort((a, b) => {
    const [x, y] = [key(a), key(b)];
    return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
  });
  return releases;
}

/** Released versions, oldest first; the last one is the main release. */
export function releases(packageUrl = new URL("../package.json", import.meta.url)) {
  const list = releasesFrom(JSON.parse(readFileSync(packageUrl, "utf8")).dependencies);
  if (!list.length) throw Error("package.json needs at least one tapirscan-X-Y-Z alias");
  return list;
}

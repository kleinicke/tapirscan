// Emit observations for the routine Python/JS supplement parity runner.
import { readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { Scanner } from "../dist/index.js";
import { runApiChecks } from "./api-consumer.mjs";
const manifest = process.argv[2];
const fixtures = JSON.parse(await readFile(manifest, "utf8"));
for (const fixture of fixtures)
  fixture.data = [...(await readFile(join(dirname(manifest), fixture.file)))];
console.log(JSON.stringify(await runApiChecks(Scanner, fixtures)));

import { copyFile, mkdir } from "node:fs/promises";

const source = new URL("../src/", import.meta.url);
const output = new URL("../dist/", import.meta.url);
for (const path of [
  "runtime-host.mjs",
  "runtime-host.d.mts",
  "runtime-detail/scanner.mjs",
  "runtime-detail/scanner.d.mts",
]) {
  const destination = new URL(path, output);
  await mkdir(new URL("./", destination), { recursive: true });
  await copyFile(new URL(path, source), destination);
}

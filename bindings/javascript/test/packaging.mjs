// Exercise the real publication verifier on an isolated copy of its package inputs.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, writeFile, mkdir, copyFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const repository = new URL("../../../", import.meta.url);
const readJson = async (path) => JSON.parse(await readFile(new URL(path, repository), "utf8"));

test("documentation snapshots keep package verification strict without invalidating WASM", async () => {
  const selection = await readJson("provenance/modes.json");
  const runtime = await readJson(selection.runtimeRevision);
  const manifest = await readJson(selection.apiWasm);
  const temporary = await mkdtemp(join(tmpdir(), "tapirscan-package-provenance-"));
  try {
    const files = new Set([
      ...Object.keys(runtime.files),
      ...Object.keys(manifest.sourceFiles),
      selection.apiWasm,
      "bindings/javascript/scripts/verify-package.mjs",
      "bindings/javascript/package.json",
      "bindings/javascript/src/index.ts",
      "bindings/javascript/THIRD_PARTY_NOTICES.md",
      ...[
        "index.js",
        "index.d.ts",
        "browser.js",
        "browser.d.ts",
        "browser-worker.js",
        "freeze.js",
        "rust-session.js",
      ].map((name) => `bindings/javascript/dist/${name}`),
      ...[...manifest.modes, ...manifest.experimentalTurbo].map(
        ({ file }) => `bindings/javascript/wasm/${file}`,
      ),
    ]);
    for (const file of files) {
      const target = join(temporary, file);
      await mkdir(dirname(target), { recursive: true });
      await copyFile(new URL(file, repository), target);
    }
    // Simulate a documentation-only promotion without changing the recorded WASMs.
    const documentation = `${await readFile(join(temporary, "README.md"), "utf8")}\nUpdated documentation.\n`;
    await writeFile(join(temporary, "README.md"), documentation);
    runtime.files["README.md"] = createHash("sha256").update(documentation).digest("hex");
    selection.runtimeRevision = "provenance/documentation-update.json";
    await writeFile(join(temporary, selection.runtimeRevision), JSON.stringify(runtime));
    const selectionPath = join(temporary, "provenance/modes.json");
    const save = () => writeFile(selectionPath, JSON.stringify(selection));
    const verify = () =>
      execFileSync(
        process.execPath,
        [join(temporary, "bindings/javascript/scripts/verify-package.mjs")],
        { encoding: "utf8", stdio: "pipe" },
      );
    await save();
    assert.match(verify(), /Package inputs verified/);
    for (const [entries, key] of [
      [selection.modes, "mode"],
      [selection.experimentalTurbo, "preset"],
    ]) {
      const original = entries[0].tag;
      entries[0].tag = "wrong-engine";
      await save();
      assert.throws(verify, new RegExp(`Selected ${key} engines do not match`));
      entries[0].tag = original;
    }
    await save();
    const source = "core/src/lib.rs";
    await writeFile(join(temporary, source), "tampered compiler input");
    // Even an updated runtime snapshot cannot hide drift from the WASM build inputs.
    const originalHash = runtime.files[source];
    runtime.files[source] = createHash("sha256").update("tampered compiler input").digest("hex");
    await writeFile(join(temporary, selection.runtimeRevision), JSON.stringify(runtime));
    assert.throws(verify, /Public Rust WASM source drift: core\/src\/lib.rs/);
    await copyFile(new URL(source, repository), join(temporary, source));
    runtime.files[source] = originalHash;
    await writeFile(join(temporary, selection.runtimeRevision), JSON.stringify(runtime));
    await writeFile(
      join(temporary, "bindings/javascript/wasm", manifest.modes[0].file),
      "tampered binary",
    );
    assert.throws(verify, /Public Rust WASM does not match its manifest/);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

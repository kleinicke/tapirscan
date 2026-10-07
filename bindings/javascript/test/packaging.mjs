// Exercise the real publication verifier on an isolated copy of its package inputs.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cp, mkdtemp, readFile, writeFile, mkdir, copyFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

const repository = new URL("../../../", import.meta.url);
const readJson = async (path) => JSON.parse(await readFile(new URL(path, repository), "utf8"));

test("package verification rejects WASM files built from different source", async () => {
  const manifest = await readJson("bindings/javascript/wasm/build.json");
  const temporary = await mkdtemp(join(tmpdir(), "tapirscan-package-build-"));
  try {
    // The source tree that the manifest digest covers.
    const folders = [
      "core/src",
      "multiformat",
      "bindings/rust",
      "bindings/wasm",
      "tools/package-source",
    ];
    for (const folder of folders)
      await cp(new URL(`${folder}/`, repository), join(temporary, folder), {
        recursive: true,
        filter: (source) => !/(^|[\\/])target([\\/]|$)/.test(source),
      });
    const files = new Set([
      ...Object.keys(manifest.sourceFiles).filter(
        (file) => !folders.some((folder) => file.startsWith(`${folder}/`)),
      ),
      "bindings/javascript/scripts/verify-package.mjs",
      "bindings/javascript/package.json",
      "bindings/javascript/src/index.ts",
      "bindings/javascript/wasm/build.json",
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
      ...Object.keys(manifest.files).map((file) => `bindings/javascript/wasm/${file}`),
    ]);
    for (const file of files) {
      const target = join(temporary, file);
      await mkdir(dirname(target), { recursive: true });
      await copyFile(new URL(file, repository), target);
    }
    const verify = () =>
      execFileSync(
        process.execPath,
        [join(temporary, "bindings/javascript/scripts/verify-package.mjs")],
        { encoding: "utf8", stdio: "pipe" },
      );
    // The copy matches the build, which also proves the verifier's file walk
    // agrees with scripts/build_wasm.py.
    assert.match(verify(), /Package inputs verified/);

    const source = join(temporary, "core/src/lib.rs");
    const original = await readFile(source);
    await writeFile(source, Buffer.concat([original, Buffer.from("\n// changed\n")]));
    assert.throws(verify, /WASM files are stale/);
    await writeFile(source, original);

    // A new source file also changes the digest.
    const added = join(temporary, "core/src/added.rs");
    await writeFile(added, "// new module\n");
    assert.throws(verify, /WASM files are stale/);
    await rm(added);
    assert.match(verify(), /Package inputs verified/);

    await writeFile(join(temporary, "bindings/javascript/wasm/medium.wasm"), "tampered binary");
    assert.throws(verify, /WASM does not match build.json/);
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
});

// Usage: node test/installed.mjs package.tgz fixtures/manifest.json
// Uses the installed Google Chrome through the playwright-core dev dependency.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, realpath, readFile, writeFile, mkdir, cp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve, extname, sep } from "node:path";
import http from "node:http";
import { fileURLToPath } from "node:url";

const [tarball, fixturePath] = process.argv.slice(2).map((p) => resolve(p));
assert.ok(tarball && fixturePath, "Supply npm tarball and fixture manifest");
const fixtures = JSON.parse(await readFile(fixturePath, "utf8"));
for (const fixture of fixtures)
  fixture.data = [...(await readFile(join(dirname(fixturePath), fixture.file)))];
const root = await realpath(await mkdtemp(join(tmpdir(), "tapirscan-npm-")));
let server, browser;
try {
  await writeFile(join(root, "package.json"), JSON.stringify({ private: true, type: "module" }));
  execFileSync(
    "npm",
    [
      "install",
      "--offline",
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      "--cache",
      join(root, "cache"),
      tarball,
    ],
    { cwd: root, stdio: "pipe" },
  );
  await cp(new URL("api-consumer.mjs", import.meta.url), join(root, "consumer.mjs"));
  await writeFile(join(root, "fixtures.json"), JSON.stringify(fixtures));
  await writeFile(
    join(root, "node.mjs"),
    `
    import { readFile } from 'node:fs/promises';
    import { Scanner } from 'tapirscan';
    import { runApiChecks } from './consumer.mjs';
    console.log(JSON.stringify({package: import.meta.resolve('tapirscan'), observations:
      await runApiChecks(Scanner, JSON.parse(await readFile(new URL('./fixtures.json', import.meta.url))))}));
  `,
  );
  const node = JSON.parse(
    execFileSync(process.execPath, [join(root, "node.mjs")], { cwd: root, encoding: "utf8" }),
  );
  assert.ok(fileURLToPath(node.package).startsWith(join(root, "node_modules") + sep));
  await mkdir(join(root, "app/engines"), { recursive: true });
  await cp(join(root, "node_modules/tapirscan/wasm"), join(root, "app/engines"), {
    recursive: true,
  });
  const loaded = [];
  server = http.createServer(async (req, res) => {
    try {
      const pathname = new URL(req.url, "http://localhost").pathname;
      const file = resolve(root, "." + pathname);
      if (!file.startsWith(root + sep)) {
        res.writeHead(403).end();
        return;
      }
      if (pathname === "/app/") {
        res.setHeader("Content-Type", "text/html");
        res.end("<!doctype html><title>Installed scanner test</title>");
        return;
      }
      if (pathname.endsWith(".wasm")) loaded.push(pathname);
      const body = await readFile(file);
      res.setHeader(
        "Content-Type",
        extname(file) === ".wasm" ? "application/wasm" : "text/javascript",
      );
      res.end(body);
    } catch {
      res.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const { chromium } = await import("playwright-core");
  browser = await chromium.launch({ channel: "chrome", headless: true });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/app/`);
  const browserRuns = [];
  for (const relocated of [false, true]) {
    loaded.length = 0;
    const observations = await page.evaluate(
      async ({ fixtures, relocated }) => {
        const { Scanner } = await import("/node_modules/tapirscan/dist/index.js");
        const { runApiChecks } = await import("/consumer.mjs");
        return runApiChecks(Scanner, fixtures, relocated ? { wasmBaseUrl: "./engines/" } : {});
      },
      { fixtures, relocated },
    );
    assert.ok(loaded.length > 0);
    const prefix = relocated ? "/app/engines/" : "/node_modules/tapirscan/wasm/";
    assert.ok(
      loaded.every((p) => p.startsWith(prefix)),
      "WASM URLs must resolve inside the installation",
    );
    assert.deepEqual(observations, node.observations);
    browserRuns.push({ relocated, scans: observations.length * 2, wasmRequests: loaded.length });
  }
  const browserChecks = await page.evaluate(
    async (fixture) => {
      const { Scanner, scan } = await import("/node_modules/tapirscan/dist/browser.js");
      const image = () => ({
        data: new Uint8Array(fixture.data),
        width: fixture.width,
        height: fixture.height,
        channels: 1,
      });
      const expected = JSON.stringify(fixture.expected.map((b) => b.text));
      const texts = (barcodes) => JSON.stringify(barcodes.barcodes.map((b) => b.text));
      const rejects = async (promise, text) => {
        try {
          await promise;
          return false;
        } catch (error) {
          return error.message.includes(text);
        }
      };
      const scanner = new Scanner({ mode: "low", formats: fixture.formats });
      const checks = [];
      try {
        const pixels = image();
        // Concurrent calls queue; the caller's pixels are copied, not transferred.
        const reads = await Promise.all([scanner.inspect(pixels), scanner.scan(image())]);
        checks.push(pixels.data.byteLength > 0);
        checks.push(JSON.stringify(reads[0].values) === expected && texts(reads[1]) === expected);
        checks.push(await rejects(scanner.scan({ ...image(), width: 1 }), "Invalid image"));
        checks.push(texts(await scanner.scan(image())) === expected);
        checks.push(
          texts(await scan(image(), { mode: "low", formats: fixture.formats })) === expected,
        );
        const interrupted = scanner.scan(image());
        scanner.dispose();
        checks.push(await rejects(interrupted, "disposed"));
        checks.push(await rejects(scanner.scan(image()), "disposed"));
        checks.push(await rejects(new Scanner({ mode: "invalid" }).ready, "mode"));
        return checks;
      } finally {
        scanner.dispose();
      }
    },
    fixtures.find((f) => f.expected.length > 0),
  );
  assert.ok(
    browserChecks.every(Boolean),
    `tapirscan/browser from the installed package: ${JSON.stringify(browserChecks)}`,
  );
  console.log(JSON.stringify({ node, browser: browserRuns, browserChecks: browserChecks.length }));
} finally {
  await browser?.close();
  if (server) await new Promise((resolve) => server.close(resolve));
  await rm(root, { recursive: true, force: true });
}

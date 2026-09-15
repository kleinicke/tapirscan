// Compare the packaged release with the pinned research runtime in an actual browser.
// Run with QUALITY_PYTHON pointing to a Pillow-enabled Python and optional TAPIRSCAN_RESEARCH_ROOT.
import { createRequire } from "node:module";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { tmpdir } from "node:os";
import http from "node:http";
import fs from "node:fs/promises";
import assert from "node:assert/strict";
const releaseRoot = fileURLToPath(new URL("../../../", import.meta.url));
const researchRoot =
  process.env.TAPIRSCAN_RESEARCH_ROOT ??
  fileURLToPath(new URL("../../../../barcode/", import.meta.url));
const require = createRequire(import.meta.url);
const { chromium } = require(join(researchRoot, "js/camera-demo/node_modules/@playwright/test"));
const fixtureDir = await fs.mkdtemp(join(tmpdir(), "tapirscan-browser-detail-"));
execFileSync(
  process.env.QUALITY_PYTHON ?? "python3",
  [join(releaseRoot, "scripts/test_detail.py"), "--fixtures", fixtureDir],
  { cwd: releaseRoot },
);
(async () => {
  const server = http
    .createServer(async (req, res) => {
      try {
        const url = new URL(req.url, "http://localhost");
        const file = url.pathname.startsWith("/fixtures/")
          ? fixtureDir + "/" + url.pathname.slice(10)
          : url.pathname.startsWith("/tapirscan/")
            ? join(releaseRoot, url.pathname.slice("/tapirscan/".length))
            : join(researchRoot, url.pathname.slice("/barcode/".length));
        const bytes = await fs.readFile(file);
        res.setHeader(
          "Content-Type",
          file.endsWith(".wasm")
            ? "application/wasm"
            : file.endsWith(".png")
              ? "image/png"
              : file.endsWith(".json")
                ? "application/json"
                : "text/javascript",
        );
        res.end(bytes);
      } catch (e) {
        res.statusCode = 404;
        res.end(String(e));
      }
    })
    .listen(0, "127.0.0.1");
  await new Promise((resolve) => server.once("listening", resolve));
  const port = server.address().port;
  const browser = await chromium.launch({ channel: "chrome", headless: true });
  const page = await browser.newPage();
  try {
    await page.goto(`http://127.0.0.1:${port}/fixtures/manifest.json`);
    const result = await page.evaluate(async () => {
      const { Scanner } = await import("/tapirscan/bindings/javascript/dist/index.js");
      const { DetailScanner } =
        await import("/barcode/js/camera-demo/src/lib/detail-20260914/scanner.mjs");
      const { policy } = await import("/tapirscan/bindings/javascript/dist/policy.js");
      const configs = await (await fetch("/tapirscan/provenance/detail-20260914.json")).json();
      const fixtures = await (await fetch("/fixtures/manifest.json")).json();
      const outputs = [];
      let recovered = 0;
      const load = async (file) =>
        await (await fetch("/tapirscan/bindings/javascript/wasm/" + file)).arrayBuffer();
      for (const config of configs.modes) {
        const release = await Scanner.create({
          mode: config.mode,
          loadWasm: async (url) => load(url.pathname.split("/").pop()),
        });
        const upstream = await DetailScanner.create(
          await load(config.tag + ".wasm"),
          await load("nano-lint-20260913.wasm"),
          config.directions,
        );
        for (const f of fixtures) {
          const image = {
            data: new Uint8Array(
              await (await fetch("/fixtures/" + f.name + ".rgba")).arrayBuffer(),
            ),
            width: f.width,
            height: f.height,
            channels: 4,
            stride: f.width * 4,
          };
          const a = release.scan(image, { debug: true }).debug;
          const b = upstream.scanLocalized(image, policy, config.fitLimit, true);
          const norm = (reads) =>
            reads.map(({ text, polygon, support }) => ({ text, polygon, support }));
          if (JSON.stringify(norm(a.scan.barcodes)) !== JSON.stringify(norm(b.recovery.barcodes)))
            throw Error(
              "Barcode mismatch " +
                config.mode +
                " " +
                f.name +
                " " +
                JSON.stringify({
                  release: norm(a.scan.barcodes),
                  research: norm(b.recovery.barcodes),
                }),
            );
          if (JSON.stringify(a.detailRegions) !== JSON.stringify(b.detailRegions))
            throw Error("Region mismatch " + config.mode + " " + f.name);
          if (JSON.stringify(a.scan.candidates) !== JSON.stringify(b.scan.candidates))
            throw Error("Candidate mismatch " + config.mode + " " + f.name);
          recovered += a.recovery.additions.length;
          outputs.push({
            mode: config.mode,
            name: f.name,
            reads: norm(a.scan.barcodes),
            recovered: a.recovery.additions.length,
            attempts: a.recovery.attempts.length,
          });
        }
        release.dispose();
        upstream.dispose();
      }
      return { outputs, recovered };
    });
    assert.ok(result.recovered > 0, "Fixtures must exercise actual recovered reads");
    console.log(
      "Exact research/release browser parity:",
      result.outputs.length,
      "frames;",
      result.recovered,
      "recovered reads",
    );
  } finally {
    await browser.close();
    server.close();
    await fs.rm(fixtureDir, { recursive: true, force: true });
  }
})().catch((e) => {
  console.error(e);
  process.exitCode = 1;
});

import { writeFile, readFile } from "node:fs/promises";
import { createServer } from "vite";
import { examples, writeBenchmarkIdentity } from "./benchmark-identity.mjs";
// Keep browser automation optional for ordinary demo builds.
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const root = new URL("../", import.meta.url);
const manifest = await writeBenchmarkIdentity();
const pkg = JSON.parse(await readFile(new URL("package.json", root), "utf8"));
const server = await createServer({
  root: root.pathname,
  optimizeDeps: { force: true },
  server: { host: "127.0.0.1", port: 0 },
});
await server.listen();
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage();
  await page.goto(new URL("benchmarks/manifest.json", server.resolvedUrls.local[0]).href);
  for (const file of examples) {
    const report = await page.evaluate(
      async ({ file, identity, version }) => {
        const {
          BenchmarkRunner,
          snapshot,
          stressNote,
          variations,
          renderVariation,
          benchmarkVersion,
        } = await import("/src/lib/benchmark.ts");
        const formats = ["EAN13", "UPCA", "EAN8", "UPCE"];
        const runner = new BenchmarkRunner(formats);
        const bitmap = await createImageBitmap(await (await fetch(`/images/${file}`)).blob());
        const image = snapshot(bitmap, 1920);
        bitmap.close();
        const scenes = [];
        try {
          const baseline = await runner.scene(image, file, ["original"], () => {});
          scenes.push(baseline);
          for (const variation of variations)
            scenes.push(
              await runner.scene(
                renderVariation(image, variation, baseline),
                variation.name,
                variation.tags,
                () => {},
              ),
            );
        } finally {
          runner.cancel();
        }
        return {
          version: benchmarkVersion,
          identity,
          created: new Date().toISOString(),
          browser: navigator.userAgent,
          source: file,
          kind: "stress",
          status: "complete",
          totalInputs: 1,
          formats,
          limit: 1920,
          scenes,
          note: stressNote,
          engines: {
            Tapirscan: version,
            ZXing: "3.1.1",
            ZBar: "0.11.0",
            "Native browser": "BarcodeDetector (see browser version)",
          },
        };
      },
      { file, identity: manifest.identity, version: pkg.version },
    );
    await writeFile(new URL(`public/benchmarks/${file}.json`, root), JSON.stringify(report) + "\n");
    console.log(
      `${file}: ${report.scenes.length} scenes; ${Object.keys(report.scenes[0].observations).join(", ")}`,
    );
  }
} finally {
  await browser.close();
  await server.close();
}

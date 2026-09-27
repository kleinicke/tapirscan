import assert from "node:assert/strict";
import { readFile, mkdir } from "node:fs/promises";
import { preview } from "vite";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const root = new URL("../", import.meta.url);
const server = await preview({ root: root.pathname, preview: { host: "127.0.0.1", port: 0 } });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const errors = [];
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(server.resolvedUrls.local[0]);
  await page.getByRole("button", { name: "Image benchmark +" }).click();
  const panel = page.getByRole("region", { name: "Image benchmark" });
  await panel.getByRole("heading", { name: "Precomputed example · pesto.jpg" }).waitFor();
  assert.equal(await panel.locator("tbody tr").count(), 5);
  // Lazy image decoding must not collapse the gallery or move pagination after a click.
  await panel.getByRole("button", { name: "Next", exact: true }).last().click();
  await page.waitForTimeout(150);
  const nav = panel.locator(".pagination");
  const navTop = await nav.evaluate((element) => element.getBoundingClientRect().top);
  assert.ok(Math.abs(navTop - 16) < 3, `pagination anchored at ${navTop}`);
  await page.waitForTimeout(250);
  assert.ok(
    Math.abs((await nav.evaluate((element) => element.getBoundingClientRect().top)) - navTop) < 2,
  );
  await panel.getByRole("button", { name: "Previous", exact: true }).first().click();
  await panel.getByRole("button", { name: "Inspect full resolution" }).first().click();
  const viewer = page.getByRole("dialog", { name: "Full-resolution benchmark image" });
  await viewer.locator("canvas").waitFor();
  assert.ok(
    await viewer.locator("canvas").evaluate(
      (canvas) =>
        canvas.width >= 1920 &&
        canvas
          .getContext("2d")
          .getImageData(0, 0, canvas.width, canvas.height)
          .data.some((value, index) => index % 4 !== 3 && value < 100),
    ),
  );
  await viewer.getByLabel("Image zoom").selectOption("1");
  assert.equal(
    await viewer
      .locator("canvas")
      .evaluate((canvas) => Math.round(canvas.getBoundingClientRect().width)),
    1920,
  );
  await viewer.getByRole("button", { name: "Close image" }).click();

  await panel.getByRole("columnheader", { name: "Images: exactly 1 unique code" }).waitFor();
  await panel.getByLabel("Benchmark filter").selectOption("multiple");
  await panel.getByLabel("Benchmark filter").selectOption("one");
  await panel.getByLabel("Benchmark filter").selectOption("all");
  const photoZoom = await page.evaluate(async () => {
    const report = await (await fetch("./benchmarks/pesto.jpg.json")).json();
    const first = report.scenes[0].observations.medium.regions[0];
    const width = (region) =>
      Math.max(...region.polygon.map((p) => p[0])) - Math.min(...region.polygon.map((p) => p[0]));
    return report.scenes
      .filter((scene) => scene.tags.includes("zoomed") && !scene.tags.includes("rotated"))
      .some((scene) =>
        scene.observations.medium.regions.some(
          (region) => region.text === first.text && width(region) > width(first) * 1.5,
        ),
      );
  });
  assert.ok(photoZoom, "photo zooms visibly enlarge the detected barcode");
  assert.ok((await panel.locator("polygon").count()) > 0);
  await panel.getByLabel("Benchmark filter").selectOption("repeat");
  assert.match(await panel.innerText(), /Repeated images \/ extra reads/);
  await panel.getByLabel("Benchmark filter").selectOption("all");
  await page.route("**/benchmarks/manifest.json", (route) =>
    route.fulfill({ json: { identity: "stale" } }),
  );
  await panel.getByRole("button", { name: "Show precomputed example" }).click();
  await panel
    .getByText("Saved benchmark does not match this benchmark recipe. Run it on this device.")
    .waitFor();
  await page.unroute("**/benchmarks/manifest.json");
  await panel.getByRole("button", { name: "Show precomputed example" }).click();
  await panel.getByRole("heading", { name: "Precomputed example · pesto.jpg" }).waitFor();
  const buffer = await readFile(new URL("public/images/synthetic-barcode.png", root));
  const file = (name) => ({ name, mimeType: "image/png", buffer });
  await panel
    .getByLabel("Upload benchmark images")
    .setInputFiles(Array.from({ length: 100 }, (_, i) => file(`barcode-${i}.png`)));
  await panel.getByRole("button", { name: "Run uploaded images" }).click();
  await panel.getByText("Completed 100 images.", { exact: true }).waitFor({ timeout: 180000 });
  const downloadPromise = page.waitForEvent("download");
  await panel.getByRole("button", { name: "Download results JSON" }).click();
  const download = await downloadPromise;
  const report = JSON.parse(await readFile(await download.path(), "utf8"));
  assert.equal(report.scenes.length, 100);
  assert.equal(report.kind, "batch");
  assert.equal(report.status, "complete");
  assert.equal(report.totalInputs, 100);
  await panel.getByRole("button", { name: "Inspect full resolution" }).first().click();
  await viewer.locator("canvas").waitFor();
  assert.equal(
    await viewer.locator("canvas").evaluate((canvas) => canvas.width),
    report.scenes[0].width,
  );
  await viewer.getByRole("button", { name: "Close image" }).click();

  for (const scene of report.scenes) {
    for (const method of ["native", "zxing", "zbar", "low", "medium"]) {
      assert.ok(
        scene.observations[method].regions.some((region) => region.text === "4006381333931"),
        `${scene.name} ${method}`,
      );
      assert.ok(scene.observations[method].scanMs >= 0);
    }
  }
  // One failed file must not prevent subsequent files from completing.
  await panel
    .getByLabel("Upload benchmark images")
    .setInputFiles([
      { name: "broken.png", mimeType: "image/png", buffer: Buffer.from("invalid") },
      file("valid.png"),
    ]);
  await panel.getByRole("button", { name: "Run uploaded images" }).click();
  await panel.getByText("Completed 2 images.", { exact: true }).waitFor({ timeout: 60000 });
  assert.match(await panel.innerText(), /load error/);
  await panel.getByLabel("Upload benchmark images").setInputFiles(file("single.png"));
  await panel.getByLabel("Benchmark experiment").selectOption("stress");
  await panel.getByRole("button", { name: "Run uploaded images" }).click();
  await panel.getByText(/Completed \d+ images\./).waitFor({ timeout: 120000 });
  assert.match(await panel.innerText(), /Zoomed variants transform the same photo/);
  const stressDownloadPromise = page.waitForEvent("download");
  await panel.getByRole("button", { name: "Download results JSON" }).click();
  const stressDownload = await stressDownloadPromise;
  const stress = JSON.parse(await readFile(await stressDownload.path(), "utf8"));
  assert.equal(stress.version, 5);
  await panel.getByRole("button", { name: "Inspect full resolution" }).nth(1).click();
  await viewer.locator("canvas").waitFor();
  assert.equal(
    await viewer.locator("canvas").evaluate((canvas) => canvas.width),
    stress.scenes[1].width,
  );
  await viewer.getByLabel("Inspection method").selectOption("low");
  await viewer.getByRole("button", { name: "Close image" }).click();

  const original = stress.scenes[0];
  const zooms = stress.scenes.filter((scene) => scene.tags.includes("zoomed"));
  assert.equal(zooms.length, 9);
  assert.ok(
    zooms.every((scene) => scene.width === original.width && scene.height === original.height),
  );
  assert.ok(zooms.some((scene) => scene.observations.medium.regions.length > 0));
  const full = stress.scenes.find((scene) => scene.name === "Complete image on canvas");
  assert.equal(full.width, Math.ceil(Math.hypot(original.width, original.height)) + 32);
  assert.equal(full.height, full.width);
  assert.ok(
    stress.scenes.every(
      (scene) => !scene.tags.includes("cropped") && !scene.tags.includes("isolated"),
    ),
  );
  // Compare the untransformed full-image thumbnail against every source pixel
  // after the documented padding and thumbnail downsampling. Detects ROI regressions.
  const pixelError = await page.evaluate(
    async ({ thumbnail, side }) => {
      const source = await createImageBitmap(
        await (await fetch("./images/synthetic-barcode.png")).blob(),
      );
      const actual = await createImageBitmap(await (await fetch(thumbnail)).blob());
      const canvas = document.createElement("canvas");
      canvas.width = side;
      canvas.height = side;
      const ctx = canvas.getContext("2d");
      ctx.fillStyle = "white";
      ctx.fillRect(0, 0, side, side);
      ctx.drawImage(source, (side - source.width) / 2, (side - source.height) / 2);
      const thumb = document.createElement("canvas");
      thumb.width = actual.width;
      thumb.height = actual.height;
      const tc = thumb.getContext("2d");
      tc.drawImage(canvas, 0, 0, actual.width, actual.height);
      const expected = tc.getImageData(0, 0, actual.width, actual.height).data;
      tc.drawImage(actual, 0, 0);
      const observed = tc.getImageData(0, 0, actual.width, actual.height).data;
      let total = 0;
      for (let i = 0; i < expected.length; i++) total += Math.abs(expected[i] - observed[i]);
      source.close();
      actual.close();
      return total / expected.length;
    },
    { thumbnail: full.thumbnail, side: full.width },
  );
  assert.ok(pixelError < 3, `complete-image thumbnail differs by ${pixelError}`);
  await panel.getByLabel("Benchmark tag").selectOption("edge");
  assert.match(await panel.innerText(), /Edge left/);
  const artifact = process.env.BENCHMARK_SCREENSHOT;
  if (artifact) {
    await mkdir(new URL(".", `file://${artifact}`), { recursive: true });
    await panel.screenshot({ path: artifact });
  }
  // Stop retains partial data and late worker messages cannot replace it.
  await panel
    .getByLabel("Upload benchmark images")
    .setInputFiles(Array.from({ length: 100 }, (_, i) => file(`cancel-${i}.png`)));
  await panel.getByRole("button", { name: "Run uploaded images" }).click();
  await panel.getByRole("button", { name: "Stop benchmark" }).click();
  await panel.getByText("Stopped. Partial results retained.").waitFor();
  await page.waitForTimeout(500);
  assert.match(await panel.innerText(), /Stopped. Partial results retained./);
  await panel.getByRole("button", { name: "Run uploaded images" }).click();
  await page.getByRole("button", { name: "Pills", exact: true }).click();
  await panel.getByRole("heading", { name: "Precomputed example · pills.jpg" }).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  assert.ok(
    await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1),
    "mobile page has no horizontal overflow",
  );
  if (artifact) await panel.screenshot({ path: artifact.replace(".png", "-mobile.png") });
  assert.deepEqual(errors, []);
  console.log(
    "PASS: saved results, overlays, 100 real five-engine scans, corrupt file isolation, uploaded stress test, filters, cancellation, source switching and mobile layout.",
  );
} finally {
  await browser.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}

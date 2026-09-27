import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
const registry = JSON.parse(
  await readFile(new URL("../src/lib/scanner-versions.json", import.meta.url), "utf8"),
);
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 950 } });
  await page.route("https://analytics.re4vive.com/**", (route) => route.abort());
  await page.addInitScript(() => {
    window.scanCalls = [];
    const original = Worker.prototype.postMessage;
    Worker.prototype.postMessage = function (message, ...args) {
      window.scanCalls.push({ engine: message.engine, releaseVersion: message.releaseVersion });
      return original.call(this, message, ...args);
    };
  });
  const errors = [];
  const requests = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => {
    if (request.url().endsWith(".wasm")) requests.push(request.url());
  });
  await page.goto(process.env.DEMO_TEST_URL || "http://127.0.0.1:5188/");
  await page.getByText("More options", { exact: true }).click();
  const selector = page.getByLabel("Tapirscan version");
  assert.equal(await selector.inputValue(), registry.default);
  await page.waitForFunction(() =>
    document.querySelector('[data-scanner="fast"]')?.textContent.includes("4104420031326"),
  );
  await page.waitForFunction(
    () => document.querySelector('.scan-results [role="status"]')?.textContent === "Scan complete",
  );
  assert.equal(await page.getByRole("button", { name: "Image benchmark +" }).count(), 0);
  const referenceCalls = await page.evaluate(
    () => window.scanCalls.filter((call) => call.engine !== "classical").length,
  );
  const referenceCards = await page
    .locator('[data-scanner="zxing"], [data-scanner="zbar"]')
    .allTextContents();
  for (const { version, modes } of registry.versions.toReversed()) {
    const asset = modes.find((entry) => entry.mode === "medium").file;
    const loaded = page.waitForResponse(
      (response) => response.url().endsWith(asset) && response.ok(),
    );
    await selector.selectOption(version);
    await loaded;
    await page.waitForFunction(() =>
      document.querySelector('[data-scanner="fast"]')?.textContent.includes("4104420031326"),
    );
    assert.equal(await page.locator(".scan-results .error").count(), 0);
    assert.equal(
      await page.evaluate(
        () => window.scanCalls.filter((call) => call.engine !== "classical").length,
      ),
      referenceCalls,
      "Changing Tapirscan version must not rerun reference readers",
    );
    assert.deepEqual(
      await page.locator('[data-scanner="zxing"], [data-scanner="zbar"]').allTextContents(),
      referenceCards,
    );
  }
  await page.getByRole("button", { name: "Show analyzed areas", exact: true }).click();
  const counts = page.getByRole("region", { name: "Analyzed area counts" });
  assert.match(await counts.innerText(), /\d+ proposed areas/);
  assert.match(await counts.innerText(), /Fast-discarded area counts are not exposed/);
  await counts.scrollIntoViewIfNeeded();
  await page.screenshot({ path: "../build/demo-version-switch/desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: "../build/demo-version-switch/mobile.png" });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
  assert.deepEqual(errors, []);
  console.log(
    "PASS: desktop/mobile version selector, real scans, switching back, version-specific WASM requests; no page errors.",
  );
  console.log(requests.filter((url) => /medium-/.test(url)));
} finally {
  await browser.close();
}

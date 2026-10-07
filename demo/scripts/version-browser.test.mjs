import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { releases } from "./releases.mjs";

const published = releases().map(({ version }) => version);
const latest = published.at(-1);
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
    if (request.url().endsWith(".wasm")) requests.push(new URL(request.url()).pathname);
  });
  await page.goto(process.env.DEMO_TEST_URL || "http://127.0.0.1:5188/");
  const found = (id) => (arg) =>
    page.waitForFunction(
      ([scanner, text]) =>
        document.querySelector(`[data-scanner="${scanner}"]`)?.textContent.includes(text),
      [id, arg],
    );
  await found("fast")("4104420031326");
  assert.ok(requests.some((url) => url.endsWith(`/engines/${latest}/medium.wasm`)));
  assert.ok(
    !requests.some((url) => url.includes("/engines/next/")),
    "The current build loads only when selected",
  );

  // The repository's current build is one checkbox away.
  await page.getByText("More scanners", { exact: false }).first().click();
  const loaded = page.waitForResponse(
    (response) => response.url().endsWith("/engines/next/medium.wasm") && response.ok(),
  );
  await page.getByLabel("TS-Med-next").check();
  await loaded;
  await found("ts-med-next")("4104420031326");
  assert.equal(await page.locator(".scan-results .error").count(), 0);
  assert.deepEqual(
    await page.evaluate(() => [
      ...new Set(
        window.scanCalls
          .filter((call) => call.engine === "classical")
          .map((call) => `${call.engine}:${call.releaseVersion}`),
      ),
    ]),
    [`classical:${latest}`, "classical:next"],
  );

  // Earlier published versions are tucked away and run from their own packages.
  const previous = published.slice(0, -1);
  assert.equal(await page.locator(".previous-releases").count(), previous.length ? 1 : 0);
  for (const version of previous) {
    await page.locator(".previous-releases summary").click();
    const response = page.waitForResponse(
      (reply) => reply.url().endsWith(`/engines/${version}/medium.wasm`) && reply.ok(),
    );
    await page.getByLabel(`TS-Med ${version}`).check();
    await response;
    assert.equal(await page.locator(".scan-results .error").count(), 0);
  }

  await mkdir("../build/demo-version-switch", { recursive: true });
  await page.screenshot({ path: "../build/demo-version-switch/desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: "../build/demo-version-switch/mobile.png" });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
  assert.deepEqual(errors, []);
  console.log(
    `PASS: main release ${latest}, on-demand next build, ${previous.length} earlier release(s); no page errors.`,
  );
} finally {
  await browser.close();
}

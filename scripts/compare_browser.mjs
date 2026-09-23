// Browser adapter for the shared comparison loop. Pixel transport is outside scan timing.
import { createServer } from "node:http";
import { readFileSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export async function browserComparison(config) {
  if (!config.playwrightModule) throw new Error("Browser comparisons require --playwright-module");
  const { chromium } = await import(pathToFileURL(config.playwrightModule));
  const files = new Map();
  const modes = {};
  for (const label of ["baseline", "candidate"]) {
    const manifest = JSON.parse(readFileSync(config[`${label}Wasm`], "utf8"));
    modes[label] = Object.fromEntries(manifest.modes.map((entry) => [entry.mode, entry.file]));
    for (const entry of manifest.modes)
      files.set(`/${label}/wasm/${entry.file}`, path.join(config[`${label}Assets`], entry.file));
  }
  const server = createServer((request, response) => {
    const url = new URL(request.url, "http://localhost");
    let file = files.get(url.pathname);
    const match = /^\/(baseline|candidate)\/dist\/([\w/-]+\.js)$/.exec(url.pathname);
    if (match) file = path.join(config[match[1]], "bindings/javascript/dist", match[2]);
    if (url.pathname === "/") {
      response.setHeader("Content-Type", "text/html");
      response.end("<!doctype html><title>Scanner comparison</title>");
      return;
    }
    try {
      if (!file) throw new Error("Unknown asset");
      response.setHeader(
        "Content-Type",
        file.endsWith(".wasm") ? "application/wasm" : "text/javascript",
      );
      response.end(readFileSync(file));
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  let browser;
  try {
    browser = await chromium.launch({
      headless: true,
      channel: config.browserChannel === "chrome" ? "chrome" : undefined,
    });
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.evaluate(() => {
      globalThis.comparisonScanners = new Map();
    });
    let currentImage;
    return {
      runtime: {
        browser: browser.version(),
        userAgent: await page.evaluate(() => navigator.userAgent),
      },
      async pair(mode, addon) {
        const result = [];
        for (const label of ["baseline", "candidate"]) {
          const key = `${label}/${mode}/${addon}`;
          await page.evaluate(
            async ({ key, label, mode, addon, file }) => {
              const host = await import(`/${label}/dist/index.js`);
              const scanner = await host.Scanner.create({
                mode,
                formats: [...host.linearFormats, ...host.matrixFormats],
                eanAddOnPolicy: addon,
                loadWasm: async () =>
                  new Uint8Array(await (await fetch(`/${label}/wasm/${file}`)).arrayBuffer()),
              });
              globalThis.comparisonScanners.set(key, scanner);
            },
            { key, label, mode, addon, file: modes[label][mode] },
          );
          result.push({
            async scanTimed(image, options) {
              // Retain only the current immutable fixture, shared by paired scans.
              if (currentImage !== image) {
                await page.evaluate(
                  ({ image, pixels }) => {
                    const binary = atob(pixels);
                    image.data = new Uint8Array(binary.length);
                    for (let i = 0; i < binary.length; i++) image.data[i] = binary.charCodeAt(i);
                    globalThis.comparisonImage = image;
                  },
                  {
                    image: { ...image, data: undefined },
                    pixels: Buffer.from(image.data).toString("base64"),
                  },
                );
                currentImage = image;
              }
              return page.evaluate(
                ({ key, options }) => {
                  const scanner = globalThis.comparisonScanners.get(key);
                  const start = performance.now();
                  const result = scanner.scan(globalThis.comparisonImage, options);
                  return [performance.now() - start, result];
                },
                { key, options },
              );
            },
            dispose() {},
          });
        }
        return result;
      },
      async close() {
        await browser.close();
        await new Promise((resolve) => server.close(resolve));
      },
    };
  } catch (error) {
    await browser?.close();
    await new Promise((resolve) => server.close(resolve));
    throw error;
  }
}

// Real browser UI and decoder tests with locally generated barcode images.
import { test, before, after, describe } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { webkit } from "playwright-core";
import { contentType, launchChrome, serve } from "./harness.mjs";

let server;
const root = new URL("../", import.meta.url);
before(async () => {
  server = await serve(async (path, response) => {
    const body = await readFile(new URL(`.${path}`, root));
    response.writeHead(200, { "content-type": contentType(path) }).end(body);
  });
});
after(() => server?.close());

async function fixturePhoto(page, multiple = false) {
  const png = await page.evaluate(async (multiple) => {
    const { fixture, ean8 } = await import("/test/fixtures.mjs");
    const images = [fixture().image, ...(multiple ? [ean8()] : [])];
    const canvas = document.createElement("canvas");
    canvas.width = 480;
    canvas.height = images.reduce((height, image) => height + image.height + 40, 0);
    const context = canvas.getContext("2d");
    context.fillStyle = "white";
    context.fillRect(0, 0, canvas.width, canvas.height);
    let top = 20;
    for (const image of images) {
      const rgba = new Uint8ClampedArray(image.width * image.height * 4);
      image.data.forEach((value, i) =>
        rgba.fill(value, i * 4, i * 4 + 3).fill(255, i * 4 + 3, i * 4 + 4),
      );
      context.putImageData(new ImageData(rgba, image.width), 0, top);
      top += image.height + 40;
    }
    return canvas.toDataURL("image/png").split(",")[1];
  }, multiple);
  return { name: "barcodes.png", mimeType: "image/png", buffer: Buffer.from(png, "base64") };
}

for (const [name, launch] of [
  ["Chrome", () => launchChrome()],
  ["WebKit", () => webkit.launch()],
]) {
  describe(`scanning workflows in ${name}`, () => {
    let browser;
    before(async () => {
      browser = await launch();
    });
    after(() => browser?.close());

    async function open() {
      const page = await browser.newPage();
      await page.goto(`${server.origin}/examples/scan-session.html`);
      // The module has run when it disables mode-independent undo for the empty session.
      await page.waitForFunction(
        () => typeof document.querySelector("#reset").onclick === "function",
      );
      return page;
    }

    test("photo scan-one finishes, freezes inputs, and resets for a fresh scan", async () => {
      const page = await open();
      try {
        await page.setInputFiles("#photo", await fixturePhoto(page));
        await page.waitForFunction(() => !document.querySelector("#result").hidden);
        const state = await page.evaluate(() => ({
          result: JSON.parse(document.querySelector("#result").textContent),
          photo: document.querySelector("#photo").disabled,
          start: document.querySelector("#start").disabled,
          stream: document.querySelector("video").srcObject,
        }));
        assert.deepEqual(state, {
          result: [{ format: "EAN13", text: "4006381333931", quantity: 1 }],
          photo: true,
          start: true,
          stream: null,
        });
        await page.click("#reset");
        assert.equal(await page.locator("#entries li").count(), 0);
        assert.equal(await page.locator("#photo").isEnabled(), true);
      } finally {
        await page.close();
      }
    });

    test("multiple photo codes require a deliberate single selection", async () => {
      const page = await open();
      try {
        await page.setInputFiles("#photo", await fixturePhoto(page, true));
        await page.waitForFunction(
          () => document.querySelectorAll("#candidates button").length === 2,
        );
        assert.equal(await page.locator("#entries li").count(), 0);
        await page.getByRole("button", { name: "Accept EAN8: 96385074", exact: true }).click();
        const result = JSON.parse(await page.locator("#result").textContent());
        assert.deepEqual(result, [{ format: "EAN8", text: "96385074", quantity: 1 }]);
      } finally {
        await page.close();
      }
    });

    test("list suppresses repeated uploads, supports extra units and undo, then exports", async () => {
      const page = await open();
      try {
        await page.selectOption("#mode", "list");
        const photo = await fixturePhoto(page);
        await page.setInputFiles("#photo", photo);
        await page.waitForFunction(
          () =>
            document.querySelectorAll("#entries li").length === 1 &&
            !document.querySelector("#photo").disabled,
        );
        await page.setInputFiles("#photo", photo);
        await page.waitForFunction(() =>
          document.querySelector("#status").textContent.includes("Already on the list"),
        );
        assert.match(await page.locator("#entries").textContent(), /1 unit/);
        await page.getByRole("button", { name: "Add another 4006381333931", exact: true }).click();
        assert.match(await page.locator("#entries").textContent(), /2 unit/);
        await page.click("#undo");
        assert.match(await page.locator("#entries").textContent(), /1 unit/);
        await page.click("#finish");
        assert.deepEqual(JSON.parse(await page.locator("#result").textContent()), [
          { format: "EAN13", text: "4006381333931", quantity: 1 },
        ]);
        assert.equal(await page.locator("#undo").isEnabled(), false);
      } finally {
        await page.close();
      }
    });

    test("camera refusal retains photo fallback and photo errors can be retried", async () => {
      const page = await open();
      try {
        await page.evaluate(() => {
          navigator.mediaDevices.getUserMedia = async () => {
            throw new Error("Permission denied");
          };
        });
        await page.click("#start");
        await page.waitForFunction(() =>
          document.querySelector("#status").textContent.includes("Choose a photo instead"),
        );
        assert.equal(await page.locator("#photo").isEnabled(), true);
        await page.setInputFiles("#photo", {
          name: "broken.png",
          mimeType: "image/png",
          buffer: Buffer.from("broken"),
        });
        await page.waitForFunction(() =>
          document.querySelector("#status").textContent.includes("Photo could not be read"),
        );
        await page.setInputFiles("#photo", await fixturePhoto(page));
        await page.waitForFunction(() => !document.querySelector("#result").hidden);
      } finally {
        await page.close();
      }
    });

    test("reset discards an in-flight photo result", async () => {
      const page = await open();
      try {
        await page.evaluate(async () => {
          const { Scanner } = await import("/dist/browser-medium.js");
          const scan = Scanner.prototype.scan;
          Scanner.prototype.scan = async function (...args) {
            const result = await scan.apply(this, args);
            await new Promise((resolve) => {
              globalThis.releasePhoto = resolve;
            });
            return result;
          };
        });
        await page.setInputFiles("#photo", await fixturePhoto(page));
        await page.waitForFunction(() => typeof globalThis.releasePhoto === "function");
        await page.click("#reset");
        await page.evaluate(() => globalThis.releasePhoto());
        // A later task lets the promise continuation and DOM updates complete.
        await page.evaluate(() => new Promise((resolve) => setTimeout(resolve, 0)));
        assert.equal(await page.locator("#entries li").count(), 0);
        assert.equal(await page.locator("#result").isVisible(), false);
        assert.equal(await page.locator("#status").textContent(), "Ready to scan.");
      } finally {
        await page.close();
      }
    });

    test("a confirmed camera result completes single mode and releases the stream", async () => {
      const page = await open();
      try {
        const photo = await fixturePhoto(page);
        await page.evaluate(async (data) => {
          const image = new Image();
          image.src = `data:image/png;base64,${data}`;
          await image.decode();
          const canvas = document.createElement("canvas");
          canvas.width = image.width;
          canvas.height = image.height;
          const context = canvas.getContext("2d");
          setInterval(() => context.drawImage(image, 0, 0), 30);
          navigator.mediaDevices.getUserMedia = async () => {
            globalThis.testStream = canvas.captureStream(30);
            return globalThis.testStream;
          };
        }, photo.buffer.toString("base64"));
        await page.click("#start");
        await page.waitForFunction(() => !document.querySelector("#result").hidden);
        assert.equal(
          await page.evaluate(() =>
            globalThis.testStream.getTracks().every((track) => track.readyState === "ended"),
          ),
          true,
        );
        assert.match(await page.locator("#result").textContent(), /4006381333931/);
      } finally {
        await page.close();
      }
    });
  });
}

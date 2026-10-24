// Shared by the browser tests: a local HTTP server and Google Chrome.
import { createServer } from "node:http";
import { chromium } from "playwright-core";

const types = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".wasm": "application/wasm",
};

/** The content type for a served file path. */
export function contentType(path) {
  const dot = path.lastIndexOf(".");
  return (dot >= 0 && types[path.slice(dot)]) || "application/octet-stream";
}

/**
 * Serve `handle(path, response)` on a free local port; a handler that throws
 * answers 404. Resolves to the origin and a `close` function.
 */
export async function serve(handle) {
  const server = createServer(async (request, response) => {
    try {
      await handle(new URL(request.url, "http://localhost").pathname, response);
    } catch {
      if (response.headersSent) response.destroy();
      else response.writeHead(404).end();
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return {
    origin: `http://127.0.0.1:${server.address().port}`,
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

/** The installed Google Chrome, or the Chromium build at CHROME_PATH. */
export function launchChrome(options = {}) {
  const { CHROME_PATH } = process.env;
  return chromium.launch({
    ...(CHROME_PATH ? { executablePath: CHROME_PATH } : { channel: "chrome" }),
    ...options,
  });
}

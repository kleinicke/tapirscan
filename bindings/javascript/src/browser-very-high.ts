// tapirscan/browser/very-high: tapirscan/browser with only the very-high WASM bundled.
import { BrowserScanner, oneShot, type SingleModeOptions } from "./browser-scanner.js";

export * from "./browser-types.js";

/** tapirscan/browser's Scanner in very-high mode; bundlers emit only its WASM file. */
export class Scanner extends BrowserScanner {
  constructor(options: SingleModeOptions = {}) {
    super(
      options,
      () =>
        new Worker(new URL("./browser-worker-very-high.js", import.meta.url), { type: "module" }),
      "very-high",
    );
  }
}

/** Scan or inspect one image with a temporary scanner. Reuse a Scanner for several images. */
export const { scan, inspect } = oneShot(Scanner);

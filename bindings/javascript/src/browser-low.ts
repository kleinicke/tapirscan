// tapirscan/browser/low: tapirscan/browser with only the low WASM bundled.
import { BrowserScanner, oneShot, type SingleModeOptions } from "./browser-scanner.js";

export * from "./browser-types.js";

/** tapirscan/browser's Scanner in low mode; bundlers emit only its WASM file. */
export class Scanner extends BrowserScanner {
  constructor(options: SingleModeOptions = {}) {
    super(
      options,
      () => new Worker(new URL("./browser-worker-low.js", import.meta.url), { type: "module" }),
      "low",
    );
  }
}

/** Scan or inspect one image with a temporary scanner. Reuse a Scanner for several images. */
export const { scan, inspect } = oneShot(Scanner);

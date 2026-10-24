// tapirscan/browser/medium: tapirscan/browser with only the medium WASM bundled.
import { BrowserScanner, oneShot, type SingleModeOptions } from "./browser-scanner.js";

export * from "./browser-types.js";
export type {
  SingleModeOptions as ScannerOptions,
  SingleModeOneShotOptions as OneShotOptions,
} from "./browser-scanner.js";

/** tapirscan/browser's Scanner in medium mode; bundlers emit only its WASM file. */
export class Scanner extends BrowserScanner {
  constructor(options: SingleModeOptions = {}) {
    super(
      options,
      () => new Worker(new URL("./browser-worker-medium.js", import.meta.url), { type: "module" }),
      "medium",
    );
  }
}

/** Scan or inspect one image with a temporary scanner. Reuse a Scanner for several images. */
export const { scan, inspect } = oneShot(Scanner);

// tapirscan/browser: the core API, run in a bundled worker, for browser image sources.
// Its worker bundles every mode and Turbo preset; tapirscan/browser/<mode> bundles one.
import { BrowserScanner, oneShot, type ScannerOptions } from "./browser-scanner.js";

export * from "./browser-types.js";
export type { OneShotOptions } from "./browser-scanner.js";

/**
 * A reusable scanner running in a dedicated worker, so scanning never blocks the
 * page. Construction is synchronous and starts loading in the background; during
 * server rendering it does nothing. Inputs and options are captured when a call
 * starts, and calls reach the worker in call order. Results are deeply frozen and
 * survive disposal. Call dispose() when done to stop the worker.
 */
export class Scanner extends BrowserScanner {
  constructor(options: ScannerOptions = {}) {
    super(
      options,
      () => new Worker(new URL("./browser-worker.js", import.meta.url), { type: "module" }),
    );
  }
}

/** Scan or inspect one image with a temporary scanner. Reuse a Scanner for several images. */
export const { scan, inspect } = oneShot<ScannerOptions>(Scanner);

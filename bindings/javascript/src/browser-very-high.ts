// tapirscan/browser/very-high: tapirscan/browser with only the very-high WASM bundled.
import {
  BrowserScanner,
  once,
  type ImageSource,
  type ModeScannerOptions,
  type OneShotOptions,
} from "./browser-scanner.js";
import type { InspectionResult, ScanResult } from "./index.js";

export * from "./browser-types.js";

type Options = ModeScannerOptions<"very-high">;

/** tapirscan/browser's Scanner, always in very-high mode; bundles only that WASM file. */
export class Scanner extends BrowserScanner {
  constructor(options: Options = {}) {
    super(
      options,
      () =>
        new Worker(new URL("./browser-worker-very-high.js", import.meta.url), { type: "module" }),
      "very-high",
    );
  }
}

/** Scan one image with a temporary scanner. Reuse a Scanner for several images. */
export function scan(
  source: ImageSource,
  options: Options & Pick<OneShotOptions, "transfer"> = {},
): Promise<ScanResult> {
  return once(Scanner, options, (scanner, call) => scanner.scan(source, call));
}

/** Inspect one image with a temporary scanner. */
export function inspect(
  source: ImageSource,
  options: Options & Pick<OneShotOptions, "transfer"> = {},
): Promise<InspectionResult> {
  return once(Scanner, options, (scanner, call) => scanner.inspect(source, call));
}

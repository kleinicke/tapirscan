// tapirscan/support: whether this runtime can scan, checked without loading a scanner.
// It imports nothing, so an app can call it before downloading the scanner code.

/** The smallest module using a SIMD instruction (i8x16.splat), as feature detectors use. */
const simdProbe = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15,
  253, 98, 11,
]);

/** Whether WebAssembly SIMD is available, which the core `tapirscan` entry needs. */
export function wasmSupported(): boolean {
  try {
    return typeof WebAssembly === "object" && WebAssembly.validate(simdProbe);
  } catch {
    return false;
  }
}

/**
 * Whether `tapirscan/browser` scanners can run here: WebAssembly SIMD, module
 * workers and `OffscreenCanvas` (Chrome 91, Firefox 114, Safari 16.4 or later).
 * Scanners that are created anyway reject with the `ScannerError` code `"unsupported"`.
 */
export function browserSupported(): boolean {
  return (
    wasmSupported() &&
    typeof Worker === "function" &&
    typeof OffscreenCanvas === "function" &&
    typeof createImageBitmap === "function" &&
    moduleWorkersSupported()
  );
}

let moduleWorkers: boolean | undefined;

/** Whether workers run modules (Firefox 105 to 113 lack them): browsers that do read `type`. */
function moduleWorkersSupported(): boolean {
  if (moduleWorkers !== undefined) return moduleWorkers;
  let read = false;
  const options = {
    get type() {
      read = true;
      return "module" as const;
    },
  };
  try {
    // The options are read before the invalid URL throws, so nothing is fetched.
    // Trusted Types may still check or report the string URL; the result holds.
    new Worker("http://[", options).terminate();
  } catch {
    // Expected: the URL is invalid.
  }
  moduleWorkers = read;
  return read;
}

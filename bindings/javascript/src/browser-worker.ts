// The tapirscan/browser worker: every mode and Turbo preset.
import { serve } from "./browser-worker-core.js";

// Literal URLs let bundlers such as Vite and webpack emit these assets without
// configuration. Keep the file names in sync with index.ts (verify-package checks).
serve({
  modes: {
    low: new URL("../wasm/low.wasm", import.meta.url),
    medium: new URL("../wasm/medium.wasm", import.meta.url),
    high: new URL("../wasm/high.wasm", import.meta.url),
    "very-high": new URL("../wasm/very-high.wasm", import.meta.url),
  },
  turbo: {
    2: new URL("../wasm/experimental-turbo2.wasm", import.meta.url),
    4: new URL("../wasm/experimental-turbo4.wasm", import.meta.url),
    8: new URL("../wasm/experimental-turbo8.wasm", import.meta.url),
    16: new URL("../wasm/experimental-turbo16.wasm", import.meta.url),
  },
});

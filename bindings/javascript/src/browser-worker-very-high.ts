// The tapirscan/browser/very-high worker: only the very-high WASM, so bundlers emit only that file.
import { serve } from "./browser-worker-core.js";

serve({ modes: { "very-high": new URL("../wasm/very-high.wasm", import.meta.url) } });

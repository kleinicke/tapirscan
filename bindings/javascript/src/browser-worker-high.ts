// The tapirscan/browser/high worker: only the high WASM, so bundlers emit only that file.
import { serve } from "./browser-worker-core.js";

serve({ modes: { high: new URL("../wasm/high.wasm", import.meta.url) } });

// The tapirscan/browser/low worker: only the low WASM, so bundlers emit only that file.
import { serve } from "./browser-worker-core.js";

serve({ modes: { low: new URL("../wasm/low.wasm", import.meta.url) } });

// The tapirscan/browser/medium worker: only the medium WASM, so bundlers emit only that file.
import { serve } from "./browser-worker-core.js";

serve({ modes: { medium: new URL("../wasm/medium.wasm", import.meta.url) } });

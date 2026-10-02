import { Scanner } from "tapirscan";
import mediumWasmUrl from "tapirscan/wasm/medium.wasm?url";

async function initialize() {
  const response = await fetch(mediumWasmUrl);
  if (!response.ok) throw new Error(`WASM load failed: ${response.status}`);
  const bytes = await response.arrayBuffer();
  const scanner = await Scanner.create({ loadWasm: async () => bytes });
  self.onmessage = ({ data: image }) => {
    try {
      self.postMessage({ values: scanner.scan(image).map((b) => b.text) });
    } catch (error) {
      self.postMessage({ error: error.message });
    }
  };
  self.postMessage({ ready: true });
}
initialize().catch((error) => self.postMessage({ error: error.message, fatal: true }));

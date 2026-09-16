// Serve this directory alongside ../dist and ../wasm, or adapt the import for your bundler.
import { Scanner } from "../dist/index.js";

let scanner;
self.onmessage = async ({ data: { id, type, options, image } }) => {
  try {
    if (type === "init") {
      if (scanner) throw Error("Scanner already initialized");
      scanner = await Scanner.create(options);
      self.postMessage({ id });
    } else if (type === "scan") {
      if (!scanner) throw Error("Scanner not initialized");
      self.postMessage({ id, result: scanner.scan(image, options) });
    } else {
      throw Error(`Unknown worker request: ${type}`);
    }
  } catch (error) {
    self.postMessage({ id, error: { message: error.message, code: error.code } });
  }
};

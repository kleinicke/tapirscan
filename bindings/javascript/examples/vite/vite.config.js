import { defineConfig } from "vite";

export default defineConfig({
  // The dev server must serve Tapirscan's worker and WASM files from the package.
  optimizeDeps: { exclude: ["tapirscan"] },
});

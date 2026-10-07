import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { releases } from "./scripts/releases.mjs";

// Published versions are package.json aliases; derive the registry and hosts from them.
function tapirscanReleases(): Plugin {
  const list = releases();
  const modules: Record<string, string> = {
    "virtual:tapirscan-releases": `export const releaseVersions = ${JSON.stringify(
      list.map(({ version }) => version),
    )};\nexport const latestRelease = ${JSON.stringify(list.at(-1)!.version)};`,
    "virtual:tapirscan-hosts": `${list
      .map(({ alias }, index) => `import * as host${index} from "${alias}";`)
      .join("\n")}\nexport const hosts = {${list
      .map(({ version }, index) => `${JSON.stringify(version)}: host${index}`)
      .join(", ")}};`,
  };
  return {
    name: "tapirscan-releases",
    resolveId: (id) => (id in modules ? `\0${id}` : undefined),
    load: (id) => (id.startsWith("\0") ? modules[id.slice(1)] : undefined),
  };
}

// Quagga2 1.12.1 initializes raw ImageWrapper input without a framegrabber.
// Its new start guard accidentally rejects that supported worker path.
function quaggaRawWorker(): Plugin {
  return {
    name: "quagga-raw-worker-start",
    enforce: "pre",
    transform(code, id) {
      if (!id.endsWith("/@ericblade/quagga2/dist/quagga.min.js")) return;
      const guard = "if(!ui.framegrabber)throw new Error";
      if (code.split(guard).length !== 2)
        throw new Error("Review Quagga2 raw-worker guard for this package version");
      const rawGrab =
        "else{var n;e.context.framegrabber.attachData(null===(n=e.context.inputImageWrapper)||void 0===n?void 0:n.data),e.context.framegrabber.grab(),e.locateAndDecode()}";
      if (code.split(rawGrab).length !== 2)
        throw new Error("Review Quagga2 raw-worker update for this package version");
      return code
        .replace(guard, "if(ui.onUIThread&&!ui.framegrabber)throw new Error")
        .replace(rawGrab, "else{e.locateAndDecode()}");
    },
  };
}
export default defineConfig({
  plugins: [tapirscanReleases(), svelte()],
  worker: { plugins: () => [tapirscanReleases(), quaggaRawWorker()] },
  base: "./",
});

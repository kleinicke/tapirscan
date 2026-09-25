import turbo from "./turbo.json";
import versions from "./scanner-versions.json";
import { Scanner, type Mode, type Format } from "tapirscan";
import type { Region } from "./types";
let scanner: Scanner | undefined;
let key = "";
self.onmessage = async ({
  data,
}: MessageEvent<{
  width: number;
  height: number;
  buffer: ArrayBuffer;
  scannerVersion: Mode;
  engine?: string;
  releaseVersion?: string;
  formats: Format[];
  finishCandidates?: boolean;
  benchmark?: boolean;
  engineBaseUrl: string;
}>) => {
  try {
    const release = versions.versions.find(
      (entry) => entry.version === (data.releaseVersion ?? versions.default),
    );
    const experimental =
      data.engine === "turbo" ? turbo : turbo.variants.find((entry) => entry.key === data.engine);
    const isTurbo = data.engine?.startsWith("turbo") ?? false;
    const build = isTurbo
      ? experimental
      : release?.modes.find((entry) => entry.mode === data.scannerVersion);
    const identity = isTurbo ? experimental?.id : release?.version;
    if (!identity || !build) throw Error("Unknown Tapirscan version or effort");
    const formats = data.formats;
    const mode = isTurbo ? "low" : data.scannerVersion;
    const next = JSON.stringify([identity, mode, formats]);
    if (!scanner || next !== key) {
      self.postMessage({ type: "initializing" });
      const fresh = await Scanner.create({
        mode,
        formats,
        loadWasm: async () => {
          const response = await fetch(new URL(build.file, data.engineBaseUrl));
          if (!response.ok) throw Error(`Could not load Tapirscan ${identity}`);
          const bytes = await response.arrayBuffer();
          const digest = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)))
            .map((value) => value.toString(16).padStart(2, "0"))
            .join("");
          if (digest !== build.sha256) throw Error("Tapirscan asset hash mismatch");
          return bytes;
        },
      });
      scanner?.dispose();
      scanner = fresh;
      key = next;
    }
    const started = performance.now();
    const result = scanner.scan(
      {
        data: new Uint8Array(data.buffer),
        width: data.width,
        height: data.height,
        channels: 4,
        stride: data.width * 4,
      },
      { debug: true, extendedBudget: isTurbo ? false : (data.finishCandidates ?? false) },
    );
    const scanMs = performance.now() - started;
    const diagnostic = result.debug;
    if (!diagnostic) throw new Error("Scanner diagnostics are unavailable");
    const recovery = diagnostic.recovery as { proposals?: Region[] } | undefined;
    const proposals = diagnostic.localization?.proposals ?? [];
    const candidates = (diagnostic.scan.candidates ?? []) as { detections?: unknown[] }[];
    const localization = diagnostic.localization as { omitted?: number } | undefined;
    const areaCounts = {
      proposed: proposals.length + (recovery?.proposals?.length ?? 0),
      checked: candidates.length,
      withoutRead: candidates.filter((candidate) => !candidate.detections?.length).length,
      omitted: localization?.omitted ?? null,
    };
    // Public results contain every selected format; primary detailRegions are EAN-only.
    const regions = [
      ...result.barcodes,
      ...diagnostic.regions.undecoded.map((region) => ({ polygon: region.polygon, text: "" })),
    ];
    self.postMessage({
      result: {
        regions,
        areaCounts: isTurbo ? undefined : areaCounts,
        proposals: [...proposals, ...(recovery?.proposals ?? [])],
        searchWindows: diagnostic.searchWindows,
        width: data.width,
        height: data.height,
        scanMs: data.benchmark ? scanMs : result.elapsedMs,
        unfinished: result.unfinished,
      },
    });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};

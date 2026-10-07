import * as next from "tapirscan";
import { hosts, type Host } from "virtual:tapirscan-hosts";
import { latestRelease } from "virtual:tapirscan-releases";
import type { Mode, Format } from "tapirscan";
import type { Region } from "./types";

type Diagnostics = {
  recovery?: { proposals?: Region[] };
  localization?: { proposals?: Region[]; omitted?: number };
  scan: { candidates?: { detections?: unknown[] }[] };
  regions: { undecoded: { polygon: unknown }[] };
  searchWindows?: unknown;
};
type ScanResult = {
  barcodes: unknown[];
  elapsedMs: number;
  diagnostics?: Diagnostics;
  debug?: Diagnostics;
};
type Session = {
  dispose(): void;
  inspect?(input: unknown, options: object): ScanResult;
  scan?(input: unknown, options: object): ScanResult;
};
// Each released package runs with its own host; "next" is the current repository build.
let scanner: Session | undefined;
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
    const release = data.releaseVersion ?? latestRelease;
    const host: Host | undefined = release === "next" ? (next as unknown as Host) : hosts[release];
    if (!host) throw Error(`Unknown Tapirscan version ${release}`);
    // Numbered Turbo readers (experimental-turbo<N>.wasm) exist only in the current build.
    const preset = /^turbo(\d+)$/.exec(data.engine ?? "")?.[1];
    const isTurbo = preset !== undefined;
    if (isTurbo && release !== "next") throw Error("Turbo readers come from the current build");
    const formats = data.formats;
    const mode = isTurbo ? "low" : data.scannerVersion;
    const file = isTurbo ? `experimental-turbo${preset}` : mode;
    const sessionKey = JSON.stringify([release, file, formats]);
    if (!scanner || sessionKey !== key) {
      self.postMessage({ type: "initializing" });
      const response = await fetch(new URL(`${release}/${file}.wasm`, data.engineBaseUrl));
      if (!response.ok) throw Error(`Could not load Tapirscan ${release} ${file}`);
      const bytes = await response.arrayBuffer();
      const fresh = await host.Scanner.create({
        ...(isTurbo ? { experimentalTurbo: Number(preset) } : { mode }),
        formats,
        loadWasm: async () => bytes,
      });
      scanner?.dispose();
      scanner = fresh as unknown as Session;
      key = sessionKey;
    }
    const started = performance.now();
    const input = {
      data: new Uint8Array(data.buffer),
      width: data.width,
      height: data.height,
      channels: 4 as const,
      stride: data.width * 4,
    };
    const extendedBudget = isTurbo ? false : (data.finishCandidates ?? false);
    // Releases before 1.3 report diagnostics through scan({ debug }).
    const result = scanner.inspect
      ? scanner.inspect(input, { extendedBudget })
      : scanner.scan!(input, { extendedBudget, debug: true });
    const scanMs = performance.now() - started;
    const diagnostic = result.diagnostics ?? result.debug;
    if (!diagnostic) throw new Error("Scanner diagnostics are unavailable");
    const recovery = diagnostic.recovery as { proposals?: Region[] } | undefined;
    const proposals = diagnostic.localization?.proposals ?? [];
    const candidates = diagnostic.scan.candidates ?? [];
    const localization = diagnostic.localization;
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
      },
    });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};

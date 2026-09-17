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
  formats: Format[];
  finishCandidates?: boolean;
  engineBaseUrl: string;
}>) => {
  try {
    const next = JSON.stringify([data.scannerVersion, data.formats]);
    if (!scanner || next !== key) {
      self.postMessage({ type: "initializing" });
      const fresh = await Scanner.create({
        mode: data.scannerVersion,
        formats: data.formats,
        wasmBaseUrl: data.engineBaseUrl,
      });
      scanner?.dispose();
      scanner = fresh;
      key = next;
    }
    const result = scanner.scan(
      {
        data: new Uint8Array(data.buffer),
        width: data.width,
        height: data.height,
        channels: 4,
        stride: data.width * 4,
      },
      { debug: true, extendedBudget: data.finishCandidates ?? false },
    );
    const diagnostic = result.debug;
    if (!diagnostic) throw new Error("Scanner diagnostics are unavailable");
    const recovery = diagnostic.recovery as { proposals?: Region[] } | undefined;
    const proposals = diagnostic.localization?.proposals ?? [];
    // Public results contain every selected format; primary detailRegions are EAN-only.
    const regions = [
      ...result.barcodes,
      ...diagnostic.regions.undecoded.map((region) => ({ polygon: region.polygon, text: "" })),
    ];
    self.postMessage({
      result: {
        regions,
        proposals: [...proposals, ...(recovery?.proposals ?? [])],
        searchWindows: diagnostic.searchWindows,
        width: data.width,
        height: data.height,
        scanMs: result.elapsedMs,
        unfinished: result.unfinished,
      },
    });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};

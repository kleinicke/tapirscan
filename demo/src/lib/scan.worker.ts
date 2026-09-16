import { Scanner, type Mode, type Format } from "tapirscan";
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
      { debug: true },
    );
    const diagnostic = result.debug!;
    const reads = diagnostic.scan.barcodes;
    const decoded = new Set(
      reads.flatMap((b) => ("candidate_indices" in b ? b.candidate_indices : [])),
    );
    const proposals = diagnostic.localization?.proposals ?? [];
    const unread = proposals.filter((_, i) => !decoded.has(i));
    const regions = diagnostic.detailRegions ?? diagnostic.scan.regions ?? [...reads, ...unread];
    self.postMessage({
      result: {
        regions,
        proposals: [...proposals, ...(diagnostic.recovery?.proposals ?? [])],
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

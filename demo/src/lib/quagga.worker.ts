import "./quagga-worker-env";
import { scanQuagga } from "./quagga";
import type { Format } from "tapirscan";
self.onmessage = async ({
  data,
}: MessageEvent<{ width: number; height: number; buffer: ArrayBuffer; formats: Format[] }>) => {
  try {
    const result = await scanQuagga(
      new ImageData(new Uint8ClampedArray(data.buffer), data.width, data.height),
      data.formats,
    );
    self.postMessage({ result });
  } catch (error) {
    self.postMessage({ error: error instanceof Error ? error.message : String(error) });
  }
};

import Quagga, { type ImageWrapper, type QuaggaJSCodeReader } from "@ericblade/quagga2";
import type { Format } from "tapirscan";
import type { Result } from "./types";

const readers: Partial<Record<Format, QuaggaJSCodeReader>> = {
  EAN13: "ean_reader",
  EAN8: "ean_8_reader",
  UPCA: "upc_reader",
  UPCE: "upc_e_reader",
  Code128: "code_128_reader",
  Code39: "code_39_reader",
  Code93: "code_93_reader",
  Codabar: "codabar_reader",
  ITF: "i2of5_reader",
};
export async function scanQuagga(image: ImageData, formats: readonly Format[]): Promise<Result> {
  const selected = formats
    .map((format) => readers[format])
    .filter((reader): reader is QuaggaJSCodeReader => !!reader);
  if (!selected.length) throw new Error("Quagga2 supports none of the selected linear formats.");
  const start = performance.now();
  const gray = new Uint8Array(image.width * image.height);
  for (let i = 0; i < gray.length; i++)
    gray[i] = (image.data[i * 4] + 2 * image.data[i * 4 + 1] + image.data[i * 4 + 2]) / 4;
  // Upstream declares this constructor as an instance, though it exposes a class.
  const Wrapper = Quagga.ImageWrapper as unknown as typeof ImageWrapper;
  const wrapper = new Wrapper({ x: image.width, y: image.height, type: "XYSize" }, gray);
  const result = await new Promise<QuaggaRead | QuaggaRead[] | null>((resolve, reject) => {
    const processed = (value: unknown) => {
      Quagga.offProcessed(processed);
      resolve(value as QuaggaRead | QuaggaRead[] | null);
    };
    Quagga.onProcessed(processed);
    try {
      void Quagga.init(
        {
          numOfWorkers: 0,
          locate: true,
          locator: { halfSample: false, patchSize: "large" },
          decoder: { readers: selected, multiple: true },
        },
        (error: unknown) => {
          if (error) {
            Quagga.offProcessed(processed);
            reject(
              error instanceof Error
                ? error
                : new Error(typeof error === "string" ? error : "Quagga2 initialization failed"),
            );
          } else Quagga.start();
        },
        wrapper,
      );
    } catch (error) {
      Quagga.offProcessed(processed);
      reject(
        error instanceof Error
          ? error
          : new Error(typeof error === "string" ? error : "Quagga2 initialization failed"),
      );
    }
  });
  const reads = Array.isArray(result) ? result : (result?.barcodes ?? (result ? [result] : []));
  return {
    regions: reads.flatMap((read) => {
      const text = read.codeResult?.code;
      if (!text || !read.box) return [];
      return [{ text, polygon: read.box.map(([x, y]) => [x, y] as const) }];
    }),
    proposals: [],
    width: image.width,
    height: image.height,
    scanMs: performance.now() - start,
    unfinished: false,
  };
}

// No-read and multiple-result responses omit fields declared required upstream.
interface QuaggaRead {
  codeResult?: { code?: string | null };
  box?: [number, number][];
  barcodes?: QuaggaRead[];
}

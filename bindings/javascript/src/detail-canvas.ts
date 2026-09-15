/** Canvas interpolation in browsers; deterministic bilinear RGB interpolation in Node. */
interface Pixels {
  data: Uint8ClampedArray;
  width: number;
  height: number;
}
export function makeCanvas(width: number, height: number) {
  if (typeof OffscreenCanvas !== "undefined") {
    const canvas = new OffscreenCanvas(width, height);
    return {
      get width() {
        return canvas.width;
      },
      set width(value: number) {
        canvas.width = value;
      },
      get height() {
        return canvas.height;
      },
      set height(value: number) {
        canvas.height = value;
      },
      native: canvas,
      getContext(_kind: string, options?: { willReadFrequently?: boolean }) {
        const context = canvas.getContext("2d", options);
        if (!context) throw Error("Could not create recovery canvas");
        return {
          imageSmoothingEnabled: true,
          putImageData(pixels: Pixels, x: number, y: number) {
            context.putImageData(
              new ImageData(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height),
              x,
              y,
            );
          },
          drawImage(
            source: { native: OffscreenCanvas },
            x: number,
            y: number,
            w: number,
            h: number,
          ) {
            context.imageSmoothingEnabled = true;
            context.drawImage(source.native, x, y, w, h);
          },
          getImageData(x: number, y: number, w: number, h: number) {
            return context.getImageData(x, y, w, h);
          },
        };
      },
    };
  }
  let pixels: Pixels = { data: new Uint8ClampedArray(), width, height };
  const canvas = {
    width,
    height,
    getContext() {
      return {
        imageSmoothingEnabled: true,
        putImageData(value: Pixels) {
          pixels = value;
        },
        getImageData() {
          return pixels;
        },
        drawImage(
          source: { getContext(): { getImageData(): Pixels } },
          _x: number,
          _y: number,
          w: number,
          h: number,
        ) {
          const input = source.getContext().getImageData();
          const data = new Uint8ClampedArray(w * h * 4);
          for (let y = 0; y < h; y++) {
            const sy = Math.max(
              0,
              Math.min(input.height - 1, ((y + 0.5) * input.height) / h - 0.5),
            );
            const y0 = Math.floor(sy),
              y1 = Math.min(input.height - 1, y0 + 1),
              fy = sy - y0;
            for (let x = 0; x < w; x++) {
              const sx = Math.max(
                0,
                Math.min(input.width - 1, ((x + 0.5) * input.width) / w - 0.5),
              );
              const x0 = Math.floor(sx),
                x1 = Math.min(input.width - 1, x0 + 1),
                fx = sx - x0;
              for (let c = 0; c < 4; c++) {
                const a = input.data[(y0 * input.width + x0) * 4 + c],
                  b = input.data[(y0 * input.width + x1) * 4 + c];
                const d = input.data[(y1 * input.width + x0) * 4 + c],
                  e = input.data[(y1 * input.width + x1) * 4 + c];
                data[(y * w + x) * 4 + c] = Math.round(
                  (a * (1 - fx) + b * fx) * (1 - fy) + (d * (1 - fx) + e * fx) * fy,
                );
              }
            }
          }
          pixels = { data, width: w, height: h };
        },
      };
    },
  };
  return canvas;
}

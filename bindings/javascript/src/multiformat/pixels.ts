import type { Image } from "../runtime-host.mjs";
const littleEndian = new Uint8Array(new Uint32Array([1]).buffer)[0] === 1;
/** Input dimensions are validated by the scanner. Preserve its integer luma rule. */
export function toGray({ data, width, height, channels, stride }: Image): Uint8Array {
  const count = width * height;
  if (channels === 1 && stride === width) return data.slice(0, count);
  const gray = new Uint8Array(count);
  if (channels === 4 && stride === width * 4 && littleEndian && data.byteOffset % 4 === 0) {
    const words = new Uint32Array(data.buffer, data.byteOffset, count);
    for (let i = 0; i < count; i++) {
      const value = words[i];
      gray[i] =
        ((value & 255) * 77 + ((value >>> 8) & 255) * 150 + ((value >>> 16) & 255) * 29 + 128) >>>
        8;
    }
  } else if (channels === 4 && stride === width * 4) {
    for (let i = 0, j = 0; j < count; i += 4, j++)
      gray[j] = (data[i] * 77 + data[i + 1] * 150 + data[i + 2] * 29 + 128) >> 8;
  } else if (channels === 3 && stride === width * 3) {
    for (let i = 0, j = 0; j < count; i += 3, j++)
      gray[j] = (data[i] * 77 + data[i + 1] * 150 + data[i + 2] * 29 + 128) >> 8;
  } else if (channels === 1) {
    for (let y = 0; y < height; y++)
      gray.set(data.subarray(y * stride, y * stride + width), y * width);
  } else {
    for (let y = 0; y < height; y++) {
      for (let x = 0, i = y * stride, j = y * width; x < width; x++, i += channels, j++)
        gray[j] = (data[i] * 77 + data[i + 1] * 150 + data[i + 2] * 29 + 128) >> 8;
    }
  }
  return gray;
}

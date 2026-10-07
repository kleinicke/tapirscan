// Pixel layout rules shared by the core and the browser entry. No WASM imports, so
// the browser entry can validate on the page without loading the core.

/** Decoded pixels: `ImageData` or an explicit gray, RGB or RGBA buffer. */
export interface PixelInput {
  readonly data: Uint8Array | Uint8ClampedArray;
  readonly width: number;
  readonly height: number;
  readonly channels?: 1 | 3 | 4;
  readonly stride?: number;
}

/** A validated layout; `addressed` bytes from the start of `data` hold the image. */
export interface PixelLayout {
  readonly data: Uint8Array | Uint8ClampedArray;
  readonly width: number;
  readonly height: number;
  readonly channels: 1 | 3 | 4;
  readonly stride: number;
  readonly addressed: number;
}

/** Validate decoded pixels against the scanner limits without copying them. */
export function pixelLayout(image: unknown): PixelLayout {
  if (image === null || typeof image !== "object" || !("data" in image))
    throw new TypeError("Expected ImageData or decoded pixels");
  const { data, width, height } = image as PixelInput;
  const explicit = "channels" in image;
  if (!explicit && !(data instanceof Uint8ClampedArray))
    throw new TypeError("Use ImageData or an explicit buffer with channels");
  const channels = explicit ? (image as PixelInput).channels : 4;
  const stride = (explicit ? (image as PixelInput).stride : undefined) ?? width * (channels ?? 0);
  const addressed = (height - 1) * stride + width * (channels ?? 0);
  if (
    !Number.isSafeInteger(width) ||
    !Number.isSafeInteger(height) ||
    width < 3 ||
    height < 3 ||
    width * height > 32 * 1024 * 1024 ||
    (channels !== 1 && channels !== 3 && channels !== 4) ||
    !Number.isSafeInteger(stride) ||
    stride < width * channels ||
    addressed > 128 * 1024 * 1024 ||
    !(data instanceof Uint8Array || data instanceof Uint8ClampedArray) ||
    data.byteLength < addressed
  )
    throw new TypeError("Invalid image dimensions, channels, stride or buffer (maximum 128 MiB)");
  return { data, width, height, channels, stride, addressed };
}

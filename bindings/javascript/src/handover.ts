// Bitmaps that tapirscan/camera hands to a tapirscan/browser scanner, which then
// sends them to its worker without the copy it otherwise makes. Internal.

const handedOver = new WeakSet<ImageBitmap>();

/** Give up `bitmap`: a scanner that receives it may transfer and close it. */
export function handOver(bitmap: ImageBitmap): void {
  handedOver.add(bitmap);
}

/** Whether `bitmap` was handed over; true once, for the scan that takes it. */
export function takeOver(bitmap: ImageBitmap): boolean {
  return handedOver.delete(bitmap);
}

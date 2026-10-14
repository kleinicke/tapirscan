// Internal row-by-row scanning for the browser worker; not a package entry point.
import type { InspectionResult, ScanOptions, ScanResult, Scanner } from "./index.js";
import type { RowReader } from "./rust-session.js";

export type { RowReader } from "./rust-session.js";
export { workingPixels } from "./rust-session.js";

/** Dimensions of an image delivered through a RowReader. */
export interface RowImage {
  width: number;
  height: number;
  channels: 1 | 3 | 4;
}

type RowScan = (
  image: RowImage,
  read: RowReader,
  options: ScanOptions,
  inspect: boolean,
) => ScanResult | InspectionResult;

const rowScans = new WeakMap<Scanner, RowScan>();

/** Called by each Scanner as it is constructed. */
export function registerRowScan(scanner: Scanner, scan: RowScan): void {
  rowScans.set(scanner, scan);
}

/**
 * Scan an image above the working size row by row: `read` fills each chunk on
 * demand. Results equal scanning the same pixels as one buffer.
 */
export function scanRows(
  scanner: Scanner,
  image: RowImage,
  read: RowReader,
  options: ScanOptions,
  inspect: boolean,
): ScanResult | InspectionResult {
  const scan = rowScans.get(scanner);
  if (!scan) throw new TypeError("Expected a scanner");
  return scan(image, read, options, inspect);
}

/** Explicit opt-in formats for the independent Medium extension. */
export const formatBits = {
  EAN13: 1,
  UPCA: 2,
  EAN8: 4,
  UPCE: 8,
  Code128: 16,
  Code39: 32,
  ITF: 64,
  Codabar: 128,
  Code93: 256,
  QRCode: 512,
  DataMatrix: 1024,
  PDF417: 2048,
  Aztec: 4096,
  DataBar: 8192,
  DataBarExpanded: 16384,
  MaxiCode: 131072,
} as const;
export type Format = keyof typeof formatBits;
export const retailFormats: readonly Format[] = ["EAN13", "UPCA", "EAN8", "UPCE"];
export const commonLinearFormats: readonly Format[] = [
  ...retailFormats,
  "Code128",
  "Code39",
  "ITF",
];
export const commonFormats: readonly Format[] = [...commonLinearFormats, "QRCode", "DataMatrix"];
export const linearFormats: readonly Format[] = [
  ...commonLinearFormats,
  "Codabar",
  "Code93",
  "DataBar",
  "DataBarExpanded",
];
export const matrixFormats: readonly Format[] = [
  "QRCode",
  "DataMatrix",
  "PDF417",
  "Aztec",
  "MaxiCode",
];
export type FormatSelection =
  Format | readonly Format[] | "retail" | "common1D" | "common" | "1D" | "2D" | "all";
export function resolveFormats(input?: readonly string[] | string): Format[] {
  if (input === "retail") return [...retailFormats];
  if (input === "common1D") return [...commonLinearFormats];
  if (input === "common") return [...commonFormats];
  if (input === "1D") return [...linearFormats];
  if (input === "2D") return [...matrixFormats];
  if (input === "all") return [...linearFormats, ...matrixFormats];
  if (input === undefined) return [...retailFormats];
  if (typeof input === "string" && Object.hasOwn(formatBits, input)) return [input as Format];
  if (!Array.isArray(input) || input.length === 0)
    throw Error("Choose at least one barcode format.");
  for (const value of input as readonly unknown[]) {
    if (typeof value !== "string") throw Error("Barcode format must be a string.");
    if (!Object.hasOwn(formatBits, value)) throw Error(`Unsupported barcode format: ${value}`);
  }
  return [...new Set(input)] as Format[];
}
export function maskFor(formats: readonly Format[]): number {
  return formats.reduce((mask, f) => mask | formatBits[f], 0);
}

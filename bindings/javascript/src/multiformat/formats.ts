/** Explicit opt-in formats for the independent Medium extension. */
import {
  commonFormats,
  commonLinearFormats,
  formatBits,
  linearFormats,
  matrixFormats,
  retailFormats,
  type Format,
} from "./format-registry.js";

export {
  commonFormats,
  commonLinearFormats,
  formatBits,
  linearFormats,
  matrixFormats,
  retailFormats,
  type Format,
};
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

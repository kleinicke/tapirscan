/** Format presets and validation of format selections. */
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
/** Own-property check that works in Chrome 91 (`Object.hasOwn` needs Chrome 93). */
export function hasKey(object: object, key: PropertyKey): boolean {
  return Object.prototype.hasOwnProperty.call(object, key);
}
export type FormatSelection =
  Format | readonly Format[] | "retail" | "common1D" | "common" | "1D" | "2D" | "all";
export function resolveFormats(input?: readonly string[] | string): Format[] {
  if (input === "retail") return [...retailFormats];
  if (input === "common1D") return [...commonLinearFormats];
  if (input === "common") return [...commonFormats];
  if (input === "1D") return [...linearFormats];
  if (input === "2D") return [...matrixFormats];
  if (input === "all") return [...linearFormats, ...matrixFormats];
  if (input === undefined) return [...commonFormats];
  if (typeof input === "string") {
    if (hasKey(formatBits, input)) return [input as Format];
    throw new TypeError(`Unsupported barcode format or preset: ${input}`);
  }
  if (!Array.isArray(input) || input.length === 0)
    throw new TypeError("Choose at least one barcode format.");
  for (const value of input as readonly unknown[]) {
    if (typeof value !== "string") throw new TypeError("Barcode format must be a string.");
    if (!hasKey(formatBits, value)) throw new TypeError(`Unsupported barcode format: ${value}`);
  }
  return [...new Set(input)] as Format[];
}
/** Exact 64-bit mask as a number: format bits stay below 2^53, beyond 32-bit operators. */
export function maskFor(formats: readonly Format[]): number {
  let [low, high] = [0, 0];
  for (const format of formats) {
    low |= formatBits[format] % 2 ** 32;
    high |= Math.floor(formatBits[format] / 2 ** 32);
  }
  return (high >>> 0) * 2 ** 32 + (low >>> 0);
}

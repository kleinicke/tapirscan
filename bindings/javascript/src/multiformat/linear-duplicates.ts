import { polygonOverlap } from "./geometry.js";
import type { Image, Quad } from "../host.js";

type Read = {
  text: string;
  format: string;
  polygon: Quad;
  support: number;
  eanAddOn?: string;
  gs1?: boolean;
  readerInitialization?: boolean;
};
const formats = new Set(["EAN13", "UPCA", "EAN8", "UPCE", "Code128", "Code39", "ITF"]);
function bitCount(value: number): number {
  let bits = value - ((value >>> 1) & 0x55555555);
  bits = (bits & 0x33333333) + ((bits >>> 2) & 0x33333333);
  return (((bits + (bits >>> 4)) & 0x0f0f0f0f) * 0x01010101) >>> 24;
}
const midpoint = (a: readonly number[], b: readonly number[]): [number, number] => [
  (a[0] + b[0]) / 2,
  (a[1] + b[1]) / 2,
];
const line = (q: Quad) => [midpoint(q[0], q[3]), midpoint(q[1], q[2])] as const;

/** Only merge equal-value, aligned bands when source pixels connect their bars.
 * This does not change search coverage or retry budgets. Exhausting the small
 * evidence budget leaves detections separate, never drops an unchecked read.
 */
export function mergeLinearDuplicates<T extends Read>(reads: readonly T[], image: Image): T[] {
  if (reads.length < 2) return [...reads];
  const seen = new Set<string>();
  let repeated = false;
  for (const read of reads) {
    if (!read.text || !formats.has(read.format)) continue;
    const key = `${read.format}:${read.text}`;
    if (seen.has(key)) {
      repeated = true;
      break;
    }
    seen.add(key);
  }
  if (!repeated) return [...reads];
  let remaining = 32768;
  const result: T[] = [];
  let values: Float64Array | undefined;
  const profile = (left: readonly number[], right: readonly number[]) => {
    if (remaining < 64) return undefined;
    remaining -= 64;
    values ??= new Float64Array(64);
    let lo = 255,
      hi = 0;
    for (let i = 0; i < 64; i++) {
      const f = (i + 0.5) / 64;
      const x = Math.round(left[0] + (right[0] - left[0]) * f);
      const y = Math.round(left[1] + (right[1] - left[1]) * f);
      if (x < 0 || y < 0 || x >= image.width || y >= image.height) return undefined;
      const p = y * image.stride + x * image.channels;
      const value =
        image.channels === 1
          ? image.data[p]
          : (77 * image.data[p] + 150 * image.data[p + 1] + 29 * image.data[p + 2]) / 256;
      values[i] = value;
      lo = Math.min(lo, value);
      hi = Math.max(hi, value);
    }
    if (hi - lo < 24) return undefined;
    const threshold = (lo + hi) / 2;
    let first = 0,
      second = 0;
    for (let i = 0; i < 32; i++) {
      if (values[i] < threshold) first |= 1 << i;
      if (values[i + 32] < threshold) second |= 1 << i;
    }
    return [first, second] as const;
  };
  const connected = (a: Quad, inputB: Quad): Quad | undefined => {
    if (!a.every((p) => p.every(Number.isFinite)) || !inputB.every((p) => p.every(Number.isFinite)))
      return undefined;
    const al = line(a),
      bl = line(inputB);
    const ax = al[1][0] - al[0][0],
      ay = al[1][1] - al[0][1];
    let bx = bl[1][0] - bl[0][0],
      by = bl[1][1] - bl[0][1];
    const aw = Math.hypot(ax, ay),
      bw = Math.hypot(bx, by);
    if (aw < 24 || bw / aw < 0.9 || bw / aw > 1.1) return undefined;
    let b = inputB;
    if (ax * bx + ay * by < 0) {
      b = [inputB[2], inputB[3], inputB[0], inputB[1]];
      bx = -bx;
      by = -by;
    }
    if ((ax * bx + ay * by) / (aw * bw) < 0.996) return undefined;
    const ac = midpoint(al[0], al[1]),
      bline = line(b),
      bc = midpoint(bline[0], bline[1]);
    const dx = bc[0] - ac[0],
      dy = bc[1] - ac[1];
    if (Math.abs(dx * ax + dy * ay) / aw > aw * 0.06) return undefined;
    // Bound endpoint movement too: rotation/perspective can move an edge farther
    // than the centers. Every interpolated sample then moves at most one pixel.
    const distance = Math.max(
      Math.hypot(bline[0][0] - al[0][0], bline[0][1] - al[0][1]),
      Math.hypot(bline[1][0] - al[1][0], bline[1][1] - al[1][1]),
    );
    const steps = Math.ceil(distance);
    if (steps < 1 || steps > 384 || (steps + 2) * 64 > remaining) return undefined;
    const reference = profile(al[0], al[1]);
    if (!reference) return undefined;
    let dark0 = reference[0],
      dark1 = reference[1];
    let light0 = ~dark0,
      light1 = ~dark1;
    const minimumDark = Math.max(4, Math.ceil((bitCount(dark0) + bitCount(dark1)) / 4));
    const minimumLight = Math.max(4, Math.ceil((bitCount(light0) + bitCount(light1)) / 4));
    for (let step = 1; step <= steps; step++) {
      const f = step / steps;
      const l: [number, number] = [
        al[0][0] + (bline[0][0] - al[0][0]) * f,
        al[0][1] + (bline[0][1] - al[0][1]) * f,
      ];
      const r: [number, number] = [
        al[1][0] + (bline[1][0] - al[1][0]) * f,
        al[1][1] + (bline[1][1] - al[1][1]) * f,
      ];
      const sample = profile(l, r);
      if (!sample) return undefined;
      const disagreement = bitCount(reference[0] ^ sample[0]) + bitCount(reference[1] ^ sample[1]);
      if (disagreement > 12) return undefined;
      dark0 &= sample[0];
      dark1 &= sample[1];
      light0 &= ~sample[0];
      light1 &= ~sample[1];
      // A skewed separator may cross columns at different heights. Require
      // individual bars AND spaces to survive the entire bridge, not just rows.
      if (
        bitCount(dark0) + bitCount(dark1) < minimumDark ||
        bitCount(light0) + bitCount(light1) < minimumLight
      )
        return undefined;
    }
    const along = (p: readonly number[]) => (-ay * p[0] + ax * p[1]) / aw;
    const top = along(midpoint(a[0], a[1])) < along(midpoint(b[0], b[1])) ? a : b;
    const bottom = along(midpoint(a[2], a[3])) > along(midpoint(b[2], b[3])) ? a : b;
    return [top[0], top[1], bottom[2], bottom[3]];
  };
  for (const read of [...reads].sort((a, b) => b.support - a.support)) {
    if (!formats.has(read.format) || !read.text || remaining < 192) {
      result.push(read);
      continue;
    }
    let merged = false;
    for (let i = 0; i < result.length; i++) {
      const other = result[i];
      if (
        read.format !== other.format ||
        read.text !== other.text ||
        read.eanAddOn !== other.eanAddOn ||
        Boolean(read.gs1) !== Boolean(other.gs1) ||
        Boolean(read.readerInitialization) !== Boolean(other.readerInitialization)
      )
        continue;
      if (polygonOverlap(other.polygon, read.polygon).smaller >= 0.65) {
        merged = true;
        break;
      }
      const polygon = connected(other.polygon, read.polygon);
      if (!polygon) continue;
      result[i] = { ...other, polygon }; // Keep strongest support; observations may overlap.
      merged = true;
      break;
    }
    if (!merged) result.push(read);
  }
  return result;
}

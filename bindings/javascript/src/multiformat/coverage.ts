import type { Quad } from "../host.js";

/** Convex containment, including the boundary; reject degenerate coverage. */
export function containsPoint(point: readonly number[], quad: Quad): boolean {
  if (!point.every(Number.isFinite) || quad.some((p) => !p.every(Number.isFinite))) return false;
  let positive = false;
  let negative = false;
  let area = 0;
  for (let i = 0; i < 4; i++) {
    const a = quad[i],
      b = quad[(i + 1) % 4];
    const cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
    positive ||= cross > 1e-6;
    negative ||= cross < -1e-6;
    area += a[0] * b[1] - b[0] * a[1];
  }
  return Math.abs(area) > 1e-6 && !(positive && negative);
}

/** Only deep retries are deferred. Normal proposals and the full-frame bit remain. */
export function uncoveredRetryMask(
  proposals: readonly { polygon: Quad }[],
  coverage: readonly Quad[],
  initial: readonly number[] = [0xffffffff, 0xffffffff],
): [number, number] {
  const mask: [number, number] = [initial[0] >>> 0, initial[1] >>> 0];
  proposals.forEach((proposal, index) => {
    if (coverage.some((quad) => proposal.polygon.every((point) => containsPoint(point, quad)))) {
      mask[index >>> 5] = (mask[index >>> 5] & ~(1 << (index & 31))) >>> 0;
    }
  });
  return mask;
}

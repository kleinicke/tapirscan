import type { Quad } from "../host.js";

export type Transform = readonly number[];
export function project(t: Transform, x: number, y: number): [number, number] {
  const z = t[6] * x + t[7] * y + 1;
  return [(t[0] * x + t[1] * y + t[2]) / z, (t[3] * x + t[4] * y + t[5]) / z];
}
export function transformFor(quad: Quad, width: number, height: number): Transform {
  const source = [
    [0, 0],
    [width, 0],
    [width, height],
    [0, height],
  ];
  const a: number[][] = [];
  for (let i = 0; i < 4; i++) {
    const [x, y] = source[i],
      [u, v] = quad[i];
    a.push([x, y, 1, 0, 0, 0, -u * x, -u * y, u], [0, 0, 0, x, y, 1, -v * x, -v * y, v]);
  }
  for (let col = 0; col < 8; col++) {
    let pivot = col;
    for (let i = col + 1; i < 8; i++) if (Math.abs(a[i][col]) > Math.abs(a[pivot][col])) pivot = i;
    if (Math.abs(a[pivot][col]) < 1e-9) throw Error("Degenerate barcode region.");
    [a[col], a[pivot]] = [a[pivot], a[col]];
    const divisor = a[col][col];
    for (let j = col; j <= 8; j++) a[col][j] /= divisor;
    for (let i = 0; i < 8; i++) {
      if (i === col) continue;
      const scale = a[i][col];
      for (let j = col; j <= 8; j++) a[i][j] -= scale * a[col][j];
    }
  }
  return a.map((row) => row[8]);
}
export function rectify(gray: Uint8Array, width: number, height: number, quad: Quad) {
  const distance = (a: readonly number[], b: readonly number[]) =>
    Math.hypot(a[0] - b[0], a[1] - b[1]);
  const w = Math.max(
    8,
    Math.ceil(Math.max(distance(quad[0], quad[1]), distance(quad[3], quad[2]))),
  );
  const h = Math.max(
    8,
    Math.ceil(Math.max(distance(quad[0], quad[3]), distance(quad[1], quad[2]))),
  );
  if (w * h > 8 * 1024 * 1024) throw Error("Barcode region exceeds rectification budget.");
  const base = transformFor(quad, w, h);
  const pad = 8,
    divisor = 1 - pad * (base[6] + base[7]);
  const t = [
    base[0] / divisor,
    base[1] / divisor,
    (base[2] - pad * (base[0] + base[1])) / divisor,
    base[3] / divisor,
    base[4] / divisor,
    (base[5] - pad * (base[3] + base[4])) / divisor,
    base[6] / divisor,
    base[7] / divisor,
  ];
  const paddedWidth = w + 2 * pad,
    paddedHeight = h + 2 * pad;
  const data = new Uint8Array(paddedWidth * paddedHeight);
  for (let y = 0; y < paddedHeight; y++)
    for (let x = 0; x < paddedWidth; x++) {
      const [xx, yy] = project(t, x + 0.5, y + 0.5);
      if (xx < 0 || yy < 0 || xx > width - 1 || yy > height - 1) {
        data[y * paddedWidth + x] = 255;
        continue;
      }
      const x0 = Math.floor(xx),
        y0 = Math.floor(yy),
        fx = xx - x0,
        fy = yy - y0;
      const x1 = Math.min(width - 1, x0 + 1),
        y1 = Math.min(height - 1, y0 + 1);
      data[y * paddedWidth + x] = Math.round(
        (gray[y0 * width + x0] * (1 - fx) + gray[y0 * width + x1] * fx) * (1 - fy) +
          (gray[y1 * width + x0] * (1 - fx) + gray[y1 * width + x1] * fx) * fy,
      );
    }
  return { data, width: paddedWidth, height: paddedHeight, transform: t };
}

/** Convex intersection, including opposite winding and projective quadrilaterals. */
export function polygonOverlap(a: Quad, b: Quad) {
  const signedArea = (p: readonly (readonly number[])[]) =>
    p.reduce((sum, q, i) => {
      const r = p[(i + 1) % p.length];
      return sum + q[0] * r[1] - q[1] * r[0];
    }, 0) / 2;
  const aa = Math.abs(signedArea(a)),
    bb = Math.abs(signedArea(b));
  if (Math.min(aa, bb) < 1e-6) return { iou: 0, smaller: 0, a: 0, b: 0 };
  const winding = Math.sign(signedArea(b));
  let points: (readonly number[])[] = [...a];
  for (let i = 0; i < 4 && points.length; i++) {
    const p = b[i],
      q = b[(i + 1) % 4];
    const side = (r: readonly number[]) =>
      winding * ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]));
    const clipped: (readonly number[])[] = [];
    for (let j = 0; j < points.length; j++) {
      const u = points[j],
        v = points[(j + 1) % points.length],
        su = side(u),
        sv = side(v);
      if (su >= 0) clipped.push(u);
      if (su >= 0 !== sv >= 0) {
        const t = su / (su - sv);
        clipped.push([u[0] + t * (v[0] - u[0]), u[1] + t * (v[1] - u[1])]);
      }
    }
    points = clipped;
  }
  const intersection = Math.min(aa, bb, Math.abs(signedArea(points)));
  return {
    iou: intersection / (aa + bb - intersection),
    smaller: intersection / Math.min(aa, bb),
    a: intersection / aa,
    b: intersection / bb,
  };
}

/** Suppress overlapping reads of the same physical symbol, never by text alone. */
export function distinctReads<
  T extends {
    text: string;
    format: string;
    polygon: Quad;
    support: number;
    eanAddOn?: string;
    readerInitialization?: boolean;
    structuredAppend?: { index: number; count: number; id?: string; parity?: number };
  },
>(reads: readonly T[]): T[] {
  const result: T[] = [];
  for (const read of [...reads].sort((a, b) => b.support - a.support)) {
    if (
      !result.some(
        (r) =>
          r.text === read.text &&
          r.format === read.format &&
          r.eanAddOn === read.eanAddOn &&
          Boolean(r.readerInitialization) === Boolean(read.readerInitialization) &&
          r.structuredAppend?.index === read.structuredAppend?.index &&
          r.structuredAppend?.count === read.structuredAppend?.count &&
          r.structuredAppend?.id === read.structuredAppend?.id &&
          r.structuredAppend?.parity === read.structuredAppend?.parity &&
          polygonOverlap(r.polygon, read.polygon).smaller >= 0.65,
      )
    )
      result.push(read);
  }
  return result;
}

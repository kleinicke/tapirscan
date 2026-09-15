import { detailProposals } from "./detail-proposals-rich.mjs";
import { sourceEvidence } from "./source-evidence.mjs";
import { coveredByContinuousBars } from "./continuity.mjs";

function refinedAngle(image, seed) {
  const gray = (x, y) => {
    const i = (y * image.width + x) * 4;
    return (77 * image.data[i] + 150 * image.data[i + 1] + 29 * image.data[i + 2]) / 256;
  };
  let xx = 0,
    yy = 0,
    xy = 0,
    sx = 0,
    sy = 0,
    n = 0;
  for (
    let y = Math.max(1, Math.round(seed.y) - 12);
    y < Math.min(image.height - 1, seed.y + 12);
    y++
  )
    for (
      let x = Math.max(1, Math.round(seed.x) - 12);
      x < Math.min(image.width - 1, seed.x + 12);
      x++
    ) {
      const a = gray(x - 1, y - 1),
        b = gray(x, y - 1),
        c = gray(x + 1, y - 1),
        d = gray(x - 1, y),
        e = gray(x + 1, y),
        f = gray(x - 1, y + 1),
        g = gray(x, y + 1),
        h = gray(x + 1, y + 1);
      const gx = (3 * (c - a) + 10 * (e - d) + 3 * (h - f)) / 16,
        gy = (3 * (f - a) + 10 * (g - b) + 3 * (h - c)) / 16;
      xx += gx * gx;
      yy += gy * gy;
      xy += gx * gy;
      sx += gx;
      sy += gy;
      n++;
    }
  return 0.5 * Math.atan2(2 * (xy - (sx * sy) / n), xx - (sx * sx) / n - yy + (sy * sy) / n);
}

function contains(point, q) {
  let hit = false;
  for (let i = 0, j = q.length - 1; i < q.length; j = i++) {
    if (
      q[i][1] > point[1] !== q[j][1] > point[1] &&
      point[0] < ((q[j][0] - q[i][0]) * (point[1] - q[i][1])) / (q[j][1] - q[i][1]) + q[i][0]
    )
      hit = !hit;
  }
  return hit;
}

function covered(image, point, barcode) {
  return contains(point, barcode.polygon) || coveredByContinuousBars(image, point, barcode);
}

function sourceSpan(p) {
  const dx = p[1][0] - p[0][0],
    dy = p[1][1] - p[0][1];
  return (
    Math.abs(dx * (p[3][1] - p[0][1]) - dy * (p[3][0] - p[0][0])) /
    Math.max(1e-9, Math.hypot(dx, dy))
  );
}

/** Bounded source-guided hypotheses, without another localizer/shear search per crop. */
export function recoverDirectSeed(
  image,
  scanner,
  policy,
  baseline,
  budget = 64,
  factor = 3,
  sequential = false,
  scharr = false,
  minimumSpan = 0,
  maxDirections = Infinity,
) {
  const start = performance.now();
  const seeds = detailProposals(image, 2, 256, 4);
  const additions = [],
    attempts = [],
    proposals = [];
  const tile = new OffscreenCanvas(1, 1),
    up = new OffscreenCanvas(1, 1);
  const tc = tile.getContext("2d"),
    uc = up.getContext("2d", { willReadFrequently: true });
  for (const seed of seeds) {
    if (seed.score < seeds[0].score * 0.6) continue;
    if ([...baseline.scan.barcodes, ...additions].some((b) => covered(image, [seed.x, seed.y], b)))
      continue;
    const quads = [];
    const initialAngle = scharr ? refinedAngle(image, seed) : seed.angle;
    const directions = (scharr ? [0, -3, 3] : [0, -5, 5, -10, 10, -15, 15, -20, 20]).map(
      (offset) => initialAngle + (offset * Math.PI) / 180,
    );
    for (const angle of directions) {
      const c = Math.cos(angle),
        s = Math.sin(angle);
      const polygon = [
        [-96, -24],
        [96, -24],
        [96, 24],
        [-96, 24],
      ].map(([u, v]) => [seed.x + u * c - v * s, seed.y + u * s + v * c]);
      const evidence = sourceEvidence(image, polygon, true, 32);
      if (Math.max(evidence.transitions, evidence.localTransitions) >= 32) quads.push(polygon);
      if (quads.length >= maxDirections) break;
    }
    if (!quads.length) continue;
    const size = 256,
      w = Math.min(size, image.width),
      h = Math.min(size, image.height);
    const x = Math.max(0, Math.min(image.width - w, Math.round(seed.x - w / 2)));
    const y = Math.max(0, Math.min(image.height - h, Math.round(seed.y - h / 2)));
    const bytes = new Uint8ClampedArray(w * h * 4);
    for (let row = 0; row < h; row++) {
      const from = ((y + row) * image.width + x) * 4;
      bytes.set(image.data.subarray(from, from + w * 4), row * w * 4);
    }
    tile.width = w;
    tile.height = h;
    tc.putImageData(new ImageData(bytes, w, h), 0, 0);
    up.width = w * factor;
    up.height = h * factor;
    uc.imageSmoothingEnabled = true;
    uc.drawImage(tile, 0, 0, up.width, up.height);
    const pixels = uc.getImageData(0, 0, up.width, up.height);
    const batches = sequential ? quads.map((q) => [q]) : [quads];
    for (const batch of batches) {
      const result = scanner.scan(
        {
          data: new Uint8Array(pixels.data.buffer),
          width: up.width,
          height: up.height,
          channels: 4,
          stride: up.width * 4,
        },
        batch.map((q) => q.map(([a, b]) => [(a - x) * factor, (b - y) * factor])),
        {
          ...policy,
          maxRetryPathsPerCandidate: budget,
          maxRetryPathsPerFrame: budget * batch.length,
        },
      );
      const allReads = result.barcodes.map((b) => ({
        ...b,
        polygon: b.polygon.map(([a, b]) => [x + a / factor, y + b / factor]),
      }));
      const reads = allReads.filter((b) => sourceSpan(b.polygon) >= minimumSpan);
      const deferredReads = allReads.filter((b) => sourceSpan(b.polygon) < minimumSpan);
      for (const read of reads) {
        const center = read.polygon.reduce((a, p) => [a[0] + p[0] / 4, a[1] + p[1] / 4], [0, 0]);
        if (
          ![...baseline.scan.barcodes, ...additions].some(
            (old) => old.text === read.text && covered(image, center, old),
          )
        )
          additions.push(read);
      }
      const localized = batch.map((polygon) => ({ polygon, score: seed.score, text: "" }));
      proposals.push(...localized);
      attempts.push({
        x,
        y,
        w,
        h,
        factor,
        // Raw evidence remains in crop coordinates with this explicit transform.
        frame: result,
        reads,
        deferredReads,
        proposals: localized,
        unfinished: result.unfinished || deferredReads.length > 0,
      });
      if (sequential && reads.some((b) => covered(image, [seed.x, seed.y], b))) break;
    }
  }
  return {
    barcodes: [...baseline.scan.barcodes, ...additions],
    additions,
    attempts,
    proposals,
    extraMs: performance.now() - start,
    searchLimited: true,
  };
}

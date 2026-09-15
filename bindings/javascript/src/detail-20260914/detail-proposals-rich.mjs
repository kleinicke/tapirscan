/** Source-resolution directional texture proposals. No labels or decoded text as inputs. */
export function detailProposals(image, count = 4, tileSize = 320, sampleStep = 2) {
  const { data, width, height } = image;
  const size = 24,
    step = sampleStep,
    cells = [];
  const gray = (x, y) => {
    const i = (y * width + x) * 4;
    return (77 * data[i] + 150 * data[i + 1] + 29 * data[i + 2]) / 256;
  };
  for (let y = 1; y + size < height; y += size)
    for (let x = 1; x + size < width; x += size) {
      let xx = 0,
        yy = 0,
        xy = 0,
        sx = 0,
        sy = 0,
        n = 0;
      for (let j = y; j < y + size; j += step)
        for (let i = x; i < x + size; i += step) {
          const v = gray(i, j),
            gx = gray(i + 1, j) - v,
            gy = gray(i, j + 1) - v;
          xx += gx * gx;
          yy += gy * gy;
          xy += gx * gy;
          sx += gx;
          sy += gy;
          n++;
        }
      xx = Math.max(0, xx - (sx * sx) / n);
      yy = Math.max(0, yy - (sy * sy) / n);
      xy -= (sx * sy) / n;
      const energy = (xx + yy) / n,
        coherence = Math.sqrt((xx - yy) ** 2 + 4 * xy * xy) / (xx + yy + 1);
      const score = Math.sqrt(energy) * coherence * coherence;
      if (score > 5)
        cells.push({
          x: x + size / 2,
          y: y + size / 2,
          score,
          coherence,
          energy,
          angle: 0.5 * Math.atan2(2 * xy, xx - yy),
        });
    }
  cells.sort((a, b) => b.score - a.score);
  const selected = [];
  for (const c of cells) {
    if (selected.some((s) => Math.hypot(s.x - c.x, s.y - c.y) < 128)) continue;
    selected.push({
      ...c,
      left: Math.max(0, Math.min(width - tileSize, Math.round(c.x - tileSize / 2))),
      top: Math.max(0, Math.min(height - tileSize, Math.round(c.y - tileSize / 2))),
      width: Math.min(width, tileSize),
      height: Math.min(height, tileSize),
    });
    if (selected.length === count) break;
  }
  return selected;
}

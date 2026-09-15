/** A bounded physical continuity claim; equal text alone never establishes identity. */
export function coveredByContinuousBars(image, point, barcode) {
  const polygon = barcode.polygon;
  const left = [(polygon[0][0] + polygon[3][0]) / 2, (polygon[0][1] + polygon[3][1]) / 2];
  const right = [(polygon[1][0] + polygon[2][0]) / 2, (polygon[1][1] + polygon[2][1]) / 2];
  const dx = right[0] - left[0],
    dy = right[1] - left[1],
    width = Math.hypot(dx, dy);
  if (width < 45) return false;
  const nx = -dy / width,
    ny = dx / width;
  const along = ((point[0] - left[0]) * dx + (point[1] - left[1]) * dy) / (width * width);
  const offset = (point[0] - left[0]) * nx + (point[1] - left[1]) * ny;
  if (along < 0 || along > 1 || Math.abs(offset) > Math.min(128, width * 0.5)) return false;
  const { data, width: iw, height: ih } = image;
  const gray = (x, y) => {
    x = Math.max(0, Math.min(iw - 1, x));
    y = Math.max(0, Math.min(ih - 1, y));
    const x0 = Math.floor(x),
      y0 = Math.floor(y),
      x1 = Math.min(iw - 1, x0 + 1),
      y1 = Math.min(ih - 1, y0 + 1),
      fx = x - x0,
      fy = y - y0;
    const sample = (a, b) => {
      const k = (b * iw + a) * 4;
      return (77 * data[k] + 150 * data[k + 1] + 29 * data[k + 2]) / 256;
    };
    return (
      (sample(x0, y0) * (1 - fx) + sample(x1, y0) * fx) * (1 - fy) +
      (sample(x0, y1) * (1 - fx) + sample(x1, y1) * fx) * fy
    );
  };
  const count = Math.min(256, Math.max(95, Math.round(width * 1.5)));
  const profile = (displacement) => {
    const values = new Float64Array(count);
    let sum = 0,
      energy = 0;
    for (let i = 0; i < count; i++) {
      const fraction = 0.02 + (0.96 * i) / (count - 1);
      const v = gray(
        left[0] + dx * fraction + nx * displacement,
        left[1] + dy * fraction + ny * displacement,
      );
      values[i] = v;
      sum += v;
      energy += v * v;
    }
    return {
      values,
      mean: sum / count,
      deviation: Math.sqrt(Math.max(0, energy / count - (sum / count) ** 2)),
    };
  };
  const reference = profile(0);
  if (reference.deviation < 5) return false;
  const agrees = (displacement) => {
    const row = profile(displacement);
    if (row.deviation < reference.deviation * 0.65 || row.deviation < 5) return false;
    let covariance = 0;
    for (let i = 0; i < count; i++)
      covariance += (reference.values[i] - reference.mean) * (row.values[i] - row.mean);
    return covariance / (count * reference.deviation * row.deviation) > 0.8;
  };
  if (!agrees(offset) || !agrees(offset / 2)) return false;
  const steps = Math.ceil(Math.abs(offset) * 2);
  for (let i = 1; i < steps; i++) if (!agrees((offset * i) / steps)) return false;
  return true;
}

/** Cheap alternating-run evidence. It prioritizes geometry, never accepts digits. */
export function sourceEvidence(image, polygon, local = false, target = Infinity) {
  const { data, width, height } = image;
  const gray = (x, y) => {
    x = Math.max(0, Math.min(width - 1, x));
    y = Math.max(0, Math.min(height - 1, y));
    const x0 = Math.floor(x),
      y0 = Math.floor(y),
      x1 = Math.min(width - 1, x0 + 1),
      y1 = Math.min(height - 1, y0 + 1),
      fx = x - x0,
      fy = y - y0;
    const v = (a, b) => {
      const i = (b * width + a) * 4;
      return (77 * data[i] + 150 * data[i + 1] + 29 * data[i + 2]) / 256;
    };
    return (
      (v(x0, y0) * (1 - fx) + v(x1, y0) * fx) * (1 - fy) +
      (v(x0, y1) * (1 - fx) + v(x1, y1) * fx) * fy
    );
  };
  let best = 0,
    localBest = 0,
    rows = 0,
    contrast = 0;
  for (let axis = 0; axis < 2; axis++)
    for (const f of [0.2, 0.4, 0.6, 0.8]) {
      const a = polygon[axis],
        b = polygon[(axis + 1) % 4],
        c = polygon[(axis + 2) % 4],
        d = polygon[(axis + 3) % 4];
      const p = [a[0] * (1 - f) + d[0] * f, a[1] * (1 - f) + d[1] * f],
        q = [b[0] * (1 - f) + c[0] * f, b[1] * (1 - f) + c[1] * f];
      const n = Math.min(
        2048,
        Math.max(64, Math.ceil(Math.hypot(q[0] - p[0], q[1] - p[1]) * 1.25)),
      );
      const samples = new Float32Array(n);
      let lo = 255,
        hi = 0;
      for (let i = 0; i < n; i++) {
        const t = -0.125 + (1.25 * i) / (n - 1),
          v = gray(p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t);
        samples[i] = v;
        lo = Math.min(lo, v);
        hi = Math.max(hi, v);
      }
      contrast = Math.max(contrast, hi - lo);
      if (hi - lo < (local ? 12 : 20)) continue;
      const threshold = (lo + hi) / 2,
        hysteresis = (hi - lo) * 0.06;
      let state = samples[0] > threshold,
        runs = 0;
      for (const v of samples) {
        if (state ? v < threshold - hysteresis : v > threshold + hysteresis) {
          state = !state;
          runs++;
        }
      }
      best = Math.max(best, runs);
      if (runs >= 20) rows++;
      if (best >= target) return { transitions: best, localTransitions: localBest, rows, contrast };
      if (local) {
        const sum = new Float64Array(n + 1);
        for (let i = 0; i < n; i++) sum[i + 1] = sum[i] + samples[i];
        let previous,
          localRuns = 0;
        for (let i = 0; i < n; i++) {
          const left = Math.max(0, i - 8),
            right = Math.min(n, i + 9),
            mean = (sum[right] - sum[left]) / (right - left);
          const next = samples[i] > mean + 2 ? true : samples[i] < mean - 2 ? false : undefined;
          if (next !== undefined) {
            if (previous !== undefined && previous !== next) localRuns++;
            previous = next;
          }
        }
        localBest = Math.max(localBest, localRuns);
        if (localBest >= target)
          return { transitions: best, localTransitions: localBest, rows, contrast };
      }
    }
  return { transitions: best, localTransitions: localBest, rows, contrast };
}
export function evidenceFilter(image, localization, minimum = 20) {
  const ranked = localization.proposals.map((p) => ({
    ...p,
    evidence: sourceEvidence(image, p.polygon),
  }));
  const proposals = ranked.filter((p) => p.evidence.transitions >= minimum),
    deferred = ranked.filter((p) => p.evidence.transitions < minimum);
  return {
    ...localization,
    proposals,
    deferred,
    workLimited: localization.workLimited || deferred.length > 0,
  };
}

/** Retain all candidates. Evidence stops once eligibility is established; counts are lower bounds. */
export function evidencePlan(image, localization, minimum = 20) {
  const proposals = localization.proposals.map((p) => ({
    ...p,
    evidence: sourceEvidence(image, p.polygon, false, minimum),
  }));
  const masks = [0, 0];
  proposals.forEach((p, i) => {
    if (p.evidence.transitions >= minimum) masks[i >>> 5] |= 1 << (i & 31);
  });
  const fullFrame = proposals.length;
  if (fullFrame < 64) masks[fullFrame >>> 5] |= 1 << (fullFrame & 31);
  return { ...localization, proposals, retryMask: masks.map((x) => x >>> 0) };
}

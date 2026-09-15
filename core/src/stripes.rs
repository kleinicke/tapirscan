//! Bounded component-preserving Scharr localizer. TS037 reference port.
#![forbid(unsafe_code)]
use crate::{
    oriented::Proposal,
    sampling::{Error, ImageView},
};
#[derive(Clone, Default)]
struct Tile {
    xx: f64,
    xy: f64,
    yy: f64,
    angle: f64,
    active: bool,
}
#[derive(Clone, Copy)]
struct Bounds {
    u0: f64,
    u1: f64,
    v0: f64,
    v1: f64,
}
#[derive(Clone, Copy)]
struct Edge {
    x: f64,
    y: f64,
    weight: f64,
}
pub struct Result {
    pub proposals: Vec<Proposal>,
    pub omitted: usize,
    pub limited: bool,
    pub trace: [usize; 13],
}
// For finite Scharr gradients, max(|dx|,|dy|) <= hypot <= |dx|+|dy|.
// These one-sided bounds only reject edges the original tests must reject;
// retained weights still use exactly the original hypot computation.
fn edge_weight(dx: f64, dy: f64, ax: f64, ay: f64) -> Option<f64> {
    let (x, y) = (dx.abs(), dy.abs());
    if x + y <= 18. {
        return None;
    }
    let projection = (dx * ax + dy * ay).abs();
    if projection <= 0.85 * x.max(y) {
        return None;
    }
    let weight = dx.hypot(dy);
    (weight > 18. && projection > 0.85 * weight).then_some(weight)
}
fn distance(a: f64, b: f64) -> f64 {
    // Undirected stripe axes repeat every pi. Avoid sin/cos/atan2 in the
    // per-neighbor component and hysteresis searches. Angles come from finite
    // structure tensors; modulo also handles callers outside atan2's range.
    let d = (a - b).abs() % std::f64::consts::PI;
    d.min(std::f64::consts::PI - d)
}
fn angle(ids: &[usize], tiles: &[Tile]) -> f64 {
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    for &k in ids {
        xx += tiles[k].xx;
        xy += tiles[k].xy;
        yy += tiles[k].yy;
    }
    0.5 * (2. * xy).atan2(xx - yy)
}
fn bounds(ids: &[usize], angle: f64, tw: usize) -> Bounds {
    let (c, s) = (angle.cos(), angle.sin());
    let mut b = Bounds {
        u0: f64::INFINITY,
        u1: f64::NEG_INFINITY,
        v0: f64::INFINITY,
        v1: f64::NEG_INFINITY,
    };
    for &k in ids {
        let (x, y) = ((k % tw * 8) as f64, (k / tw * 8) as f64);
        for (px, py) in [(x, y), (x + 8., y), (x, y + 8.), (x + 8., y + 8.)] {
            let (u, v) = (px * c + py * s, -px * s + py * c);
            b.u0 = b.u0.min(u);
            b.u1 = b.u1.max(u);
            b.v0 = b.v0.min(v);
            b.v1 = b.v1.max(v);
        }
    }
    b
}
/// Diagnostic geometry uses the localizer's working raster, not source pixels.
#[derive(Debug)]
pub struct GroupDiagnostic {
    pub index: usize,
    pub tiles: usize,
    pub edges: usize,
    pub angle: f64,
    pub initial_angle: f64,
    pub bounds: [f64; 4],
    pub reason: &'static str,
}
pub fn detect(im: ImageView<'_>) -> std::result::Result<Result, Error> {
    detect_with_observer(im, |_| {})
}
/// Read-only group trace; the observer never influences scanner decisions.
pub fn detect_with_observer(
    im: ImageView<'_>,
    observe: impl FnMut(GroupDiagnostic),
) -> std::result::Result<Result, Error> {
    detect_grid(im, 768., false, observe)
}
/// Pixel-only supplementary grid. The original detector remains the first pass.
pub(crate) fn detect_secondary(im: ImageView<'_>) -> std::result::Result<Result, Error> {
    detect_grid(im, 640., true, |_| {})
}
fn detect_grid(
    im: ImageView<'_>,
    working_dimension: f64,
    bilinear: bool,
    mut observe: impl FnMut(GroupDiagnostic),
) -> std::result::Result<Result, Error> {
    if im.width < 3 || im.height < 3 {
        return Err(Error::Dimensions);
    }
    let scale = (working_dimension / im.width.max(im.height) as f64).min(1.);
    let (w, h) = (
        ((im.width as f64 * scale).round() as usize).max(3),
        ((im.height as f64 * scale).round() as usize).max(3),
    );
    let mut gray = vec![0f32; w * h];
    let mut gx = vec![0f32; w * h];
    let mut gy = vec![0f32; w * h];
    if bilinear {
        // Diagnostic antialiased working-raster hypothesis; decoder source pixels
        // stay untouched. Same legacy RGB weights, bilinear center sampling.
        let luminance = |x: usize, y: usize| {
            let i = y * im.stride + x * im.channels;
            if im.channels == 1 {
                f64::from(im.data[i])
            } else {
                (77. * f64::from(im.data[i])
                    + 150. * f64::from(im.data[i + 1])
                    + 29. * f64::from(im.data[i + 2]))
                    / 256.
            }
        };
        for y in 0..h {
            let sy = ((y as f64 + 0.5) * im.height as f64 / h as f64 - 0.5)
                .clamp(0., (im.height - 1) as f64);
            let y0 = sy.floor() as usize;
            let y1 = (y0 + 1).min(im.height - 1);
            let fy = sy - y0 as f64;
            for x in 0..w {
                let sx = ((x as f64 + 0.5) * im.width as f64 / w as f64 - 0.5)
                    .clamp(0., (im.width - 1) as f64);
                let x0 = sx.floor() as usize;
                let x1 = (x0 + 1).min(im.width - 1);
                let fx = sx - x0 as f64;
                let a = luminance(x0, y0);
                let b = luminance(x1, y0);
                let c = luminance(x0, y1);
                let d = luminance(x1, y1);
                let u = a + (b - a) * fx;
                let v = c + (d - c) * fx;
                gray[y * w + x] = (u + (v - u) * fy) as f32;
            }
        }
    } else {
        // Source-column mapping is invariant across all output rows. Integer RGB
        // weights are exactly the legacy binary-fraction luminance, not a new filter.
        let columns: Vec<_> = (0..w)
            .map(|x| {
                (((x as f64 + 0.5) * im.width as f64 / w as f64).floor() as usize).min(im.width - 1)
                    * im.channels
            })
            .collect();
        for y in 0..h {
            let sy = (((y as f64 + 0.5) * im.height as f64 / h as f64).floor() as usize)
                .min(im.height - 1);
            let row = &im.data[sy * im.stride..];
            let dst = &mut gray[y * w..(y + 1) * w];
            if im.channels == 1 {
                for (x, &i) in columns.iter().enumerate() {
                    dst[x] = f32::from(row[i]);
                }
            } else {
                for (x, &i) in columns.iter().enumerate() {
                    dst[x] = (77 * u32::from(row[i])
                        + 150 * u32::from(row[i + 1])
                        + 29 * u32::from(row[i + 2])) as f32
                        / 256.;
                }
            }
        }
    }
    // Only the <=768px working gray raster is conditioned. Source RGB stays
    // untouched; the decoder samples original pixels with its own contrast gates.
    let mut histogram = [0usize; 256];
    for &v in &gray {
        histogram[v.clamp(0., 255.).floor() as usize] += 1;
    }
    let quantile = |target: usize| {
        let mut sum = 0;
        for (i, &n) in histogram.iter().enumerate() {
            sum += n;
            if sum > target {
                return i as f32;
            }
        }
        255.
    };
    let lo = quantile(gray.len() * 2 / 100);
    let hi = quantile(gray.len() * 98 / 100);
    let span = hi - lo;
    let conditioned = (8.0..96.0).contains(&span);
    if conditioned {
        let gain = (192. / span).min(8.);
        for v in &mut gray {
            *v = ((*v - lo) * gain).clamp(0., 255.);
        }
    }
    let (tw, th) = (w.div_ceil(8), h.div_ceil(8));
    let mut tiles = vec![Tile::default(); tw * th];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            // Unconditioned samples are exact multiples of 1/256. Every Scharr
            // intermediate fits within 20 significant bits, so f32 is exact.
            // Conditioned low-light samples retain the original f64 arithmetic.
            let (dx, dy) = if !conditioned && !bilinear {
                let g = |i: usize| gray[i];
                (
                    f64::from(
                        (3. * (g(i - w + 1) - g(i - w - 1))
                            + 10. * (g(i + 1) - g(i - 1))
                            + 3. * (g(i + w + 1) - g(i + w - 1)))
                            / 16.,
                    ),
                    f64::from(
                        (3. * (g(i + w - 1) - g(i - w - 1))
                            + 10. * (g(i + w) - g(i - w))
                            + 3. * (g(i + w + 1) - g(i - w + 1)))
                            / 16.,
                    ),
                )
            } else {
                let g = |i: usize| f64::from(gray[i]);
                let dx = (3. * (g(i - w + 1) - g(i - w - 1))
                    + 10. * (g(i + 1) - g(i - 1))
                    + 3. * (g(i + w + 1) - g(i + w - 1)))
                    / 16.;
                let dy = (3. * (g(i + w - 1) - g(i - w - 1))
                    + 10. * (g(i + w) - g(i - w))
                    + 3. * (g(i + w + 1) - g(i - w + 1)))
                    / 16.;
                (dx, dy)
            };
            gx[i] = dx as f32;
            gy[i] = dy as f32;
            let t = &mut tiles[y / 8 * tw + x / 8];
            t.xx += dx * dx;
            t.xy += dx * dy;
            t.yy += dy * dy;
        }
    }
    for t in &mut tiles {
        let energy = t.xx + t.yy;
        t.angle = 0.5 * (2. * t.xy).atan2(t.xx - t.yy);
        t.active = energy > 64. * 80. && (t.xx - t.yy).hypot(2. * t.xy) / (energy + 1.) > 0.60;
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut seen = vec![false; tiles.len()];
    for seed in 0..tiles.len() {
        if seen[seed] || !tiles[seed].active {
            continue;
        }
        let mut q = vec![seed];
        seen[seed] = true;
        let mut head = 0;
        while head < q.len() {
            let k = q[head];
            head += 1;
            let (tx, ty) = ((k % tw) as isize, (k / tw) as isize);
            for oy in -1..=1 {
                for ox in -1..=1 {
                    let (nx, ny) = (tx + ox, ty + oy);
                    if nx < 0 || ny < 0 || nx >= tw as isize || ny >= th as isize {
                        continue;
                    }
                    let n = ny as usize * tw + nx as usize;
                    if !seen[n]
                        && tiles[n].active
                        && distance(tiles[k].angle, tiles[n].angle) < std::f64::consts::PI / 9.
                    {
                        seen[n] = true;
                        q.push(n);
                    }
                }
            }
        }
        if q.len() >= 3 {
            groups.push(q);
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let eligible = groups.len().min(64);
    let angles: Vec<_> = groups
        .iter()
        .take(eligible)
        .map(|g| angle(g, &tiles))
        .collect();
    let mut used = vec![false; eligible];
    let mut merged = Vec::new();
    let mut merge_checks = 0;
    for seed in 0..eligible {
        if used[seed] {
            continue;
        }
        let mut members = vec![seed];
        used[seed] = true;
        let mut cache = vec![None; eligible];
        let axis = angles[seed];
        let mut changed = true;
        while changed && members.len() < 8 {
            changed = false;
            for i in 0..eligible {
                if used[i] {
                    continue;
                }
                let b = *cache[i].get_or_insert_with(|| bounds(&groups[i], axis, tw));
                let (mut close, mut compatible) = (false, true);
                for &j in &members {
                    merge_checks += 1;
                    let a = *cache[j].get_or_insert_with(|| bounds(&groups[j], axis, tw));
                    let (ha, hb) = (a.v1 - a.v0, b.v1 - b.v0);
                    let overlap = a.v1.min(b.v1) - a.v0.max(b.v0);
                    let gap = (a.u0 - b.u1).max(b.u0 - a.u1).max(0.);
                    if distance(angles[i], angles[j]) > std::f64::consts::PI / 18.
                        || ha.min(hb) / ha.max(hb) < 0.65
                        || overlap < 0.8 * ha.min(hb)
                    {
                        compatible = false;
                        break;
                    }
                    close |= gap <= 16.;
                }
                if compatible && close {
                    members.push(i);
                    used[i] = true;
                    changed = true;
                    if members.len() == 8 {
                        break;
                    }
                }
            }
        }
        if members.len() > 1 {
            merged.push(
                members
                    .iter()
                    .flat_map(|&i| groups[i].iter().copied())
                    .collect::<Vec<_>>(),
            );
        }
    }
    let originals = groups.len();
    let merged_count = merged.len();
    groups.extend(merged);
    let base_count = groups.len();
    let mut growth_limited = false;
    if cfg!(feature = "experimental-stripe-hysteresis") {
        let mut grown = Vec::new();
        for seed in groups.iter().take(originals.min(64)) {
            if !(3..=24).contains(&seed.len()) {
                continue;
            }
            let axis = angle(seed, &tiles);
            let b = bounds(seed, axis, tw);
            let (c, s) = (axis.cos(), axis.sin());
            let mut ids = seed.clone();
            let mut frontier = seed.clone();
            // Fixed seed direction and bar-height envelope prevent indirect
            // angle/height bridges. Three tile rings, at most64 tiles per seed.
            for _ in 0..3 {
                let mut next = Vec::new();
                for &k in &frontier {
                    let (tx, ty) = ((k % tw) as isize, (k / tw) as isize);
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (x, y) = (tx + dx, ty + dy);
                            if x < 0 || y < 0 || x >= tw as isize || y >= th as isize {
                                continue;
                            }
                            let n = y as usize * tw + x as usize;
                            if ids.contains(&n) || ids.len() >= 64 {
                                continue;
                            }
                            let t = &tiles[n];
                            let energy = t.xx + t.yy;
                            if energy <= 64. * 80.
                                || (t.xx - t.yy).hypot(2. * t.xy) / (energy + 1.) < 0.4
                                || distance(t.angle, axis) > std::f64::consts::PI / 9.
                            {
                                continue;
                            }
                            let (px, py) = (x as f64 * 8. + 4., y as f64 * 8. + 4.);
                            let (u, v) = (px * c + py * s, -px * s + py * c);
                            if u < b.u0 - 24. || u > b.u1 + 24. || v < b.v0 || v > b.v1 {
                                continue;
                            }
                            ids.push(n);
                            next.push(n);
                        }
                    }
                }
                frontier = next;
                if frontier.is_empty() {
                    break;
                }
            }
            growth_limited |= ids.len() >= 64;
            if ids.len() > seed.len() {
                grown.push(ids);
            }
        }
        groups.extend(grown);
    }
    let unrefined = groups.len().saturating_sub(128);
    let refined = groups.len().min(128);
    let mut proposals = Vec::new();
    let mut bands = Vec::new();
    let mut rescued_bands = Vec::new();
    let mut angle_bands = Vec::new();
    let (mut band_refined, mut band_unexamined) = (0, 0);
    let mut extra_proposals = Vec::new();
    let mut extra_bands = Vec::new();
    for (group_index, queue) in groups.iter().take(128).enumerate() {
        let (mut xx, mut xy, mut yy) = (0., 0., 0.);
        for &k in queue {
            xx += tiles[k].xx;
            xy += tiles[k].xy;
            yy += tiles[k].yy;
        }
        let mut angle = 0.5 * (2. * xy).atan2(xx - yy);
        let (ax, ay) = (angle.cos(), angle.sin());
        let mut edges = Vec::new();
        for &k in queue {
            let (tx, ty) = (k % tw, k / tw);
            for y in (ty * 8).max(1)..((ty + 1) * 8).min(h - 1) {
                for x in (tx * 8).max(1)..((tx + 1) * 8).min(w - 1) {
                    let i = y * w + x;
                    let (dx, dy) = (f64::from(gx[i]), f64::from(gy[i]));
                    if let Some(weight) = edge_weight(dx, dy, ax, ay) {
                        edges.push(Edge {
                            x: x as f64 + 0.5,
                            y: y as f64 + 0.5,
                            weight,
                        });
                    }
                }
            }
        }
        if edges.len() < 80 {
            let b = bounds(queue, angle, tw);
            observe(GroupDiagnostic {
                index: group_index,
                tiles: queue.len(),
                edges: edges.len(),
                angle,
                initial_angle: angle,
                bounds: [b.u0, b.u1, b.v0, b.v1],
                reason: "few_edges",
            });
            continue;
        }
        let mut best = f64::NEG_INFINITY;
        let initial = angle;
        let offset = (w as f64).hypot(h as f64);
        let mut bins = vec![0f32; (offset * 4.).ceil() as usize + 8];
        let (mut clear_start, mut clear_end) = (0, 0);
        for step in -10..=10 {
            let a = initial + f64::from(step) * std::f64::consts::PI / 360.;
            let (c, s) = (a.cos(), a.sin());
            bins[clear_start..clear_end].fill(0.);
            let (mut lo, mut hi) = (bins.len(), 0);
            for e in &edges {
                let p = (e.x * c + e.y * s + offset) * 2.;
                let b = p.floor() as isize;
                let f = p - b as f64;
                if b >= 0 && (b as usize + 1) < bins.len() {
                    let b = b as usize;
                    lo = lo.min(b);
                    hi = hi.max(b + 2);
                    bins[b] = (f64::from(bins[b]) + e.weight * (1. - f)) as f32;
                    bins[b + 1] = (f64::from(bins[b + 1]) + e.weight * f) as f32;
                }
            }
            clear_start = lo;
            clear_end = hi;
            let mut score = 0.;
            for &b in &bins[lo..hi] {
                score += f64::from(b) * f64::from(b);
            }
            if score > best {
                best = score;
                angle = a;
            }
        }
        let (c, s) = (angle.cos(), angle.sin());
        let (mut u0, mut u1, mut v0, mut v1) = (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        );
        for e in &edges {
            let (u, v) = (e.x * c + e.y * s, -e.x * s + e.y * c);
            u0 = u0.min(u);
            u1 = u1.max(u);
            v0 = v0.min(v);
            v1 = v1.max(v);
        }
        let (across, along) = (u1 - u0, v1 - v0);
        observe(GroupDiagnostic {
            index: group_index,
            tiles: queue.len(),
            edges: edges.len(),
            angle,
            initial_angle: initial,
            bounds: [u0, u1, v0, v1],
            reason: if across < 45. {
                "narrow"
            } else if along < 7. {
                "short"
            } else if across / along < 0.65 {
                "tall"
            } else if across / along > 30. {
                "flat"
            } else {
                "accepted_parent"
            },
        });
        let valid_parent = across >= 45. && along >= 7. && (0.65..=30.).contains(&(across / along));
        let rescue_tall = cfg!(feature = "experimental-stripe-tall-bands")
            && group_index < base_count
            && across >= 45.
            && along >= 24.
            && across / along < 0.65;
        if !valid_parent && !rescue_tall {
            continue;
        }
        // A table/label bridge can make the parent much taller than the barcode.
        // Retain the parent; append bounded cross-axis dense-edge bands later.
        if cfg!(feature = "experimental-stripe-bands") && along >= 24. {
            let n = along.ceil() as usize + 1;
            let mut density = vec![0usize; n];
            for e in &edges {
                let v = (-e.x * s + e.y * c - v0).floor() as usize;
                if v < n {
                    density[v] += 1;
                }
            }
            let smooth: Vec<f64> = (0..n)
                .map(|i| {
                    let lo = i.saturating_sub(2);
                    let hi = (i + 3).min(n);
                    density[lo..hi].iter().sum::<usize>() as f64 / (hi - lo) as f64
                })
                .collect();
            let peak = smooth.iter().copied().fold(0f64, f64::max);
            let threshold = (peak * 0.55).max(12.);
            let mut start = 0;
            let mut admitted = 0;
            while start < n && admitted < 2 {
                if smooth[start] < threshold {
                    start += 1;
                    continue;
                }
                let mut end = start + 1;
                while end < n && smooth[end] >= threshold {
                    end += 1;
                }
                if end - start >= 7 && (end - start) as f64 <= along * 0.55 {
                    let (bv0, bv1) = (v0 + start as f64, v0 + end as f64);
                    let (mut bu0, mut bu1) = (f64::INFINITY, f64::NEG_INFINITY);
                    for e in &edges {
                        let v = -e.x * s + e.y * c;
                        if v >= bv0 && v <= bv1 {
                            let u = e.x * c + e.y * s;
                            bu0 = bu0.min(u);
                            bu1 = bu1.max(u);
                        }
                    }
                    let band_ratio = (bu1 - bu0) / (bv1 - bv0);
                    if bu1 - bu0 >= 45. && (!rescue_tall || (0.65..=30.).contains(&band_ratio)) {
                        let point = |u: f64, v: f64| {
                            [
                                (u * c - v * s) * im.width as f64 / w as f64,
                                (u * s + v * c) * im.height as f64 / h as f64,
                            ]
                        };
                        (if rescue_tall {
                            &mut rescued_bands
                        } else if group_index < base_count {
                            &mut bands
                        } else {
                            &mut extra_bands
                        })
                        .push(Proposal {
                            polygon: [
                                point(bu0 - 1., bv0),
                                point(bu1 + 1., bv0),
                                point(bu1 + 1., bv1),
                                point(bu0 - 1., bv1),
                            ],
                            score: 0.99f64.min((xx - yy).hypot(2. * xy) / (xx + yy + 1.)),
                        });
                        if cfg!(feature = "experimental-band-local-angle")
                            && group_index < base_count
                        {
                            if band_refined < 8 {
                                band_refined += 1;
                                if let Some(p) = refine_band_angle(
                                    &edges,
                                    &gx,
                                    &gy,
                                    w,
                                    angle,
                                    [bu0, bu1, bv0, bv1],
                                    im.width as f64 / w as f64,
                                    im.height as f64 / h as f64,
                                ) {
                                    angle_bands.push(p);
                                }
                            } else {
                                band_unexamined += 1;
                            }
                        }
                        admitted += 1;
                    }
                }
                start = end;
            }
        }
        // A rejected parent is never admitted merely because one band is useful.
        if !valid_parent {
            continue;
        }
        u0 -= 1.;
        u1 += 1.;
        v0 += 1f64.min(along * 0.05);
        v1 -= 1f64.min(along * 0.05);
        let point = |u: f64, v: f64| {
            [
                (u * c - v * s) * im.width as f64 / w as f64,
                (u * s + v * c) * im.height as f64 / h as f64,
            ]
        };
        (if group_index < base_count {
            &mut proposals
        } else {
            &mut extra_proposals
        })
        .push(Proposal {
            polygon: [point(u0, v0), point(u1, v0), point(u1, v1), point(u0, v1)],
            score: 0.99f64.min((xx - yy).hypot(2. * xy) / (xx + yy + 1.)),
        });
    }
    proposals.extend(bands);
    // Preserve the order of previously admitted parents and bands. New bands
    // use the same proposal cap and explicit omitted-work accounting.
    proposals.extend(rescued_bands);
    let original_proposals = proposals.clone();
    proposals.extend(extra_proposals);
    proposals.extend(extra_bands);
    let mut extent_omitted = 0;
    let mut extent_unexamined = 0;
    let (mut extent_eligible, mut extent_examined, mut extent_ineligible) = (0, 0, 0);
    if cfg!(feature = "experimental-stripe-extent") {
        let (scheduled, eligible, ineligible) = extent_plan(
            &proposals,
            cfg!(feature = "experimental-extent-eligible-budget"),
        );
        extent_eligible = eligible;
        extent_examined = scheduled.len();
        extent_ineligible = ineligible;
        extent_unexamined = if cfg!(feature = "experimental-extent-eligible-budget") {
            eligible - scheduled.len()
        } else {
            proposals.len().saturating_sub(24)
        };
        let mut extended = Vec::new();
        for index in scheduled {
            let p = &proposals[index];
            if let Some(q) = source_extent(im, p) {
                if extended.len() < 8 {
                    extended.push(q);
                } else {
                    extent_omitted += 1;
                }
            }
        }
        // All original041 regions stay first. Additional growth proposals must
        // earn source-axis extension evidence before admission to the decoder.
        proposals = original_proposals;
        proposals.extend(extended);
    }
    proposals.extend(angle_bands);
    let mut fragment_omitted = 0;
    if cfg!(feature = "experimental-post-band-fragments") {
        let n = proposals.len().min(24);
        let mut joined = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                if let Some(q) = join_fragments(&proposals[i], &proposals[j], 16. / scale) {
                    if cfg!(feature = "experimental-fragment-coverage")
                        && proposals
                            .iter()
                            .take(24)
                            .any(|old| covers_fragment(old, &q, 2. / scale))
                    {
                        continue;
                    }
                    if joined.len() < 8 {
                        joined.push(q);
                    } else {
                        fragment_omitted += 1;
                    }
                }
            }
        }
        proposals.extend(joined);
    }
    if cfg!(feature = "experimental-source-angle") {
        // Direction correction does not require an additional extent change.
        // Keep established extent refinements first, then supplement angle-only
        // hypotheses under the same explicit proposal cap.
        let parent_count = proposals.len().min(24);
        #[cfg(feature = "experimental-angle-replace")]
        let (refinements, angle_only) =
            source_refinements_replacing(im, &mut proposals[..parent_count]);
        #[cfg(not(feature = "experimental-angle-replace"))]
        let (refinements, angle_only) = source_refinements(im, &proposals[..parent_count]);
        fragment_omitted +=
            refinements.len().saturating_sub(8) + angle_only.len().saturating_sub(8);
        proposals.extend(refinements.into_iter().take(8));
        proposals.extend(angle_only.into_iter().take(8));
    }
    let valid = proposals.len();
    let omitted = valid.saturating_sub(24)
        + fragment_omitted
        + unrefined
        + extent_omitted
        + extent_unexamined
        + band_unexamined;
    proposals.truncate(24);
    Ok(Result {
        proposals,
        omitted,
        limited: omitted > 0 || originals > 64 || growth_limited,
        trace: [
            originals,
            eligible,
            merged_count,
            merge_checks,
            refined,
            unrefined,
            valid,
            extent_unexamined,
            extent_eligible,
            extent_examined,
            extent_ineligible,
            band_refined,
            band_unexamined,
        ],
    })
}

// Reject only dimensions safely outside the downstream extent interval.
// Rotation preserves lengths; a coordinate-scaled slack retains rounding-edge
// cases for the unchanged post-rotation check.
#[cfg(test)]
fn source_extent_possible(p: &Proposal) -> bool {
    let q = p.polygon;
    let w = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
    let h = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
    let scale = q.iter().flatten().fold(1f64, |a, &v| a.max(v.abs()));
    let slack = scale * 1e-10;
    w >= 76. - slack && w <= 400. + slack && h >= 12. - slack && h <= 200. + slack
}
#[cfg(test)]
mod source_extent_precheck_tests {
    use super::*;
    #[test]
    fn retains_rotation_boundary_cases() {
        for w in [76., 400.] {
            for h in [12., 200.] {
                for origin in [0., 10000., 1_000_000.] {
                    for degree in 0..360 {
                        let a = f64::from(degree).to_radians();
                        let (c, s) = (a.cos(), a.sin());
                        let p = Proposal {
                            polygon: [[0., 0.], [w, 0.], [w, h], [0., h]]
                                .map(|[x, y]| [origin + c * x - s * y, origin + s * x + c * y]),
                            score: 1.,
                        };
                        assert!(source_extent_possible(&p));
                    }
                }
            }
        }
    }
    #[test]
    fn rejects_clearly_ineligible_dimensions() {
        for (w, h) in [(40., 50.), (800., 50.), (200., 5.), (200., 300.)] {
            let p = Proposal {
                polygon: [[0., 0.], [w, 0.], [w, h], [0., h]],
                score: 1.,
            };
            assert!(!source_extent_possible(&p));
        }
    }
}

// Bounded native-pixel tensor inside admitted quads. Supplement only: prior
// candidates retain order, dimensions and all-visible decode attempts.
#[cfg(any(test, not(feature = "experimental-angle-replace")))]
fn source_refinements(im: ImageView<'_>, parents: &[Proposal]) -> (Vec<Proposal>, Vec<Proposal>) {
    let (mut extents, mut angles) = (Vec::new(), Vec::new());
    for p in parents {
        if let Some(q) = source_angle(im, p) {
            if let Some(grown) = source_extent(im, &q) {
                extents.push(grown);
            } else {
                angles.push(q);
            }
        }
    }
    (extents, angles)
}
// Research ablation only: a highly coherent, modest correction can stand in
// for its angle-only parent. Extent growth remains supplemental, unchanged.
#[cfg(feature = "experimental-angle-replace")]
fn angle_replacement_qualified(parent: &Proposal, q: &Proposal) -> bool {
    let direction =
        |p: &Proposal| (p.polygon[1][1] - p.polygon[0][1]).atan2(p.polygon[1][0] - p.polygon[0][0]);
    q.score >= 0.9 && distance(direction(parent), direction(q)) <= 10f64.to_radians() + 1e-12
}
#[cfg(feature = "experimental-angle-replace")]
fn admit_source_refinement(
    parent: &mut Proposal,
    q: Proposal,
    grown: Option<Proposal>,
    extents: &mut Vec<Proposal>,
    angles: &mut Vec<Proposal>,
) {
    if let Some(grown) = grown {
        extents.push(grown);
    } else if angle_replacement_qualified(parent, &q) {
        *parent = q;
    } else {
        angles.push(q);
    }
}
#[cfg(feature = "experimental-angle-replace")]
fn source_refinements_replacing(
    im: ImageView<'_>,
    parents: &mut [Proposal],
) -> (Vec<Proposal>, Vec<Proposal>) {
    let (mut extents, mut angles) = (Vec::new(), Vec::new());
    for p in parents {
        // One source tensor, then one extent check for this parent, as in control.
        if let Some(q) = source_angle(im, p) {
            let grown = source_extent(im, &q);
            admit_source_refinement(p, q, grown, &mut extents, &mut angles);
        }
    }
    (extents, angles)
}
fn source_angle(im: ImageView<'_>, p: &Proposal) -> Option<Proposal> {
    let q = p.polygon;
    let ex = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
    let ey = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
    let w = ex[0].hypot(ex[1]);
    let h = ey[0].hypot(ey[1]);
    if w < 76. || h < 12. {
        return None;
    }
    let gray = |x: usize, y: usize| {
        let i = y * im.stride + x * im.channels;
        if im.channels == 1 {
            f64::from(im.data[i])
        } else {
            (77. * f64::from(im.data[i])
                + 150. * f64::from(im.data[i + 1])
                + 29. * f64::from(im.data[i + 2]))
                / 256.
        }
    };
    let (mut xx, mut xy, mut yy, mut count) = (0., 0., 0., 0);
    for j in 0..32 {
        for i in 0..64 {
            let u = (f64::from(i) + 0.5) / 64.;
            let v = (f64::from(j) + 0.5) / 32.;
            let x = (q[0][0] + ex[0] * u + ey[0] * v).floor();
            let y = (q[0][1] + ex[1] * u + ey[1] * v).floor();
            if x < 1. || y < 1. || x >= (im.width - 1) as f64 || y >= (im.height - 1) as f64 {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            let dx = gray(x + 1, y) - gray(x - 1, y);
            let dy = gray(x, y + 1) - gray(x, y - 1);
            if dx.hypot(dy) < 18. {
                continue;
            }
            xx += dx * dx;
            xy += dx * dy;
            yy += dy * dy;
            count += 1;
        }
    }
    let coherence = (xx - yy).hypot(2. * xy) / (xx + yy + 1.);
    if count < 80 || coherence < 0.6 {
        return None;
    }
    let a = 0.5 * (2. * xy).atan2(xx - yy);
    let parent = ex[1].atan2(ex[0]);
    let delta = (a - parent + std::f64::consts::PI / 2.).rem_euclid(std::f64::consts::PI)
        - std::f64::consts::PI / 2.;
    if delta.abs() < 0.5f64.to_radians() || delta.abs() > 30f64.to_radians() {
        return None;
    }
    let center = [(q[0][0] + q[2][0]) * 0.5, (q[0][1] + q[2][1]) * 0.5];
    let (c, s) = (delta.cos(), delta.sin());
    Some(Proposal {
        polygon: q.map(|v| {
            let (x, y) = (v[0] - center[0], v[1] - center[1]);
            [center[0] + c * x - s * y, center[1] + s * x + c * y]
        }),
        score: coherence.min(0.99),
    })
}

// Supplemental fit from the edges inside a previously admitted dense band.
// Parent geometry and all prior candidates remain unchanged. No decoder/GT input.
fn refine_band_angle(
    edges: &[Edge],
    gx: &[f32],
    gy: &[f32],
    stride: usize,
    parent: f64,
    b: [f64; 4],
    sx: f64,
    sy: f64,
) -> Option<Proposal> {
    let (c, s) = (parent.cos(), parent.sin());
    let subset: Vec<_> = edges
        .iter()
        .filter(|e| {
            let v = -e.x * s + e.y * c;
            v >= b[2] && v <= b[3]
        })
        .collect();
    if subset.len() < 80 {
        return None;
    }
    let (mut xx, mut xy, mut yy) = (0., 0., 0.);
    for e in &subset {
        let i = (e.y.floor() as usize) * stride + e.x.floor() as usize;
        let (dx, dy) = (f64::from(gx[i]), f64::from(gy[i]));
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    let coherence = (xx - yy).hypot(2. * xy) / (xx + yy + 1.);
    if coherence < 0.60 {
        return None;
    }
    let initial = 0.5 * (2. * xy).atan2(xx - yy);
    if distance(initial, parent) > 15f64.to_radians() {
        return None;
    }
    let offset = (stride as f64).hypot((gx.len() / stride) as f64);
    let mut bins = vec![0f32; (offset * 4.).ceil() as usize + 8];
    let (mut best, mut angle) = (f64::NEG_INFINITY, initial);
    let mut best_step: i32 = 0;
    for step in -10..=10 {
        let a = initial + f64::from(step) * std::f64::consts::PI / 360.;
        let (c, s) = (a.cos(), a.sin());
        bins.fill(0.);
        for e in &subset {
            let p = (e.x * c + e.y * s + offset) * 2.;
            let i = p.floor() as usize;
            let f = p - i as f64;
            if i + 1 < bins.len() {
                bins[i] = (f64::from(bins[i]) + e.weight * (1. - f)) as f32;
                bins[i + 1] = (f64::from(bins[i + 1]) + e.weight * f) as f32;
            }
        }
        let score = bins
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>();
        if score > best {
            best = score;
            angle = a;
            best_step = step;
        }
    }
    // A search endpoint collapsing to the parent gives no supplementary geometry.
    // Preserve every accepted projection fit; only this censored search may use
    // the already-qualified local tensor. The same output and work caps apply.
    if cfg!(feature = "experimental-band-endpoint-tensor")
        && best_step.abs() == 10
        && distance(angle, parent) < 0.5f64.to_radians()
    {
        angle = initial;
    }
    if distance(angle, parent) < 0.5f64.to_radians() || distance(angle, parent) > 15f64.to_radians()
    {
        return None;
    }
    let (c, s) = (angle.cos(), angle.sin());
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for e in subset {
        let (u, v) = (e.x * c + e.y * s, -e.x * s + e.y * c);
        u0 = u0.min(u);
        u1 = u1.max(u);
        v0 = v0.min(v);
        v1 = v1.max(v);
    }
    if u1 - u0 < 45. || v1 - v0 < 7. || !(0.65..=30.).contains(&((u1 - u0) / (v1 - v0))) {
        return None;
    }
    let at = |u: f64, v: f64| [(u * c - v * s) * sx, (u * s + v * c) * sy];
    Some(Proposal {
        polygon: [
            at(u0 - 1., v0),
            at(u1 + 1., v0),
            at(u1 + 1., v1),
            at(u0 - 1., v1),
        ],
        score: coherence.min(0.99),
    })
}

// Same source-dimension predicate as the pixel-level extent verifier. Completing
// this cheap check is not an unexamined search; it rejects no base proposals.
fn extent_dimensions(p: &Proposal) -> Option<(f64, f64)> {
    let q = &p.polygon;
    let w = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
    let h = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
    ((76. ..=400.).contains(&w) && (12. ..=200.).contains(&h)).then_some((w, h))
}
fn extent_plan(proposals: &[Proposal], eligible_budget: bool) -> (Vec<usize>, usize, usize) {
    let eligible: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter_map(|(i, p)| extent_dimensions(p).map(|_| i))
        .collect();
    let scheduled = eligible
        .iter()
        .copied()
        .filter(|&i| eligible_budget || i < 24)
        .take(24)
        .collect();
    let count = eligible.len();
    (scheduled, count, proposals.len() - count)
}

/// Signed source-pixel multiline edge evidence, ported from061/extent.mts.
fn source_extent(im: ImageView<'_>, p: &Proposal) -> Option<Proposal> {
    let q = &p.polygon;
    let ex = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
    let ey = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
    let (w, _h) = extent_dimensions(p)?;
    let margin = (w * 0.35).min(100.);
    let start = -(margin.ceil() as isize);
    let end = (w + margin).ceil() as isize;
    let count = (end - start + 1) as usize;
    let gray = |x: usize, y: usize| {
        let i = y * im.stride + x * im.channels;
        if im.channels == 1 {
            f64::from(im.data[i])
        } else {
            (77. * f64::from(im.data[i])
                + 150. * f64::from(im.data[i + 1])
                + 29. * f64::from(im.data[i + 2]))
                / 256.
        }
    };
    let sample = |x: f64, y: f64| -> Option<f64> {
        if x < 0. || y < 0. || x >= (im.width - 1) as f64 || y >= (im.height - 1) as f64 {
            return None;
        }
        let (ix, iy) = (x.floor() as usize, y.floor() as usize);
        let (fx, fy) = (x - ix as f64, y - iy as f64);
        Some(
            (gray(ix, iy) * (1. - fx) + gray(ix + 1, iy) * fx) * (1. - fy)
                + (gray(ix, iy + 1) * (1. - fx) + gray(ix + 1, iy + 1) * fx) * fy,
        )
    };
    let mut positive = vec![0u8; count];
    let mut negative = vec![0u8; count];
    for fraction in [0.2, 0.35, 0.5, 0.65, 0.8] {
        let mut last = None;
        for i in 0..count {
            let u = (start + i as isize) as f64 / w;
            let v = sample(
                q[0][0] + ex[0] * u + ey[0] * fraction,
                q[0][1] + ex[1] * u + ey[1] * fraction,
            );
            if let (Some(v), Some(old)) = (v, last) {
                if v - old >= 18. {
                    positive[i] += 1;
                }
                if old - v >= 18. {
                    negative[i] += 1;
                }
            }
            last = v;
        }
    }
    let mut events = Vec::new();
    let mut prior = 0i8;
    for i in 0..count {
        let polarity = if positive[i] >= 3 {
            1
        } else if negative[i] >= 3 {
            -1
        } else {
            0
        };
        if polarity != 0 && polarity != prior {
            events.push((start + i as isize) as f64);
        }
        prior = polarity;
    }
    let gap = (w * 0.05).clamp(8., 24.);
    let (mut begin, mut best_begin, mut best_end) = (0, 0, 0);
    while begin < events.len() {
        let mut end = begin + 1;
        while end < events.len() && events[end] - events[end - 1] <= gap {
            end += 1;
        }
        if end - begin >= 30
            && events[begin] < w * 0.5
            && events[end - 1] > w * 0.5
            && end - begin > best_end - best_begin
        {
            best_begin = begin;
            best_end = end;
        }
        begin = end;
    }
    if best_end == 0 {
        return None;
    }
    let lo = (events[best_begin] - 2.).min(0.);
    let hi = (events[best_end - 1] + 2.).max(w);
    if lo >= -2. && hi <= w + 2. {
        return None;
    }
    let at = |u: f64, v: f64| {
        [
            q[0][0] + ex[0] * u / w + ey[0] * v,
            q[0][1] + ex[1] * u / w + ey[1] * v,
        ]
    };
    Some(Proposal {
        polygon: [at(lo, 0.), at(hi, 0.), at(hi, 1.), at(lo, 1.)],
        score: p.score,
    })
}
#[cfg(test)]
mod extent_tests {
    use super::*;
    fn image(kind: usize) -> Vec<u8> {
        let mut d = vec![255; 360 * 100];
        for y in 20..80 {
            for x in 60..260 {
                if kind > 0 && ((x - 60) / if kind == 2 { 1 } else { 3 }) % 2 == 0 {
                    d[y * 360 + x] = 0;
                }
            }
        }
        d
    }
    fn quad() -> Proposal {
        Proposal {
            polygon: [[90., 25.], [275., 25.], [275., 75.], [90., 75.]],
            score: 1.,
        }
    }
    #[test]
    fn source_angle_then_extent_preserves_source_stripes() {
        let d = image(1);
        let im = ImageView::new(&d, 360, 100, 1, 360).unwrap();
        let a = 5f64.to_radians();
        let (c, s) = (a.cos(), a.sin());
        let q = Proposal {
            polygon: [[90., 40.], [275., 40.], [275., 60.], [90., 60.]].map(|p| {
                let (x, y) = (p[0] - 182.5, p[1] - 50.);
                [182.5 + c * x - s * y, 50. + s * x + c * y]
            }),
            score: 1.,
        };
        let corrected = source_angle(im, &q).expect("native stripe tensor corrects slant");
        let grown = source_extent(im, &corrected).expect("clipped stripe extent grows");
        assert!(grown.polygon[0][0] < 70.);
        assert!((grown.polygon[1][1] - grown.polygon[0][1]).abs() < 1.);
        let blank = image(0);
        assert!(source_angle(ImageView::new(&blank, 360, 100, 1, 360).unwrap(), &q).is_none());
    }
    #[test]
    fn source_extent_recovers_clipped_bars_and_polarity() {
        for kind in [1, 2] {
            let d = image(kind);
            let im = ImageView::new(&d, 360, 100, 1, 360).unwrap();
            let q = quad();
            let r = source_extent(im, &q).unwrap();
            assert!(r.polygon[0][0] <= 62.);
            assert_eq!(r.polygon[1][0], 275.);
            assert_eq!(q.polygon[0], [90., 25.]);
        }
    }
    #[test]
    fn source_extent_rejects_blank_and_single_row_artifacts() {
        let mut d = image(0);
        let q = quad();
        assert!(source_extent(ImageView::new(&d, 360, 100, 1, 360).unwrap(), &q).is_none());
        for x in 60..260 {
            d[50 * 360 + x] = ((x / 3) % 2 * 255) as u8;
        }
        assert!(source_extent(ImageView::new(&d, 360, 100, 1, 360).unwrap(), &q).is_none());
    }
    #[test]
    fn source_extent_respects_quiet_gap() {
        let mut d = image(1);
        for y in 20..80 {
            for x in 8..35 {
                d[y * 360 + x] = ((x / 3) % 2 * 255) as u8;
            }
        }
        let r = source_extent(ImageView::new(&d, 360, 100, 1, 360).unwrap(), &quad()).unwrap();
        assert!(r.polygon[0][0] > 50.);
    }
}

#[cfg(all(test, feature = "experimental-stripe-extent"))]
mod extent_budget_tests {
    use super::*;
    #[test]
    fn clutter_reports_unexamined_extent_work() {
        let (w, h) = (1000, 800);
        let mut data = vec![255u8; w * h];
        for row in 0..6 {
            for col in 0..8 {
                let (x0, y0) = (15 + col * 122, 20 + row * 125);
                for y in y0..y0 + 40 {
                    for x in x0..x0 + 96 {
                        data[y * w + x] = if (x - x0) / 3 % 2 == 0 { 0 } else { 255 };
                    }
                }
            }
        }
        let r = detect(ImageView::new(&data, w, h, 1, w).unwrap()).unwrap();
        assert!(
            r.trace[7] > 0,
            "fixture must exceed extent budget: {:?}",
            r.trace
        );
        assert!(r.limited);
        assert!(r.omitted >= r.trace[7]);
        assert!(r.proposals.len() <= 24);
    }
    #[test]
    fn blank_has_no_unexamined_extent_work() {
        let d = vec![255u8; 256 * 256];
        let r = detect(ImageView::new(&d, 256, 256, 1, 256).unwrap()).unwrap();
        assert_eq!(r.trace[7], 0);
        assert_eq!(r.omitted, 0);
        assert!(!r.limited);
    }
}

#[cfg(test)]
mod angle_distance_tests {
    use super::distance;
    #[test]
    fn modulo_distance_matches_trigonometric_reference() {
        let pi = std::f64::consts::PI;
        for i in -720..=720 {
            for j in -90..=90 {
                let a = f64::from(i) * pi / 360.;
                let b = f64::from(j) * pi / 180.;
                let old = (2. * (a - b)).sin().atan2((2. * (a - b)).cos()).abs() / 2.;
                assert!((distance(a, b) - old).abs() < 2e-15);
            }
        }
        assert_eq!(distance(-pi / 2., pi / 2.), 0.);
        assert_eq!(distance(0., pi / 2.), pi / 2.);
        assert_eq!(distance(0., 0.), 0.);
    }
    #[test]
    fn angle_gate_preserves_sides_of_all_used_thresholds() {
        let pi = std::f64::consts::PI;
        for threshold in [pi / 9., pi / 18.] {
            for base in [-pi / 2., -0.3, 0., 0.7, pi / 2.] {
                for sign in [-1., 1.] {
                    for delta in [-1e-12, 1e-12] {
                        let other = base + sign * (threshold + delta);
                        assert_eq!(distance(base, other) < threshold, delta < 0.);
                    }
                }
            }
        }
    }
}

#[cfg(all(test, feature = "experimental-stripe-tall-bands"))]
mod tall_band_tests {
    use super::*;
    #[test]
    fn dense_barcode_band_survives_tall_connected_border() {
        let (w, h) = (400, 400);
        let mut d = vec![255u8; w * h];
        for y in 20..380 {
            for x in 80..83 {
                d[y * w + x] = 0;
            }
        }
        for y in 200..260 {
            for x in 80..280 {
                if ((x - 80) / 3) % 2 == 0 {
                    d[y * w + x] = 0;
                }
            }
        }
        let im = ImageView::new(&d, w, h, 1, w).unwrap();
        let mut tall = false;
        let r = detect_with_observer(im, |g| {
            tall |= g.reason == "tall" && g.edges > 1000;
        })
        .unwrap();
        assert!(tall, "must exercise rejected connected parent");
        assert!(
            r.proposals.iter().any(|p| {
                let q = p.polygon;
                let width = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
                let height = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
                let cy = q.iter().map(|p| p[1]).sum::<f64>() / 4.;
                width > 180. && height < 80. && (200. ..260.).contains(&cy)
            }),
            "dense band missing"
        );
        let plain = detect(im).unwrap();
        assert_eq!(plain.trace, r.trace);
        assert_eq!(plain.omitted, r.omitted);
        assert_eq!(plain.proposals.len(), r.proposals.len());
        for (a, b) in plain.proposals.iter().zip(&r.proposals) {
            assert_eq!(a.polygon, b.polygon);
            assert_eq!(a.score, b.score);
        }
    }
    #[test]
    fn lone_tall_border_does_not_become_a_barcode_band() {
        let (w, h) = (400, 400);
        let mut d = vec![255u8; w * h];
        for y in 20..380 {
            for x in 80..83 {
                d[y * w + x] = 0;
            }
        }
        assert!(detect(ImageView::new(&d, w, h, 1, w).unwrap())
            .unwrap()
            .proposals
            .is_empty());
    }
}

#[cfg(test)]
mod edge_bound_tests {
    use super::edge_weight;
    #[test]
    fn prefilter_keeps_exact_original_weights_and_decisions() {
        for ix in -255..=255 {
            for iy in -255..=255 {
                let (dx, dy) = (f64::from(ix) / 2., f64::from(iy) / 2.);
                for angle in [0f64, 0.4, 0.9, 1.5, 2.4] {
                    let (ax, ay) = (angle.cos(), angle.sin());
                    let w = dx.hypot(dy);
                    let original = (w > 18. && (dx * ax + dy * ay).abs() > 0.85 * w).then_some(w);
                    assert_eq!(edge_weight(dx, dy, ax, ay), original);
                }
            }
        }
    }
}

#[cfg(test)]
mod extent_plan_tests {
    use super::*;
    fn q(w: f64, h: f64) -> Proposal {
        Proposal {
            polygon: [[0., 0.], [w, 0.], [w, h], [0., h]],
            score: 1.,
        }
    }
    #[test]
    fn ineligible_regions_do_not_consume_sampling_slots() {
        let mut p = vec![q(500., 50.); 24];
        p.extend((0..30).map(|_| q(150., 50.)));
        let (old, n, bad) = extent_plan(&p, false);
        assert!(old.is_empty());
        assert_eq!((n, bad), (30, 24));
        let (new, n, bad) = extent_plan(&p, true);
        assert_eq!(new, (24..48).collect::<Vec<_>>());
        assert_eq!(n - new.len(), 6);
        assert_eq!(bad, 24);
        assert_eq!(p.len(), 54);
    }
    #[test]
    fn shared_dimension_check_preserves_boundaries_and_invalid_rejection() {
        for (w, h, valid) in [
            (76., 12., true),
            (400., 200., true),
            (75.9, 20., false),
            (400.1, 20., false),
            (100., 11.9, false),
            (100., 200.1, false),
            (f64::NAN, 50., false),
        ] {
            assert_eq!(extent_dimensions(&q(w, h)).is_some(), valid);
        }
        let p = vec![q(100., 50.); 30];
        let (a, _, _) = extent_plan(&p, false);
        let (b, _, _) = extent_plan(&p, true);
        assert_eq!(a, b);
        assert_eq!(a.len(), 24);
    }
}

#[cfg(test)]
mod band_angle_tests {
    use super::*;
    #[test]
    fn band_angle_rejects_blank_or_unsupported_edges() {
        let g = vec![0f32; 100 * 100];
        assert!(refine_band_angle(&[], &g, &g, 100, 0., [0., 99., 0., 99.], 1., 1.).is_none());
        let e: Vec<_> = (0..100)
            .map(|i| Edge {
                x: f64::from(i % 50) + 10.5,
                y: f64::from(i / 50) + 20.5,
                weight: 50.,
            })
            .collect();
        assert!(refine_band_angle(&e, &g, &g, 100, 0., [0., 99., 0., 99.], 1., 1.).is_none());
    }
    #[test]
    fn band_angle_fits_coherent_slanted_edges_in_parent_band() {
        let w = 320;
        let h = 240;
        let a = 0.10f64;
        let (c, s) = (a.cos(), a.sin());
        let mut gx = vec![0f32; w * h];
        let mut gy = gx.clone();
        let mut edges = Vec::new();
        for u in (-90..=90).step_by(6) {
            for v in -25..=25 {
                let x = 160. + f64::from(u) * c - f64::from(v) * s;
                let y = 120. + f64::from(u) * s + f64::from(v) * c;
                let i = y.floor() as usize * w + x.floor() as usize;
                gx[i] = (100. * c) as f32;
                gy[i] = (100. * s) as f32;
                edges.push(Edge {
                    x: x.floor() + 0.5,
                    y: y.floor() + 0.5,
                    weight: 100.,
                });
            }
        }
        let p = refine_band_angle(&edges, &gx, &gy, w, 0., [50., 270., 85., 155.], 1., 1.)
            .expect("coherent slanted band");
        let q = p.polygon;
        let angle = (q[1][1] - q[0][1]).atan2(q[1][0] - q[0][0]);
        assert!(distance(angle, a) < 1f64.to_radians());
    }
}

// Apply component compatibility after dense bands have isolated bar height.
fn join_fragments(a: &Proposal, b: &Proposal, max_gap: f64) -> Option<Proposal> {
    let qa = a.polygon;
    let qb = b.polygon;
    let angle = (qa[1][1] - qa[0][1]).atan2(qa[1][0] - qa[0][0]);
    let other = (qb[1][1] - qb[0][1]).atan2(qb[1][0] - qb[0][0]);
    if distance(angle, other) > std::f64::consts::PI / 18. {
        return None;
    }
    let (c, s) = (angle.cos(), angle.sin());
    let bounds = |q: crate::scan::Quad| {
        let mut z = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for p in q {
            let (u, v) = (p[0] * c + p[1] * s, -p[0] * s + p[1] * c);
            z[0] = z[0].min(u);
            z[1] = z[1].max(u);
            z[2] = z[2].min(v);
            z[3] = z[3].max(v);
        }
        z
    };
    let x = bounds(qa);
    let y = bounds(qb);
    let (hx, hy) = (x[3] - x[2], y[3] - y[2]);
    let gap = (x[0] - y[1]).max(y[0] - x[1]);
    if gap <= 0.
        || gap > max_gap
        || hx.min(hy) / hx.max(hy) < 0.65
        || x[3].min(y[3]) - x[2].max(y[2]) < 0.8 * hx.min(hy)
    {
        return None;
    }
    let (u0, u1, v0, v1) = (
        x[0].min(y[0]),
        x[1].max(y[1]),
        x[2].min(y[2]),
        x[3].max(y[3]),
    );
    let at = |u: f64, v: f64| [u * c - v * s, u * s + v * c];
    Some(Proposal {
        polygon: [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)],
        score: a.score.min(b.score),
    })
}
#[cfg(test)]
mod fragment_tests {
    use super::*;
    fn p(x: f64, y: f64, w: f64, h: f64) -> Proposal {
        Proposal {
            polygon: [[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
            score: 0.9,
        }
    }
    #[test]
    fn adjacent_fragments_join_without_stacking_or_wide_gap() {
        let a = p(10., 20., 90., 60.);
        let b = p(110., 22., 140., 58.);
        let r = join_fragments(&a, &b, 16.).unwrap();
        assert_eq!(
            r.polygon,
            [[10., 20.], [250., 20.], [250., 80.], [10., 80.]]
        );
        assert!(join_fragments(&a, &p(10., 92., 90., 60.), 16.).is_none());
        assert!(join_fragments(&a, &p(130., 20., 90., 60.), 16.).is_none());
        assert!(join_fragments(&a, &p(90., 20., 90., 60.), 16.).is_none());
        assert!(join_fragments(&a, &p(110., 60., 90., 60.), 16.).is_none());
    }
}

fn covers_fragment(old: &Proposal, new: &Proposal, tolerance: f64) -> bool {
    let q = old.polygon;
    let area = (0..4)
        .map(|i| q[i][0] * q[(i + 1) % 4][1] - q[i][1] * q[(i + 1) % 4][0])
        .sum::<f64>();
    if area.abs() < 1e-6 {
        return false;
    }
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = dx.hypot(dy);
        if length < 1e-6 {
            return false;
        }
        for p in new.polygon {
            if area.signum() * (dx * (p[1] - a[1]) - dy * (p[0] - a[0])) < -tolerance * length {
                return false;
            }
        }
    }
    true
}
#[cfg(test)]
mod fragment_coverage_tests {
    use super::*;
    fn p(x: f64, w: f64) -> Proposal {
        Proposal {
            polygon: [[x, 0.], [x + w, 0.], [x + w, 60.], [x, 60.]],
            score: 1.,
        }
    }
    #[test]
    fn coverage_only_suppresses_existing_extent() {
        assert!(covers_fragment(&p(0., 100.), &p(-1., 102.), 2.));
        assert!(!covers_fragment(&p(0., 100.), &p(0., 150.), 2.));
        let mut reversed = p(0., 100.);
        reversed.polygon.reverse();
        assert!(covers_fragment(&reversed, &p(1., 98.), 2.));
    }
}

#[cfg(test)]
mod angle_only_refinement_tests {
    use super::*;
    #[test]
    fn useful_rotation_does_not_require_crop_growth() {
        let (w, h) = (360, 160);
        let mut pixels = vec![255u8; w * h];
        for y in 30..130 {
            for x in 60..280 {
                pixels[y * w + x] = if x % 6 < 3 { 0 } else { 255 };
            }
        }
        let a = 5f64.to_radians();
        let (c, t) = (a.cos(), a.sin());
        let p = Proposal {
            polygon: [[-130., -45.], [130., -45.], [130., 45.], [-130., 45.]]
                .map(|[x, y]| [180. + c * x - t * y, 80. + t * x + c * y]),
            score: 0.9,
        };
        let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
        let (grown, angles) = source_refinements(im, &[p]);
        assert!(grown.is_empty());
        assert_eq!(angles.len(), 1);
        let q = angles[0].polygon;
        assert!((q[1][1] - q[0][1]).abs() < 1.);
    }
}

#[cfg(all(test, feature = "experimental-angle-replace"))]
mod angle_replacement_tests {
    use super::*;
    fn proposal(degree: f64, score: f64) -> Proposal {
        let a = degree.to_radians();
        let (c, s) = (a.cos(), a.sin());
        Proposal {
            polygon: [[-130., -45.], [130., -45.], [130., 45.], [-130., 45.]]
                .map(|[x, y]| [180. + c * x - s * y, 80. + s * x + c * y]),
            score,
        }
    }
    #[test]
    fn only_high_coherence_small_correction_replaces_parent_geometry() {
        for (angle, score, replaces) in [
            (5., 0.9, true),
            (10., 0.95, true),
            (10.1, 0.99, false),
            (5., 0.899, false),
        ] {
            let mut parent = proposal(angle, 0.8);
            let old = parent.polygon;
            let corrected = proposal(0., score);
            let expected = corrected.polygon;
            let (mut grown, mut supplements) = (Vec::new(), Vec::new());
            admit_source_refinement(&mut parent, corrected, None, &mut grown, &mut supplements);
            assert!(grown.is_empty());
            if replaces {
                assert_eq!(parent.polygon, expected);
                assert!(supplements.is_empty());
                assert_eq!(parent.score, score);
            } else {
                assert_eq!(parent.polygon, old);
                assert_eq!(supplements.len(), 1);
                assert_eq!(supplements[0].polygon, expected);
            }
        }
        assert!(angle_replacement_qualified(
            &proposal(175., 0.8),
            &proposal(-179., 0.95)
        ));
    }
    #[test]
    fn grown_extent_keeps_parent_and_supplement_even_when_angle_qualifies() {
        let mut parent = proposal(5., 0.8);
        let before = parent.polygon;
        let corrected = proposal(0., 0.99);
        let mut grown = proposal(0., 0.99);
        grown.polygon[0][0] -= 30.;
        grown.polygon[3][0] -= 30.;
        let expected = grown.polygon;
        let (mut extents, mut angles) = (Vec::new(), Vec::new());
        admit_source_refinement(
            &mut parent,
            corrected,
            Some(grown),
            &mut extents,
            &mut angles,
        );
        assert_eq!(parent.polygon, before);
        assert_eq!(extents.len(), 1);
        assert_eq!(extents[0].polygon, expected);
        assert!(angles.is_empty());
    }
    #[test]
    fn source_tensor_replaces_qualified_angle_only_but_preserves_large_correction() {
        let (w, h) = (360, 160);
        let mut pixels = vec![255u8; w * h];
        for y in 30..130 {
            for x in 60..280 {
                pixels[y * w + x] = if x % 6 < 3 { 0 } else { 255 };
            }
        }
        let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
        for degree in [5., 15.] {
            let parent = proposal(degree, 0.8);
            let before = parent.polygon;
            let mut parents = vec![parent];
            let (control_grown, control_angles) = source_refinements(im, &[parent]);
            let (grown, angles) = source_refinements_replacing(im, &mut parents);
            assert!(grown.is_empty());
            assert!(control_grown.is_empty());
            assert_eq!(control_angles.len(), 1);
            if degree == 5. {
                assert!(angles.is_empty());
                assert_eq!(parents[0].polygon, control_angles[0].polygon);
                assert_ne!(parents[0].polygon, before);
            } else {
                assert_eq!(parents[0].polygon, before);
                assert_eq!(angles.len(), 1);
                assert_eq!(angles[0].polygon, control_angles[0].polygon);
            }
        }
    }
}

#[cfg(test)]
mod exact_luma_gradient_tests {
    #[test]
    fn integer_luminance_matches_all_rgb_values() {
        for r in 0..256u32 {
            for g in 0..256u32 {
                for b in 0..256u32 {
                    let new = (77 * r + 150 * g + 29 * b) as f32 / 256.;
                    let old = ((77. * f64::from(r) + 150. * f64::from(g) + 29. * f64::from(b))
                        / 256.) as f32;
                    assert_eq!(new.to_bits(), old.to_bits());
                }
            }
        }
    }
    #[test]
    fn raw_scharr_f32_matches_f64_at_extremes_and_random_pixels() {
        let mut seed = 19u32;
        for _ in 0..100_000 {
            let p: [f32; 8] = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed % 65281) as f32 / 256.
            });
            let fast = (3. * (p[2] - p[0]) + 10. * (p[4] - p[3]) + 3. * (p[7] - p[5])) / 16.;
            let old = (3. * (f64::from(p[2]) - f64::from(p[0]))
                + 10. * (f64::from(p[4]) - f64::from(p[3]))
                + 3. * (f64::from(p[7]) - f64::from(p[5])))
                / 16.;
            assert_eq!(f64::from(fast).to_bits(), old.to_bits());
        }
    }
}

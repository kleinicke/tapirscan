//! Bar-segment voting localizer for Medium.
//!
//! A linear barcode is a family of parallel, similarly long edge segments whose centres lie on
//! one line across the bars. Strong Scharr edges on a working raster (never upscaled) are
//! labelled into elongated segments by polarity-aware orientation; segments grown from the
//! longest unused seed form one cluster per symbol. Each cluster becomes one parallelogram: the
//! scan axis follows the principal axis of the member centres, the bar direction the
//! length-weighted mean segment angle. Split clusters of one symbol are merged, and every
//! cluster keeps a few alternative boxes that the pipeline tries only when its box stays
//! unread.
#![allow(
    // Raster indices and small counts convert between usize, isize and f32/f64 throughout;
    // all values are bounded by the working raster (<= 768 px per side).
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names
)]
use super::{Image, Proposal, Quad};

/// Long side of the working raster; smaller images keep their resolution.
const WORK: f64 = 768.;
/// Polarity-aware gradient orientation bins.
const BINS: usize = 16;
/// Minimum Scharr gradient magnitude of an edge pixel.
const EDGE: f32 = 250.;
/// Minimum segment length (pixels) and length-to-width ratio.
const MIN_LENGTH: f64 = 6.;
const ELONGATION: f64 = 3.;
/// Cluster growth: parallel within 8 degrees, length within 0.5-2x of the seed, offset along
/// the bars below 0.35 seed lengths, gaps across the bars below 0.6 seed lengths.
const PARALLEL_DEGREES: f64 = 8.;
const ALONG: f64 = 0.35;
const GAP: f64 = 0.6;
const MIN_SEGMENTS: usize = 6;
/// Quiet-zone pad across the bars, as a fraction of the cluster width.
const PAD: f64 = 0.08;
/// Member count at which a cluster scores 1.
const FULL: f64 = 25.;
/// Clusters with at least this many segments keep alternative boxes.
const ALTERNATIVE_SEGMENTS: usize = 10;
/// Split clusters merge when less than this many box heights apart across the bars.
const MERGE_GAP: f64 = 0.3;
const MAX_PROPOSALS: usize = 24;

pub(crate) struct Localized {
    pub proposals: Vec<Proposal>,
    /// Alternative boxes per primary box, tried only for unread clusters.
    #[cfg(feature = "medium")]
    pub alternatives: Vec<(Quad, Vec<Quad>)>,
}

struct Segment {
    x: f64,
    y: f64,
    angle: f64,
    length: f64,
}

fn gray(image: Image<'_>, work: f64) -> (Vec<f32>, usize, usize, f64) {
    let long = image.width.max(image.height) as f64;
    let s = (work / long).min(1.);
    let w = ((image.width as f64 * s).round() as usize).max(3);
    let h = ((image.height as f64 * s).round() as usize).max(3);
    let lum = |x: usize, y: usize| -> f32 {
        let p = &image.data[y * image.stride + x * image.channels..];
        if image.channels == 1 {
            f32::from(p[0])
        } else {
            (77. * f32::from(p[0]) + 150. * f32::from(p[1]) + 29. * f32::from(p[2])) / 256.
        }
    };
    let mut g = vec![0f32; w * h];
    if s >= 1. {
        for y in 0..h {
            for x in 0..w {
                g[y * w + x] = lum(x, y);
            }
        }
        return (g, w, h, s);
    }
    // Average a 2 x 2 sample grid per working cell: single point samples alias bars whose
    // modules shrink below one working pixel into short, misplaced segments. Four samples keep
    // the cost bounded by the working raster rather than the source size.
    let at = |o: usize, f: f64, n: usize| (((o as f64 + f) / s) as usize).min(n - 1);
    let xs: Vec<[usize; 2]> = (0..w)
        .map(|ox| [at(ox, 0.25, image.width), at(ox, 0.75, image.width)])
        .collect();
    let channels = image.channels;
    let luma = |row: &[u8], x: usize| -> u32 {
        let p = &row[x * channels..];
        if channels == 1 {
            256 * u32::from(p[0])
        } else {
            77 * u32::from(p[0]) + 150 * u32::from(p[1]) + 29 * u32::from(p[2])
        }
    };
    for oy in 0..h {
        let r0 = &image.data[at(oy, 0.25, image.height) * image.stride..];
        let r1 = &image.data[at(oy, 0.75, image.height) * image.stride..];
        for (v, &[x0, x1]) in g[oy * w..(oy + 1) * w].iter_mut().zip(&xs) {
            let sum = luma(r0, x0) + luma(r0, x1) + luma(r1, x0) + luma(r1, x1);
            *v = sum as f32 / 1024.;
        }
    }
    (g, w, h, s)
}

/// Polynomial arctangent (error below 0.005 rad), ample for 22.5 degree bins.
fn fast_angle(gx: f32, gy: f32) -> f32 {
    let (ax, ay) = (gx.abs(), gy.abs());
    let (mn, mx) = (ax.min(ay), ax.max(ay).max(1e-9));
    let t = mn / mx;
    let mut r = t * (0.9724 - 0.1919 * t * t);
    if ay > ax {
        r = std::f32::consts::FRAC_PI_2 - r;
    }
    if gx < 0. {
        r = std::f32::consts::PI - r;
    }
    if gy < 0. {
        r = -r;
    }
    r
}

fn segments(g: &[f32], w: usize, h: usize) -> Vec<Segment> {
    let n = w * h;
    let mut bin = vec![u8::MAX; n];
    let mut strong = Vec::new();
    let limit = EDGE * EDGE;
    let mut gxs = vec![0f32; w];
    let mut gys = vec![0f32; w];
    let mut m2 = vec![0f32; w];
    for y in 1..h - 1 {
        let (r0, r1, r2) = (
            &g[(y - 1) * w..y * w],
            &g[y * w..(y + 1) * w],
            &g[(y + 1) * w..(y + 2) * w],
        );
        for x in 1..w - 1 {
            let gx = 3. * (r0[x + 1] - r0[x - 1])
                + 10. * (r1[x + 1] - r1[x - 1])
                + 3. * (r2[x + 1] - r2[x - 1]);
            let gy =
                3. * (r2[x - 1] - r0[x - 1]) + 10. * (r2[x] - r0[x]) + 3. * (r2[x + 1] - r0[x + 1]);
            gxs[x] = gx;
            gys[x] = gy;
            m2[x] = gx * gx + gy * gy;
        }
        for x in 1..w - 1 {
            if m2[x] > limit {
                let a = fast_angle(gxs[x], gys[x]);
                let i = y * w + x;
                bin[i] = ((((a + std::f32::consts::PI) / (2. * std::f32::consts::PI)) * BINS as f32)
                    as usize
                    % BINS) as u8;
                strong.push(i);
            }
        }
    }
    let mut seen = vec![false; n];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let circular = |a: u8, b: u8| {
        let d = (i32::from(a) - i32::from(b)).rem_euclid(BINS as i32);
        d.min(BINS as i32 - d)
    };
    for &seed in &strong {
        if seen[seed] {
            continue;
        }
        let k = bin[seed];
        seen[seed] = true;
        stack.push(seed);
        let (mut c, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0f64, 0., 0., 0., 0., 0.);
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as f64, (i / w) as f64);
            c += 1.;
            sx += x;
            sy += y;
            sxx += x * x;
            syy += y * y;
            sxy += x * y;
            // Strong pixels never lie on the border, so all neighbours are in range.
            for j in [
                i - w - 1,
                i - w,
                i - w + 1,
                i - 1,
                i + 1,
                i + w - 1,
                i + w,
                i + w + 1,
            ] {
                if !seen[j] && bin[j] != u8::MAX && circular(bin[j], k) <= 1 {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
        if c < MIN_LENGTH {
            continue;
        }
        let (mx, my) = (sx / c, sy / c);
        let (vxx, vyy, vxy) = (sxx / c - mx * mx, syy / c - my * my, sxy / c - mx * my);
        let tr = vxx + vyy;
        let det = vxx * vyy - vxy * vxy;
        let disc = (tr * tr / 4. - det).max(0.).sqrt();
        let (l1, l2) = (tr / 2. + disc, (tr / 2. - disc).max(1e-6));
        let length = 4. * l1.sqrt();
        let width = 4. * l2.sqrt();
        if length < MIN_LENGTH || length < ELONGATION * width {
            continue;
        }
        let angle = (0.5 * (2. * vxy).atan2(vxx - vyy)).rem_euclid(std::f64::consts::PI);
        out.push(Segment {
            x: mx,
            y: my,
            angle,
            length,
        });
    }
    out
}

/// Parallelogram of a cluster: scan axis from the principal axis of the member centres (unless
/// a narrow cluster spreads mostly along its bars), bar direction from the length-weighted
/// mean segment angle. Returns `None` when the two axes are nearly parallel.
fn parallelogram(
    segs: &[Segment],
    members: &[usize],
    across: [f64; 2],
    along: [f64; 2],
    bar_length: f64,
    s: f64,
) -> Option<Quad> {
    let k = members.len() as f64;
    let (mx, my) = members.iter().fold((0., 0.), |(x, y), &m| {
        (x + segs[m].x / k, y + segs[m].y / k)
    });
    let (mut cxx, mut cyy, mut cxy) = (0., 0., 0.);
    for &m in members {
        let (dx, dy) = (segs[m].x - mx, segs[m].y - my);
        cxx += dx * dx;
        cyy += dy * dy;
        cxy += dx * dy;
    }
    let theta = 0.5 * (2. * cxy).atan2(cxx - cyy);
    let mut d = [theta.cos(), theta.sin()];
    if d[0] * across[0] + d[1] * across[1] < 0. {
        d = [-d[0], -d[1]];
    }
    if d[0] * across[0] + d[1] * across[1] < 30_f64.to_radians().cos() {
        d = across;
    }
    let (c2, s2) = members.iter().fold((0., 0.), |(c, s_), &m| {
        let w = segs[m].length;
        (
            c + w * (2. * segs[m].angle).cos(),
            s_ + w * (2. * segs[m].angle).sin(),
        )
    });
    let bar = 0.5 * s2.atan2(c2);
    let mut vb = [bar.cos(), bar.sin()];
    if vb[0] * along[0] + vb[1] * along[1] < 0. {
        vb = [-vb[0], -vb[1]];
    }
    let cross = d[0] * vb[1] - d[1] * vb[0];
    if cross.abs() <= 0.3 {
        return None;
    }
    let mut ts: Vec<f64> = Vec::new();
    let mut ws: Vec<f64> = Vec::new();
    for &m in members {
        let (ex, ey) = (segs[m].x - mx, segs[m].y - my);
        // e = t * d + w * vb
        ts.push((ex * vb[1] - ey * vb[0]) / cross);
        ws.push((d[0] * ey - d[1] * ex) / cross);
    }
    ts.sort_by(f64::total_cmp);
    ws.sort_by(f64::total_cmp);
    let (t0, t1) = (ts[0], ts[ts.len() - 1]);
    let pad = PAD * (t1 - t0);
    let (t0, t1) = (t0 - pad, t1 + pad);
    let wc = ws[ws.len() / 2];
    let l = bar_length;
    let c = |t: f64, w: f64| {
        [
            (mx + t * d[0] + w * vb[0]) / s,
            (my + t * d[1] + w * vb[1]) / s,
        ]
    };
    Some([
        c(t0, wc - l / 2.),
        c(t1, wc - l / 2.),
        c(t1, wc + l / 2.),
        c(t0, wc + l / 2.),
    ])
}

/// Box scaled along its first (`su`) and second (`sv`) edge around its centre, then rotated.
fn scaled(q: Quad, su: f64, sv: f64, rotation: f64) -> Quad {
    let centre = [
        q.iter().map(|p| p[0]).sum::<f64>() / 4.,
        q.iter().map(|p| p[1]).sum::<f64>() / 4.,
    ];
    let eu = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
    let ev = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
    let (c, sn) = (rotation.cos(), rotation.sin());
    let r = |d: [f64; 2]| [d[0] * c - d[1] * sn, d[0] * sn + d[1] * c];
    let (hu, hv) = (
        r([eu[0] * su / 2., eu[1] * su / 2.]),
        r([ev[0] * sv / 2., ev[1] * sv / 2.]),
    );
    [
        [centre[0] - hu[0] - hv[0], centre[1] - hu[1] - hv[1]],
        [centre[0] + hu[0] - hv[0], centre[1] + hu[1] - hv[1]],
        [centre[0] + hu[0] + hv[0], centre[1] + hu[1] + hv[1]],
        [centre[0] - hu[0] + hv[0], centre[1] - hu[1] + hv[1]],
    ]
}

/// Segments bucketed by centre on a square grid with the median segment length as cell size.
struct Grid {
    cells: Vec<Vec<usize>>,
    width: usize,
    height: usize,
    cell: f64,
}

impl Grid {
    fn new(segs: &[Segment], w: usize, h: usize, cell: f64) -> Self {
        let width = (w as f64 / cell) as usize + 1;
        let height = (h as f64 / cell) as usize + 1;
        let mut cells: Vec<Vec<usize>> = vec![Vec::new(); width * height];
        for (i, sg) in segs.iter().enumerate() {
            cells[(sg.y / cell) as usize * width + (sg.x / cell) as usize].push(i);
        }
        Self {
            cells,
            width,
            height,
            cell,
        }
    }

    /// Unused segments parallel to seed `i`, of similar length and aligned along its bars,
    /// as (offset across the bars, index) pairs.
    fn candidates(&self, segs: &[Segment], used: &[bool], i: usize, out: &mut Vec<(f64, usize)>) {
        let a = &segs[i];
        let (va, ua) = (
            [a.angle.cos(), a.angle.sin()],
            [-a.angle.sin(), a.angle.cos()],
        );
        let parallel = PARALLEL_DEGREES.to_radians();
        let r = (4. * a.length / self.cell).ceil() as isize;
        let (cx, cy) = ((a.x / self.cell) as isize, (a.y / self.cell) as isize);
        out.clear();
        for gy in (cy - r).max(0)..=(cy + r).min(self.height as isize - 1) {
            for gx in (cx - r).max(0)..=(cx + r).min(self.width as isize - 1) {
                for &j in &self.cells[gy as usize * self.width + gx as usize] {
                    if used[j] {
                        continue;
                    }
                    let b = &segs[j];
                    let mut da = (a.angle - b.angle).abs();
                    da = da.min(std::f64::consts::PI - da);
                    if da > parallel || b.length < 0.5 * a.length || b.length > 2. * a.length {
                        continue;
                    }
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    if (dx * va[0] + dy * va[1]).abs() < ALONG * a.length {
                        out.push((dx * ua[0] + dy * ua[1], j));
                    }
                }
            }
        }
    }
}

/// One cluster per symbol, grown from the longest unused segment; candidates are compared with
/// the seed only, so segment lengths cannot drift along a chain.
fn clusters(segs: &[Segment], w: usize, h: usize, s: f64) -> Localized {
    let n = segs.len();
    let mut lens: Vec<f64> = segs.iter().map(|s| s.length).collect();
    lens.sort_by(f64::total_cmp);
    let cell = lens[n / 2].max(4.);
    let grid = Grid::new(segs, w, h, cell);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| segs[j].length.total_cmp(&segs[i].length));
    let mut used = vec![false; n];
    let mut out: Vec<(Proposal, usize)> = Vec::new();
    let mut alternatives: Vec<(Quad, Vec<Quad>)> = Vec::new();
    let mut cand: Vec<(f64, usize)> = Vec::new();
    for &i in &order {
        if used[i] {
            continue;
        }
        let a = &segs[i];
        let (va, ua) = (
            [a.angle.cos(), a.angle.sin()],
            [-a.angle.sin(), a.angle.cos()],
        );
        grid.candidates(segs, &used, i, &mut cand);
        if cand.len() < MIN_SEGMENTS {
            continue;
        }
        cand.sort_by(|x, y| x.0.total_cmp(&y.0));
        let Some(seed) = cand.iter().position(|c| c.1 == i) else {
            continue;
        };
        let limit = GAP * a.length;
        let mut lo = seed;
        while lo > 0 && cand[lo].0 - cand[lo - 1].0 < limit {
            lo -= 1;
        }
        let mut hi = seed;
        while hi + 1 < cand.len() && cand[hi + 1].0 - cand[hi].0 < limit {
            hi += 1;
        }
        if hi - lo + 1 < MIN_SEGMENTS {
            continue;
        }
        let members: Vec<usize> = cand[lo..=hi].iter().map(|c| c.1).collect();
        for &m in &members {
            used[m] = true;
        }
        let pad = PAD * (cand[hi].0 - cand[lo].0);
        let (u0, u1) = (cand[lo].0 - pad, cand[hi].0 + pad);
        let mut pv: Vec<f64> = members
            .iter()
            .map(|&m| (segs[m].x - a.x) * va[0] + (segs[m].y - a.y) * va[1])
            .collect();
        let mut ls: Vec<f64> = members.iter().map(|&m| segs[m].length).collect();
        pv.sort_by(f64::total_cmp);
        ls.sort_by(f64::total_cmp);
        let vc = pv[pv.len() / 2];
        let l = ls[ls.len() / 2];
        let corner = |uu: f64, vv: f64| {
            [
                (a.x + uu * ua[0] + vv * va[0]) / s,
                (a.y + uu * ua[1] + vv * va[1]) / s,
            ]
        };
        let mut polygon = [
            corner(u0, vc - l / 2.),
            corner(u1, vc - l / 2.),
            corner(u1, vc + l / 2.),
            corner(u0, vc + l / 2.),
        ];
        if let Some(q) = parallelogram(segs, &members, ua, va, l, s) {
            polygon = q;
        }
        // Match the stripe proposals: positive shoelace area, first edge across the bars.
        let area: f64 = (0..4)
            .map(|k| {
                polygon[k][0] * polygon[(k + 1) % 4][1] - polygon[(k + 1) % 4][0] * polygon[k][1]
            })
            .sum();
        if area < 0. {
            polygon = [polygon[1], polygon[0], polygon[3], polygon[2]];
        }
        let score = 0.5 + 0.5 * (members.len() as f64 / FULL).min(1.);
        out.push((Proposal { polygon, score }, members.len()));
        if members.len() >= ALTERNATIVE_SEGMENTS {
            alternatives.push((polygon, alternative_boxes(polygon)));
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out = merge_split(out, &mut alternatives);
    out.truncate(MAX_PROPOSALS);
    alternatives.retain(|(q, _)| out.iter().any(|(p, _)| p.polygon == *q));
    #[cfg(not(feature = "medium"))]
    let _ = alternatives;
    Localized {
        proposals: out.into_iter().map(|(p, _)| p).collect(),
        #[cfg(feature = "medium")]
        alternatives,
    }
}

/// Alternative geometric hypotheses for one box: taller and wider, and rotated by 1.5 and 3 degrees.
fn alternative_boxes(polygon: Quad) -> Vec<Quad> {
    vec![
        scaled(polygon, 1.15, 1.4, 0.),
        scaled(polygon, 1.05, 1.0, (-1.5_f64).to_radians()),
        scaled(polygon, 1.05, 1.0, 1.5_f64.to_radians()),
        scaled(polygon, 1.05, 1.0, (-3_f64).to_radians()),
        scaled(polygon, 1.05, 1.0, 3_f64.to_radians()),
    ]
}

/// Join split clusters of one symbol: parallel (< 5 degrees), less than 0.3 box heights apart
/// perpendicular to the scan axis, and adjacent across the bars. Larger clusters absorb smaller.
fn merge_split(
    out: Vec<(Proposal, usize)>,
    alternatives: &mut [(Quad, Vec<Quad>)],
) -> Vec<(Proposal, usize)> {
    let mut merged: Vec<(Proposal, usize)> = Vec::new();
    for (p, n) in out {
        let q = p.polygon;
        let mut absorbed = false;
        for (k, kn) in &mut merged {
            let a = k.polygon;
            let (ux, uy) = (a[1][0] - a[0][0], a[1][1] - a[0][1]);
            let (vx, vy) = (a[3][0] - a[0][0], a[3][1] - a[0][1]);
            let (wa, ha) = (ux.hypot(uy), vx.hypot(vy));
            if wa < 1e-6 || ha < 1e-6 {
                continue;
            }
            let (u, v) = ([ux / wa, uy / wa], [vx / ha, vy / ha]);
            let (bx, by) = (q[1][0] - q[0][0], q[1][1] - q[0][1]);
            let bw = bx.hypot(by).max(1e-6);
            if ((u[0] * by - u[1] * bx) / bw).abs() > 5_f64.to_radians().sin() {
                continue;
            }
            let ca = [
                a.iter().map(|c| c[0]).sum::<f64>() / 4.,
                a.iter().map(|c| c[1]).sum::<f64>() / 4.,
            ];
            let cb = [
                q.iter().map(|c| c[0]).sum::<f64>() / 4.,
                q.iter().map(|c| c[1]).sum::<f64>() / 4.,
            ];
            // Distance perpendicular to the scan axis; the bar direction need not be orthogonal.
            let dv = u[0] * (cb[1] - ca[1]) - u[1] * (cb[0] - ca[0]);
            let hp = (u[0] * vy - u[1] * vx).abs();
            let proj: Vec<f64> = q
                .iter()
                .map(|c| (c[0] - a[0][0]) * u[0] + (c[1] - a[0][1]) * u[1])
                .collect();
            let b0 = proj.iter().copied().fold(f64::INFINITY, f64::min);
            let b1 = proj.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            if dv.abs() > 0.3 * hp || (b0 - wa).max(-b1) > MERGE_GAP * ha {
                continue;
            }
            let (lo, hi) = (b0.min(0.), b1.max(wa));
            let c = |t: f64, w: f64| [a[0][0] + t * u[0] + w * v[0], a[0][1] + t * u[1] + w * v[1]];
            let old = k.polygon;
            k.polygon = [c(lo, 0.), c(hi, 0.), c(hi, ha), c(lo, ha)];
            *kn += n;
            for entry in alternatives.iter_mut() {
                if entry.0 == old {
                    entry.0 = k.polygon;
                }
            }
            absorbed = true;
            break;
        }
        if !absorbed {
            merged.push((p, n));
        }
    }
    merged.sort_by(|a, b| b.1.cmp(&a.1));
    merged
}

pub(crate) fn localize(image: Image<'_>) -> Localized {
    let (g, w, h, s) = gray(image, WORK);
    let segs = segments(&g, w, h);
    if segs.is_empty() {
        return Localized {
            proposals: Vec::new(),
            #[cfg(feature = "medium")]
            alternatives: Vec::new(),
        };
    }
    clusters(&segs, w, h, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(data: &[u8], width: usize, height: usize) -> Image<'_> {
        Image {
            data,
            width,
            height,
            channels: 1,
            stride: width,
        }
    }

    #[test]
    fn one_box_spans_a_bar_pattern_across_its_bars() {
        // 30 dark bars with varying widths in a 400 x 200 white image, bars x 100..~300.
        let (width, height) = (400, 200);
        let mut data = vec![255u8; width * height];
        let mut x = 100;
        for k in 0..30 {
            let bar = 2 + k % 3;
            for y in 60..140 {
                for dx in 0..bar {
                    data[y * width + x + dx] = 0;
                }
            }
            x += bar + 2 + (k * 7) % 3;
        }
        let found = localize(image(&data, width, height));
        assert_eq!(found.proposals.len(), 1);
        let q = found.proposals[0].polygon;
        let across = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
        let along = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
        assert!(across > 180. && along > 50. && across > along);
        let centre_x = q.iter().map(|p| p[0]).sum::<f64>() / 4.;
        let centre_y = q.iter().map(|p| p[1]).sum::<f64>() / 4.;
        assert!((150. ..250.).contains(&centre_x) && (80. ..120.).contains(&centre_y));
        #[cfg(feature = "medium")]
        assert_eq!(found.alternatives.len(), 1);
        #[cfg(feature = "medium")]
        assert_eq!(found.alternatives[0].1.len(), 5);
    }

    #[test]
    fn blank_images_have_no_proposals() {
        let data = vec![200u8; 64 * 48];
        let found = localize(image(&data, 64, 48));
        assert!(found.proposals.is_empty());
        #[cfg(feature = "medium")]
        assert!(found.alternatives.is_empty());
    }
}

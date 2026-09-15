//! PDF417 guard-strip localization independent of payload row decoding.
//! Fit the repeated start/stop guards across rows, then rectify their bar axes.
use crate::linear::pattern_error;
type Quad = [[f32; 2]; 4];
#[derive(Clone)]
struct Strip {
    start: bool,
    reversed: bool,
    module: f32,
    points: Vec<[f32; 2]>,
}
struct Fit {
    start: bool,
    reversed: bool,
    module: f32,
    slope: f32,
    intercept: f32,
    top: f32,
    bottom: f32,
    support: usize,
}
impl Strip {
    fn fit(&self) -> Option<Fit> {
        if self.points.len() < 4 {
            return None;
        }
        let n = self.points.len() as f32;
        let mx = self.points.iter().map(|p| p[0]).sum::<f32>() / n;
        let my = self.points.iter().map(|p| p[1]).sum::<f32>() / n;
        let yy = self.points.iter().map(|p| (p[1] - my).powi(2)).sum::<f32>();
        if yy < 1. {
            return None;
        }
        let slope = self
            .points
            .iter()
            .map(|p| (p[0] - mx) * (p[1] - my))
            .sum::<f32>()
            / yy;
        let intercept = mx - slope * my;
        let error = (self
            .points
            .iter()
            .map(|p| (p[0] - slope * p[1] - intercept).powi(2))
            .sum::<f32>()
            / n)
            .sqrt();
        let top = self.points.first()?[1];
        let bottom = self.points.last()?[1];
        if slope.abs() > 1.4 || error > self.module * 1.2 || bottom - top < self.module * 2. {
            return None;
        }
        Some(Fit {
            start: self.start,
            reversed: self.reversed,
            module: self.module / (1. + slope * slope).sqrt(),
            slope,
            intercept,
            top,
            bottom,
            support: self.points.len(),
        })
    }
}
// Compact PDF417 has only the repeated start guard. Recover its width from
// independently decoded left indicators after aligning to the fitted bar axis.
fn compact_quad(
    gray: &[u8],
    w: usize,
    h: usize,
    fit: &Fit,
    step: usize,
    original: impl Fn(f32, f32) -> [f32; 2],
) -> Option<Quad> {
    let norm = (1. + fit.slope * fit.slope).sqrt();
    let sign = if fit.reversed { -1. } else { 1. };
    let cross = |y: f32| ((fit.slope * y + fit.intercept) * fit.slope + y) / norm;
    let delta = fit.module * 17. * fit.slope * sign;
    let top = cross(fit.top) - (-delta).max(0.) - step as f32;
    let bottom = cross(fit.bottom) + delta.max(0.) + step as f32;
    let point = |along: f32, across: f32| {
        let y = (across * norm - fit.intercept * fit.slope) / norm.powi(2);
        let x = fit.slope * y + fit.intercept;
        original(x + along * sign / norm, y - along * sign * fit.slope / norm)
    };
    let mut votes = [[0usize; 30]; 3];
    let mut modules = Vec::new();
    let samples = ((bottom - top) / fit.module.max(1.)).ceil().clamp(3., 270.) as usize;
    let width = (fit.module * 38.).ceil() as usize;
    if width < 38 {
        return None;
    }
    for i in 0..samples {
        let across = top + (i as f32 + 0.5) * (bottom - top) / samples as f32;
        let row: Vec<u8> = (0..width)
            .map(|x| {
                let [xx, yy] = point(x as f32 - fit.module * 2., across);
                let xx = xx.round() as isize;
                let yy = yy.round() as isize;
                if xx >= 0 && yy >= 0 && xx < w as isize && yy < h as isize {
                    gray[yy as usize * w + xx as usize]
                } else {
                    255
                }
            })
            .collect();
        if let Some((value, cluster, module)) = crate::pdf417::left_indicator(&row, 0)
            .or_else(|| crate::pdf417::left_indicator(&row, 1))
        {
            votes[cluster][value % 30] += 1;
            modules.push(module);
        }
    }
    let mut metadata = [0; 3];
    for (i, counts) in votes.iter().enumerate() {
        let (value, &count) = counts.iter().enumerate().max_by_key(|(_, count)| *count)?;
        if count < 2 || count * 2 <= counts.iter().sum() {
            return None;
        }
        metadata[i] = value;
    }
    let rows = metadata[0] * 3 + metadata[1] % 3 + 1;
    let columns = metadata[2] + 1;
    let level = metadata[1] / 3;
    if !(3..=90).contains(&rows)
        || level > 8
        || rows * columns > 928
        || (1usize << (level + 1)) >= rows * columns
    {
        return None;
    }
    // Keep an extra module beyond the two-module quiet zone so resampling and
    // reversed scanline edge rounding cannot shorten it below the reader gate.
    modules.sort_by(f32::total_cmp);
    let module = modules[modules.len() / 2];
    let end = module * ((columns + 2) * 17 + 4) as f32;
    Some([
        point(-2. * fit.module, top),
        point(end, top),
        point(end, bottom),
        point(-2. * fit.module, bottom),
    ])
}
///
/// # Panics
///
/// Panics if the supplied grayscale buffer or dimensions are inconsistent.
pub fn proposals(gray: &[u8], w: usize, h: usize) -> (Vec<(Quad, usize)>, bool) {
    let mut out: Vec<(Quad, usize)> = Vec::new();
    let mut compact_out: Vec<(Quad, usize)> = Vec::new();
    let mut limited = false;
    let mut compact_attempts = 0;
    for degrees in [0f32, 90., 45., -45.] {
        let angle = degrees.to_radians();
        let cs = angle.cos();
        let sn = angle.sin();
        let corners = [
            [0., 0.],
            [w as f32 - 1., 0.],
            [w as f32 - 1., h as f32 - 1.],
            [0., h as f32 - 1.],
        ];
        let amin = corners
            .iter()
            .map(|p| p[0] * cs + p[1] * sn)
            .fold(f32::INFINITY, f32::min);
        let amax = corners
            .iter()
            .map(|p| p[0] * cs + p[1] * sn)
            .fold(f32::NEG_INFINITY, f32::max);
        let bmin = corners
            .iter()
            .map(|p| -p[0] * sn + p[1] * cs)
            .fold(f32::INFINITY, f32::min);
        let bmax = corners
            .iter()
            .map(|p| -p[0] * sn + p[1] * cs)
            .fold(f32::NEG_INFINITY, f32::max);
        let (width, height) = (
            (amax - amin).ceil() as usize + 1,
            (bmax - bmin).ceil() as usize + 1,
        );
        let original = |x: f32, y: f32| {
            [
                (x + amin) * cs - (y + bmin) * sn,
                (x + amin) * sn + (y + bmin) * cs,
            ]
        };
        let step = (height / 400).max(1);
        let mut strips_by_mode: [Vec<Strip>; 2] = std::array::from_fn(|_| Vec::new());
        let mut active_by_mode: [Vec<usize>; 2] = std::array::from_fn(|_| Vec::new());
        for y in (0..height).step_by(step) {
            let row: Vec<_> = (0..width)
                .map(|x| {
                    let [xx, yy] = original(x as f32, y as f32);
                    let xx = xx.round() as isize;
                    let yy = yy.round() as isize;
                    if xx >= 0 && yy >= 0 && xx < w as isize && yy < h as isize {
                        gray[yy as usize * w + xx as usize]
                    } else {
                        255
                    }
                })
                .collect();
            if row.iter().all(|&value| value == row[0]) {
                continue;
            }
            for (mode, strips) in strips_by_mode.iter_mut().enumerate() {
                let active = &mut active_by_mode[mode];
                // A completed strip can never match a later row again. Keep
                // it for fitting, but omit it from future association searches.
                active.retain(|&index| {
                    y as f32 - strips[index].points.last().unwrap()[1] <= step as f32 * 3.
                });
                let mut bits = crate::threshold(&row, mode);
                for reversed in [false, true] {
                    if reversed {
                        bits.reverse();
                    }
                    let (runs, offsets) = crate::runs(&bits);
                    for s in (usize::from(!bits[0])..runs.len().saturating_sub(8)).step_by(2) {
                        for start in [true, false] {
                            let pattern: &[u8] = if start {
                                &[8, 1, 1, 1, 1, 1, 1, 3]
                            } else {
                                &[7, 1, 1, 3, 1, 1, 1, 2, 1]
                            };
                            if s + pattern.len() > runs.len()
                                || pattern_error(&runs[s..], pattern) > 0.12
                            {
                                continue;
                            }
                            let module = runs[s..s + pattern.len()].iter().sum::<f32>()
                                / if start { 17. } else { 18. };
                            let edge = offsets[if start { s } else { s + 9 }];
                            let edge_x = if reversed { width - edge } else { edge } as f32;
                            let group = active
                                .iter()
                                .copied()
                                .filter(|&index| {
                                    let g = &strips[index];
                                    g.start == start
                                        && g.reversed == reversed
                                        && (g.module / module - 1.).abs() < 0.3
                                })
                                .filter(|&index| {
                                    let g = &strips[index];
                                    let p = g.points.last().unwrap();
                                    let dy = y as f32 - p[1];
                                    dy > 0.
                                        && dy <= step as f32 * 3.
                                        && (p[0] - edge_x).abs() <= dy * 1.5 + module * 2.
                                })
                                .min_by(|&a, &b| {
                                    let a = &strips[a];
                                    let b = &strips[b];
                                    (a.points.last().unwrap()[0] - edge_x)
                                        .abs()
                                        .total_cmp(&(b.points.last().unwrap()[0] - edge_x).abs())
                                });
                            if let Some(index) = group {
                                let g = &mut strips[index];
                                g.points.push([edge_x, y as f32]);
                                g.module = (g.module * 3. + module) / 4.;
                            } else if strips.len() < 1024 {
                                active.push(strips.len());
                                strips.push(Strip {
                                    start,
                                    reversed,
                                    module,
                                    points: vec![[edge_x, y as f32]],
                                });
                            } else {
                                limited = true;
                            }
                        }
                    }
                }
            }
        }
        for strips in strips_by_mode {
            let fitted: Vec<_> = strips.iter().filter_map(Strip::fit).collect();
            for a in fitted.iter().filter(|g| g.start) {
                let mut paired = false;
                for b in fitted
                    .iter()
                    .filter(|g| !g.start && g.reversed == a.reversed)
                {
                    if (a.slope - b.slope).abs() > 0.12
                        || !(0.7..=1.4).contains(&(a.module / b.module))
                    {
                        continue;
                    }
                    let slope = f32::midpoint(a.slope, b.slope);
                    let norm = (1. + slope * slope).sqrt();
                    let module = f32::midpoint(a.module, b.module);
                    let left = a.intercept / norm;
                    let right = b.intercept / norm;
                    let span = (right - left) * if a.reversed { -1. } else { 1. };
                    if !(50.0..650.0).contains(&(span / module)) {
                        continue;
                    }
                    let cross = |g: &Fit, y: f32| ((g.slope * y + g.intercept) * slope + y) / norm;
                    // An oblique row must cross the entire guard. Its observed
                    // edge interval is shorter than the actual bar height.
                    let extent = |g: &Fit| {
                        let delta = g.module
                            * if g.start { 17. } else { 18. }
                            * slope
                            * if g.start == g.reversed { -1. } else { 1. };
                        (
                            cross(g, g.top) - (-delta).max(0.),
                            cross(g, g.bottom) + delta.max(0.),
                        )
                    };
                    let (at, ab) = extent(a);
                    let (bt, bb) = extent(b);
                    let overlap = ab.min(bb) - at.max(bt);
                    if overlap < (ab - at).min(bb - bt) * 0.4 {
                        continue;
                    }
                    let edge_point = |g: &Fit, cross: f32, side: f32| {
                        let y = (cross * norm - g.intercept * slope) / (1. + g.slope * slope);
                        let x = g.slope * y + g.intercept;
                        let point = [
                            x + side * module * 2. / norm,
                            y - side * module * 2. * slope / norm,
                        ];
                        original(point[0], point[1])
                    };
                    let (l, lt, lb, r, rt, rb) = if a.reversed {
                        (b, bt, bb, a, at, ab)
                    } else {
                        (a, at, ab, b, bt, bb)
                    };
                    let quad = [
                        edge_point(l, lt - step as f32, -1.),
                        edge_point(r, rt - step as f32, 1.),
                        edge_point(r, rb + step as f32, 1.),
                        edge_point(l, lb + step as f32, -1.),
                    ];
                    paired = true;
                    if out
                        .iter()
                        .any(|(p, _)| crate::regions::overlap(p, &quad) > 0.7)
                    {
                        continue;
                    }
                    if out.len() < 24 {
                        out.push((quad, a.support.min(b.support)));
                    } else {
                        limited = true;
                    }
                }
                if !paired {
                    if compact_attempts >= 64 {
                        limited = true;
                        continue;
                    }
                    compact_attempts += 1;
                    if let Some(q) = compact_quad(gray, w, h, a, step, original) {
                        // Preserve overlapping crops with different fitted
                        // axes until row metadata and correction are tested.
                        let axis = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
                        if compact_out.iter().any(|(p, _)| {
                            let other = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
                            let sine = (axis[0] * other[1] - axis[1] * other[0]).abs()
                                / (axis[0].hypot(axis[1]) * other[0].hypot(other[1]));
                            sine < 0.025 && crate::regions::overlap(p, &q) > 0.7
                        }) {
                            continue;
                        }
                        if compact_out.len() < 24 {
                            compact_out.push((q, a.support));
                        } else {
                            limited = true;
                        }
                    }
                }
            }
        }
    }
    // Fully paired guards take priority over speculative Compact rectangles.
    for item in compact_out {
        if out.len() < 24 {
            out.push(item);
        } else {
            limited = true;
        }
    }
    (out, limited)
}
#[must_use]
pub fn rectify(
    gray: &[u8],
    w: usize,
    h: usize,
    q: Quad,
) -> Option<(Vec<u8>, usize, usize, [f32; 8])> {
    let distance = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let width = distance(q[0], q[1]).ceil() as usize;
    let height = distance(q[0], q[3]).ceil() as usize;
    if width < 30 || height < 8 || width.checked_mul(height)? > 8 * 1024 * 1024 {
        return None;
    }
    let t = crate::qr_detect::homography(
        [
            [0., 0.],
            [width as f32, 0.],
            [width as f32, height as f32],
            [0., height as f32],
        ],
        q,
    )?;
    let mut pixels = vec![255; width * height];
    for y in 0..height {
        for x in 0..width {
            let [xx, yy] = crate::qr_detect::map(&t, x as f32 + 0.5, y as f32 + 0.5);
            if xx < 0. || yy < 0. || xx > w as f32 - 1. || yy > h as f32 - 1. {
                continue;
            }
            let x0 = xx.floor() as usize;
            let y0 = yy.floor() as usize;
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = xx - x0 as f32;
            let fy = yy - y0 as f32;
            pixels[y * width + x] = ((f32::from(gray[y0 * w + x0]) * (1. - fx)
                + f32::from(gray[y0 * w + x1]) * fx)
                * (1. - fy)
                + (f32::from(gray[y1 * w + x0]) * (1. - fx) + f32::from(gray[y1 * w + x1]) * fx)
                    * fy)
                .round() as u8;
        }
    }
    Some((pixels, width, height, t))
}

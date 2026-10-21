//! PDF417 guard-strip localization independent of payload row decoding.
//! Fit the repeated start/stop guards across rows, then rectify their bar axes.
use crate::linear::pattern_error;
type Quad = [[f32; 2]; 4];

// Wasm has truncation but no ties-away rounding instruction. Keep the same
// rounded integer without calling roundf for each rotated image coordinate.
fn rounded_coordinate(value: f32) -> isize {
    #[cfg(target_arch = "wasm32")]
    {
        wasm_rounded_coordinate(value)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::numeric::f32_isize(value.round())
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn wasm_rounded_coordinate(value: f32) -> isize {
    let integral = value.trunc();
    let rounded = if (value - integral).abs() >= 0.5 {
        integral + 1.0_f32.copysign(value)
    } else {
        integral
    };
    crate::numeric::f32_isize(rounded)
}
fn zero_degree_exact(w: usize, h: usize, generated_width: usize, generated_height: usize) -> bool {
    // At 0 degrees on the image's own grid, below 2^24, every rotated coordinate
    // is an exact pixel index, so the row is copied; otherwise rows are sampled.
    generated_width == w && generated_height == h && w <= (1usize << 24) && h <= (1usize << 24)
}

// Both guards begin with a wide bar followed by two one-module runs.
// The existing matcher permits at most one module of error per run: even
// the narrower stop guard needs >=6 modules versus <=2 for either neighbor.
// This necessary condition avoids a full pattern score for ordinary stripes.
fn possible_guard(runs: &[f32]) -> bool {
    runs.len() >= 3 && runs[0] >= 3. * runs[1].max(runs[2])
}
/// Guard pattern error after undoing print gain: ink spread widens the bars
/// and narrows the spaces by the same amount, so the sums of neighbouring runs
/// stay put. The best of a few gain steps counts.
fn gain_error(runs: &[f32], pattern: &[u8]) -> f32 {
    let modules = f32::from(pattern.iter().sum::<u8>());
    let module = runs[..pattern.len()].iter().sum::<f32>() / modules;
    let mut best = f32::INFINITY;
    for gain in [0.25f32, 0.5] {
        let mut adjusted = [0f32; 9];
        for (i, value) in adjusted.iter_mut().enumerate().take(pattern.len()) {
            let shift = gain * module * if i % 2 == 0 { 1. } else { -1. };
            *value = (runs[i] - shift).max(module * 0.2);
        }
        best = best.min(pattern_error(&adjusted[..pattern.len()], pattern));
    }
    best
}
/// Whether two quadrilaterals have nearly the same height, so that a taller
/// crop of the same symbol is kept as its own proposal.
fn same_extent(first: &Quad, second: &Quad) -> bool {
    let side = |quad: &Quad| (quad[3][0] - quad[0][0]).hypot(quad[3][1] - quad[0][1]);
    (side(first) / side(second).max(1e-3) - 1.).abs() < 0.04
}
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
/// Least-squares line `x = intercept + slope * y` through `[x, y]` points.
pub(crate) struct LineFit {
    pub intercept: f32,
    pub slope: f32,
    /// Sum of squared row deviations from the mean row; the slope is 0 without spread.
    pub spread: f32,
}

/// Fit `x` against `y` by least squares around the point means.
pub(crate) fn fit_line(points: impl Iterator<Item = [f32; 2]> + Clone) -> LineFit {
    let (count, sx, sy) = points
        .clone()
        .fold((0_usize, 0., 0.), |(count, sx, sy), [x, y]| {
            (count + 1, sx + x, sy + y)
        });
    let n = crate::numeric::usize_f32(count);
    let (mean_x, mean_y) = (sx / n, sy / n);
    let (covariance, spread) = points.fold((0., 0.), |(covariance, spread), [x, y]| {
        let dy = y - mean_y;
        (covariance + dy * (x - mean_x), spread + dy * dy)
    });
    let slope = if spread > 0. { covariance / spread } else { 0. };
    LineFit {
        intercept: mean_x - slope * mean_y,
        slope,
        spread,
    }
}
impl Strip {
    fn fit(&self) -> Option<Fit> {
        if self.points.len() < 4 {
            return None;
        }
        let n = crate::numeric::usize_f32(self.points.len());
        let LineFit {
            intercept,
            slope,
            spread,
        } = fit_line(self.points.iter().copied());
        if spread < 1. {
            return None;
        }
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
/// Blur and perspective shorten the matched part of a guard strip. Follow the
/// centre of the guard's wide bar along the fitted line until it turns light,
/// so the fitted ends reach the symbol's true top and bottom edges.
fn extend_fit(
    fit: &mut Fit,
    gray: &[u8],
    (width, height): (usize, usize),
    original: &impl Fn(f32, f32) -> [f32; 2],
    step: usize,
    patient: bool,
) {
    let norm = (1. + fit.slope * fit.slope).sqrt();
    let horizontal = fit.module * norm;
    let inward = if fit.start == fit.reversed { -1. } else { 1. };
    let center = |y: f32, offset: f32| {
        let [px, py] = original(
            fit.slope * y + fit.intercept + inward * offset * horizontal,
            y,
        );
        let (px, py) = (px.round(), py.round());
        (px >= 0.
            && py >= 0.
            && px < crate::numeric::usize_f32(width)
            && py < crate::numeric::usize_f32(height))
        .then(|| {
            f32::from(gray[crate::numeric::f32_usize(py) * width + crate::numeric::f32_usize(px)])
        })
    };
    let bar = if fit.start { 4. } else { 14.5 };
    let (mut dark, mut light, mut count) = (0., 0., 0.);
    let (top, bottom) = (fit.top, fit.bottom);
    let samples = 8;
    for i in 0..=samples {
        let y = top
            + (bottom - top) * crate::numeric::usize_f32(i) / crate::numeric::usize_f32(samples);
        if let (Some(on), Some(off)) = (center(y, bar), center(y, -1.5)) {
            dark += on;
            light += off;
            count += 1.;
        }
    }
    if count < 3. || (light - dark) / count < 40. {
        return;
    }
    let threshold = (light + dark) / (2. * count);
    let limit = crate::numeric::usize_f32(height.max(width));
    let walk = |from: f32, sign: f32| {
        let mut edge = from;
        let mut light_run = 0;
        let mut y = from;
        while (y - from).abs() < limit {
            y += sign * crate::numeric::usize_f32(step);
            match center(y, bar) {
                Some(v) if v <= threshold => {
                    edge = y;
                    light_run = 0;
                }
                _ => {
                    light_run += 1;
                    // The patient walk crosses the cracks that blur, grain and
                    // print wear leave in a guard bar; the plain one stops at
                    // the first gap, which keeps stacked symbols apart.
                    if light_run >= 2
                        && (!patient
                            || crate::numeric::usize_f32(light_run * step) >= fit.module * 1.5)
                    {
                        break;
                    }
                }
            }
        }
        edge
    };
    fit.top = walk(top, -1.).min(top);
    fit.bottom = walk(bottom, 1.).max(bottom);
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
) -> Option<[Quad; 2]> {
    let norm = (1. + fit.slope * fit.slope).sqrt();
    let sign = if fit.reversed { -1. } else { 1. };
    let cross = |y: f32| ((fit.slope * y + fit.intercept) * fit.slope + y) / norm;
    let delta = fit.module * 17. * fit.slope * sign;
    let top = cross(fit.top) - (-delta).max(0.) - crate::numeric::usize_f32(step);
    let bottom = cross(fit.bottom) + delta.max(0.) + crate::numeric::usize_f32(step);
    let point = |along: f32, across: f32| {
        let y = (across * norm - fit.intercept * fit.slope) / norm.powi(2);
        let x = fit.slope * y + fit.intercept;
        original(x + along * sign / norm, y - along * sign * fit.slope / norm)
    };
    let mut votes = [[0usize; 30]; 3];
    let mut modules = Vec::new();
    let samples =
        crate::numeric::f32_usize(((bottom - top) / fit.module.max(1.)).ceil().clamp(3., 270.));
    let width = crate::numeric::f32_usize((fit.module * 38.).ceil());
    if width < 38 {
        return None;
    }
    for i in 0..samples {
        let across = top
            + (crate::numeric::usize_f32(i) + 0.5) * (bottom - top)
                / crate::numeric::usize_f32(samples);
        let row: Vec<u8> = (0..width)
            .map(|x| {
                let [xx, yy] = point(crate::numeric::usize_f32(x) - fit.module * 2., across);
                let xx = crate::numeric::f32_isize(xx.round());
                let yy = crate::numeric::f32_isize(yy.round());
                if xx >= 0 && yy >= 0 && xx < (w).cast_signed() && yy < (h).cast_signed() {
                    gray[(yy).cast_unsigned() * w + (xx).cast_unsigned()]
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
    let quad = |modules: usize| {
        let end = module * crate::numeric::usize_f32(modules + 4);
        [
            point(-2. * fit.module, top),
            point(end, top),
            point(end, bottom),
            point(-2. * fit.module, bottom),
        ]
    };
    // A start guard without a matched stop guard is either a Compact symbol
    // (start, left indicator, data) or a full symbol whose stop guard blur or
    // print gain hid: offer both widths.
    Some([quad((columns + 2) * 17), quad((columns + 3) * 17 + 18)])
}
///
/// # Panics
///
/// Panics if the supplied grayscale buffer or dimensions are inconsistent.
#[expect(
    clippy::too_many_lines,
    reason = "PDF417 proposal construction preserves ordered row grouping, merging and truncation evidence within one bounded pass."
)]
pub fn proposals(gray: &[u8], w: usize, h: usize) -> (Vec<(Quad, usize)>, bool) {
    // Index 1 holds the proposals of the patient guard walk. They follow every
    // plain proposal, so a taller crop never decodes before the symbol it
    // might otherwise swallow.
    let mut out: [Vec<(Quad, usize)>; 2] = [Vec::new(), Vec::new()];
    let mut compact_out: [Vec<(Quad, usize)>; 2] = [Vec::new(), Vec::new()];
    let mut limited = false;
    let mut compact_attempts = [0usize; 2];
    for degrees in [0f32, 90., 45., -45.] {
        let angle = degrees.to_radians();
        let cs = angle.cos();
        let sn = angle.sin();
        let corners = [
            [0., 0.],
            [crate::numeric::usize_f32(w) - 1., 0.],
            [
                crate::numeric::usize_f32(w) - 1.,
                crate::numeric::usize_f32(h) - 1.,
            ],
            [0., crate::numeric::usize_f32(h) - 1.],
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
            crate::numeric::f32_usize((amax - amin).ceil()) + 1,
            crate::numeric::f32_usize((bmax - bmin).ceil()) + 1,
        );
        let original = |x: f32, y: f32| {
            [
                (x + amin) * cs - (y + bmin) * sn,
                (x + amin) * sn + (y + bmin) * cs,
            ]
        };
        let step = (height / 400).max(1);
        // Slots 0 and 1 hold guards that match as printed, one per threshold
        // mode. Slots 2 and 3 hold guards that match only after undoing print
        // gain; they never mix with exact ones, so gain tolerance cannot
        // disturb a symbol that localizes without it.
        let mut strips_by_mode: [Vec<Strip>; 4] = std::array::from_fn(|_| Vec::new());
        let mut active_by_mode: [Vec<usize>; 4] = std::array::from_fn(|_| Vec::new());
        // `threshold` is a pure function of one sampled row and mode. Cache
        // the preceding row's two bitmaps so identical adjacent samples do
        // not repeat preprocessing; all row, mode, reverse, and association
        // order remains unchanged.
        let mut previous_row = Vec::new();
        let mut previous_bits: [Vec<bool>; 2] = [Vec::new(), Vec::new()];
        let mut row = Vec::with_capacity(width);
        let mut run_buffers: [(Vec<f32>, Vec<usize>); 2] =
            std::array::from_fn(|_| (Vec::new(), Vec::new()));
        // The rotated coordinates split into column and row products, computed
        // once each; every sample subtracts and adds exactly these f32 values.
        let direct_rows = degrees == 0. && zero_degree_exact(w, h, width, height);
        let columns: Vec<[f32; 2]> = if direct_rows {
            Vec::new()
        } else {
            (0..width)
                .map(|x| {
                    let x = crate::numeric::usize_f32(x) + amin;
                    [x * cs, x * sn]
                })
                .collect()
        };
        for y in (0..height).step_by(step) {
            row.clear();
            if direct_rows {
                row.extend_from_slice(&gray[y * w..(y + 1) * w]);
            } else {
                let along = crate::numeric::usize_f32(y) + bmin;
                let (row_x, row_y) = (along * sn, along * cs);
                row.extend(columns.iter().map(|&[column_x, column_y]| {
                    let xx = rounded_coordinate(column_x - row_x);
                    let yy = rounded_coordinate(column_y + row_y);
                    if xx >= 0 && yy >= 0 && xx < (w).cast_signed() && yy < (h).cast_signed() {
                        gray[(yy).cast_unsigned() * w + (xx).cast_unsigned()]
                    } else {
                        255
                    }
                }));
            }
            if row.iter().all(|&value| value == row[0]) {
                continue;
            }
            let same_row = previous_row == row;
            for mode in 0..2 {
                // A completed strip can never match a later row again. Keep
                // it for fitting, but omit it from future association searches.
                for slot in [mode, mode + 2] {
                    let strips = &strips_by_mode[slot];
                    active_by_mode[slot].retain(|&index| {
                        crate::numeric::usize_f32(y) - strips[index].points.last().unwrap()[1]
                            <= crate::numeric::usize_f32(step) * 3.
                    });
                }
                if !same_row {
                    crate::threshold_into(&row, mode, &mut previous_bits[mode]);
                }
                let bits = &previous_bits[mode];
                let (runs, offsets) = &mut run_buffers[mode];
                crate::runs_into(bits, runs, offsets);
                for reversed in [false, true] {
                    if reversed {
                        crate::reverse_runs(runs, offsets, width);
                    }
                    let first_black = bits[if reversed { width - 1 } else { 0 }];
                    for s in (usize::from(!first_black)..runs.len().saturating_sub(8)).step_by(2) {
                        if !possible_guard(&runs[s..]) {
                            continue;
                        }
                        for start in [true, false] {
                            let pattern: &[u8] = if start {
                                &[8, 1, 1, 1, 1, 1, 1, 3]
                            } else {
                                &[7, 1, 1, 3, 1, 1, 1, 2, 1]
                            };
                            if s + pattern.len() > runs.len() {
                                continue;
                            }
                            let slot = if pattern_error(&runs[s..], pattern) <= 0.12 {
                                mode
                            } else if gain_error(&runs[s..], pattern) <= 0.12 {
                                mode + 2
                            } else {
                                continue;
                            };
                            let strips = &mut strips_by_mode[slot];
                            let active = &mut active_by_mode[slot];
                            let module = runs[s..s + pattern.len()].iter().sum::<f32>()
                                / if start { 17. } else { 18. };
                            let edge = offsets[if start { s } else { s + 9 }];
                            let edge_x = crate::numeric::usize_f32(if reversed {
                                width - edge
                            } else {
                                edge
                            });
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
                                    let dy = crate::numeric::usize_f32(y) - p[1];
                                    dy > 0.
                                        && dy <= crate::numeric::usize_f32(step) * 3.
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
                                g.points.push([edge_x, crate::numeric::usize_f32(y)]);
                                g.module = (g.module * 3. + module) / 4.;
                            } else if strips.len() < 1024 {
                                active.push(strips.len());
                                strips.push(Strip {
                                    start,
                                    reversed,
                                    module,
                                    points: vec![[edge_x, crate::numeric::usize_f32(y)]],
                                });
                            } else {
                                limited = true;
                            }
                        }
                    }
                }
            }
            std::mem::swap(&mut row, &mut previous_row);
        }
        for (strips, patient) in strips_by_mode
            .iter()
            .flat_map(|strips| [(strips, false), (strips, true)])
        {
            let which = usize::from(patient);
            let mut fitted: Vec<_> = strips.iter().filter_map(Strip::fit).collect();
            for fit in &mut fitted {
                extend_fit(fit, gray, (w, h), &original, step, patient);
            }
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
                        edge_point(l, lt - crate::numeric::usize_f32(step), -1.),
                        edge_point(r, rt - crate::numeric::usize_f32(step), 1.),
                        edge_point(r, rb + crate::numeric::usize_f32(step), 1.),
                        edge_point(l, lb + crate::numeric::usize_f32(step), -1.),
                    ];
                    paired = true;
                    if out.iter().flatten().any(|(p, _)| {
                        crate::regions::overlap(p, &quad) > 0.7
                            && (!patient || same_extent(p, &quad))
                    }) {
                        continue;
                    }
                    if out[which].len() < 96 {
                        out[which].push((quad, a.support.min(b.support)));
                    } else {
                        limited = true;
                    }
                }
                if !paired {
                    if compact_attempts[which] >= 64 {
                        limited = true;
                        continue;
                    }
                    compact_attempts[which] += 1;
                    if let Some(quads) = compact_quad(gray, w, h, a, step, original) {
                        for q in quads {
                            // Preserve overlapping crops with different fitted
                            // axes until row metadata and correction are tested.
                            let axis = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
                            if compact_out.iter().flatten().any(|(p, _)| {
                                let other = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
                                let sine = (axis[0] * other[1] - axis[1] * other[0]).abs()
                                    / (axis[0].hypot(axis[1]) * other[0].hypot(other[1]));
                                let length = axis[0].hypot(axis[1]);
                                let other_length = other[0].hypot(other[1]);
                                sine < 0.025
                                    && (length / other_length - 1.).abs() < 0.03
                                    && same_extent(p, &q)
                                    && crate::regions::overlap(p, &q) > 0.7
                            }) {
                                continue;
                            }
                            if compact_out[which].len() < 96 {
                                compact_out[which].push((q, a.support));
                            } else {
                                limited = true;
                            }
                        }
                    }
                }
            }
        }
    }
    // Fully paired guards take priority over speculative Compact rectangles.
    let mut ordered = Vec::new();
    let [out_plain, out_patient] = out;
    let [compact_plain, compact_patient] = compact_out;
    for item in out_plain
        .into_iter()
        .chain(compact_plain)
        .chain(out_patient)
        .chain(compact_patient)
    {
        if ordered.len() < 96 {
            ordered.push(item);
        } else {
            limited = true;
        }
    }
    (ordered, limited)
}

#[must_use]
pub fn rectify(
    gray: &[u8],
    w: usize,
    h: usize,
    q: Quad,
) -> Option<(Vec<u8>, usize, usize, [f32; 8])> {
    let distance = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    let width = crate::numeric::f32_usize(distance(q[0], q[1]).ceil());
    let height = crate::numeric::f32_usize(distance(q[0], q[3]).ceil());
    if width < 30 || height < 8 || width.checked_mul(height)? > 8 * 1024 * 1024 {
        return None;
    }
    let t = crate::qr_detect::homography(
        [
            [0., 0.],
            [crate::numeric::usize_f32(width), 0.],
            [
                crate::numeric::usize_f32(width),
                crate::numeric::usize_f32(height),
            ],
            [0., crate::numeric::usize_f32(height)],
        ],
        q,
    )?;
    let mut pixels = vec![255; width * height];
    for y in 0..height {
        for x in 0..width {
            let [xx, yy] = crate::qr_detect::map(
                &t,
                crate::numeric::usize_f32(x) + 0.5,
                crate::numeric::usize_f32(y) + 0.5,
            );
            if xx < 0.
                || yy < 0.
                || xx > crate::numeric::usize_f32(w) - 1.
                || yy > crate::numeric::usize_f32(h) - 1.
            {
                continue;
            }
            let x0 = crate::numeric::f32_usize(xx.floor());
            let y0 = crate::numeric::f32_usize(yy.floor());
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let fx = xx - crate::numeric::usize_f32(x0);
            let fy = yy - crate::numeric::usize_f32(y0);
            pixels[y * width + x] = crate::numeric::f32_u8(
                ((f32::from(gray[y0 * w + x0]) * (1. - fx) + f32::from(gray[y0 * w + x1]) * fx)
                    * (1. - fy)
                    + (f32::from(gray[y1 * w + x0]) * (1. - fx)
                        + f32::from(gray[y1 * w + x1]) * fx)
                        * fy)
                    .round(),
            );
        }
    }
    Some((pixels, width, height, t))
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "Bounded test generator creates positive integer run widths and deliberately converts them to the production f32 representation."
    )]
    fn cheap_guard_check_retains_accepted_patterns() {
        let mut seed = 193_u32;
        let mut accepted = 0;
        for pattern in [
            &[8, 1, 1, 1, 1, 1, 1, 3][..],
            &[7, 1, 1, 3, 1, 1, 1, 2, 1][..],
        ] {
            for module in 1..=12 {
                for _ in 0..1000 {
                    let runs: Vec<f32> = pattern
                        .iter()
                        .map(|&n| {
                            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            (n * module + (seed % 5) as i32 - 2).max(1) as f32
                        })
                        .collect();
                    if pattern_error(&runs, &pattern.iter().map(|&v| v as u8).collect::<Vec<_>>())
                        <= 0.12
                    {
                        accepted += 1;
                        assert!(possible_guard(&runs));
                    }
                }
            }
        }
        assert!(accepted > 1000);
        assert!(!possible_guard(&[2., 2., 2., 2.]));
    }
}

#[cfg(test)]
mod axial_sampling_tests {
    use super::zero_degree_exact;

    #[test]
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Reference sampler reproduces old f32 roundtrip on bounded positive test coordinates."
    )]
    fn zero_degree_direct_row_matches_old_integer_affine_sampling() {
        for &(w, h) in &[(1, 1), (2, 3), (40, 41), (401, 257)] {
            let gray: Vec<u8> = (0..w * h).map(|i| u8::try_from(i % 251).unwrap()).collect();
            let y = h / 2;
            let old: Vec<u8> = (0..w)
                .map(|x| {
                    let xx = (x as f32).round() as usize;
                    let yy = (y as f32).round() as usize;
                    gray[yy * w + xx]
                })
                .collect();
            let direct = gray[y * w..(y + 1) * w].to_vec();
            assert_eq!(direct, old);
            assert!(zero_degree_exact(w, h, w, h));
        }
    }

    #[test]
    fn zero_degree_guard_rejects_unrepresentable_f32_dimensions() {
        let exact = 1usize << 24;
        assert!(zero_degree_exact(exact, 1, exact, 1));
        assert!(!zero_degree_exact(exact + 1, 1, exact + 1, 1));
        assert!(!zero_degree_exact(exact + 2, 1, exact + 2, 1));
        assert!(zero_degree_exact(exact, 1, exact, 1));
        assert!(!zero_degree_exact(exact + 1, 1, exact + 1, 1));
        assert!(!zero_degree_exact(exact, 1, exact + 1, 1));
    }
}

#[cfg(test)]
mod rounding_tests {
    use super::{rounded_coordinate, wasm_rounded_coordinate};

    #[test]
    fn rotated_coordinates_match_ties_away_rounding() {
        let check = |value: f32| {
            assert_eq!(
                wasm_rounded_coordinate(value),
                crate::numeric::f32_isize(value.round()),
                "{value:?}"
            );
            assert_eq!(
                rounded_coordinate(value),
                crate::numeric::f32_isize(value.round())
            );
        };
        for integer in -4096..=4096 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "The bounded test integers are exactly representable."
            )]
            let boundary = integer as f32 + 0.5;
            check(boundary);
            check(boundary.next_down());
            check(boundary.next_up());
        }
        for bits in [
            0,
            1,
            0x007f_ffff,
            0x8000_0000,
            0x8000_0001,
            0x807f_ffff,
            0x7f80_0001,
            0xff80_0001,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            0xffc0_0000,
            0x7f7f_ffff,
            0xff7f_ffff,
        ] {
            check(f32::from_bits(bits));
        }
        for power in [22, 23, 24, 31, 32, 63, 64] {
            for sign in [-1., 1.] {
                let value = sign * 2.0_f32.powi(power);
                check(value.next_down());
                check(value);
                check(value.next_up());
            }
        }
        // Cover exponents, signs and mantissas, including conversion saturation.
        let mut bits = 0x7361_6d70_u32;
        for _ in 0..200_000 {
            bits = bits.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            check(f32::from_bits(bits));
        }
    }
}

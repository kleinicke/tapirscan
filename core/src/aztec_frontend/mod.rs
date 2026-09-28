//! Bullseye localization and independent Aztec grid sampling.
use crate::qr_frontend::binarization;
use barcode_multiformat::{qr_detect, Detection};
use decoder as aztec;
mod center_index;
mod curves;
pub mod decoder;
mod geometry;
mod refinement;
#[derive(Clone)]
struct Center {
    x: f32,
    y: f32,
    module: f32,
    support: usize,
}
fn cross(
    bits: &impl Fn(usize) -> bool,
    w: usize,
    h: usize,
    x: usize,
    y: usize,
    vertical: bool,
    expected: f32,
) -> Option<(f32, f32)> {
    if !bits(y * w + x) {
        return None;
    }
    let limit = if vertical { h } else { w };
    let pixel = |at: usize| {
        if vertical {
            bits(at * w + x)
        } else {
            bits(y * w + at)
        }
    };
    let mut start = if vertical { y } else { x };
    let mut end = start + 1;
    // Accepted cross scales are at most 1.7 times the row scale, and each
    // accepted run is at most 1.7 times that cross scale. Larger runs cannot
    // pass the existing checks; stop before following them across the image.
    let maximum = barcode_multiformat::numeric::f32_usize((expected * 2.9).ceil());
    while start > 0 && pixel(start - 1) {
        start -= 1;
        if end - start > maximum {
            return None;
        }
    }
    while end < limit && pixel(end) {
        end += 1;
        if end - start > maximum {
            return None;
        }
    }
    let center = barcode_multiformat::numeric::usize_f32(start + end) / 2.;
    let mut widths = [0.; 7];
    widths[0] = barcode_multiformat::numeric::usize_f32(end - start);
    for ring in 1..=3 {
        let expect = ring % 2 == 0;
        let old = start;
        while start > 0 && pixel(start - 1) == expect {
            start -= 1;
            if old - start > maximum {
                return None;
            }
        }
        widths[2 * ring - 1] = barcode_multiformat::numeric::usize_f32(old - start);
        let old = end;
        while end < limit && pixel(end) == expect {
            end += 1;
            if end - old > maximum {
                return None;
            }
        }
        widths[2 * ring] = barcode_multiformat::numeric::usize_f32(end - old);
    }
    let module = widths[..7].iter().sum::<f32>() / 7.;
    if module < 0.8
        || widths[..7]
            .iter()
            .any(|v| (*v - module).abs() > module * 0.7)
    {
        return None;
    }
    Some((center, module))
}
fn centers_with_rows(bits: &[bool], w: usize, h: usize, row_cache: &RowCache<'_>) -> Vec<Center> {
    centers_from_runs(w, h, &|i| bits[i], |y, out| {
        row_cache(y, &bits[y * w..(y + 1) * w], out);
    })
}
fn centers_gray(gray: &[u8], w: usize, h: usize, cut: u8) -> Vec<Center> {
    let mut row = vec![false; w];
    centers_from_runs(w, h, &|i| gray[i] <= cut, |y, out| {
        for (target, &value) in row.iter_mut().zip(&gray[y * w..(y + 1) * w]) {
            *target = value <= cut;
        }
        crate::qr_frontend::transition_offsets_into(&row, out);
    })
}
fn centers_from_runs(
    w: usize,
    h: usize,
    bits: &impl Fn(usize) -> bool,
    mut copy_rows: impl FnMut(usize, &mut Vec<usize>),
) -> Vec<Center> {
    let mut found = center_index::Index::new(w, h);
    let mut refined = center_index::Index::new(w, h);
    let step = (h / 600).max(1);
    let mut offsets = Vec::new();
    let mut runs = Vec::new();
    for y in (0..h).step_by(step) {
        copy_rows(y, &mut offsets);
        runs.clear();
        #[expect(
            clippy::cast_precision_loss,
            reason = "This preserves the existing usize-to-f32 run-width conversion after exact transition extraction."
        )]
        let widths = offsets.windows(2).map(|pair| (pair[1] - pair[0]) as f32);
        runs.extend(widths);
        for i in 0..runs.len().saturating_sub(8) {
            if !bits(y * w + offsets[i]) {
                continue;
            }
            let module = runs[i + 1..i + 8].iter().sum::<f32>() / 7.;
            if module < 0.8
                || runs[i + 1..i + 8]
                    .iter()
                    .any(|v| (*v - module).abs() > module * 0.7)
            {
                continue;
            }
            let x = offsets[i + 4] + barcode_multiformat::numeric::f32_usize(runs[i + 4]) / 2;
            let Some((cy, my)) = cross(bits, w, h, x, y, true, module) else {
                continue;
            };
            if my / module < 0.6 || my / module > 1.7 {
                continue;
            }
            found.insert(
                barcode_multiformat::numeric::usize_f32(offsets[i + 4]) + runs[i + 4] * 0.5,
                cy,
                (my + module) * 0.5,
                module * 1.5,
            );
            let Some((cx, mx)) = cross(
                bits,
                w,
                h,
                x,
                barcode_multiformat::numeric::f32_usize(
                    cy.floor()
                        .min(barcode_multiformat::numeric::usize_f32(h - 1)),
                ),
                false,
                my,
            ) else {
                continue;
            };
            if !(0.6..=1.7).contains(&(mx / my)) {
                continue;
            }
            let module = (mx + my) * 0.5;
            refined.insert(cx, cy, module, module * 1.5);
        }
    }
    let mut found = found.centers;
    for c in refined.centers {
        if !found.iter().any(|old| {
            (old.x - c.x).hypot(old.y - c.y) < c.module * 0.2
                && (old.module / c.module - 1.).abs() < 0.05
        }) {
            found.push(c);
        }
    }
    found.retain(|c| {
        has_two_directions(bits, w, h, c.x, c.y, c.module * 5.)
            && has_oblique_rings(bits, w, h, c, 3)
    });
    found.sort_by_key(|c| std::cmp::Reverse(c.support));
    found
}
// Parallel stripes can satisfy both axis scans at 45 degrees. A bullseye also
// crosses several rings in both diagonal directions, independent of its pose.
fn has_two_directions(
    bits: &impl Fn(usize) -> bool,
    width: usize,
    height: usize,
    x: f32,
    y: f32,
    radius: f32,
) -> bool {
    if width < 3 || height < 3 {
        return true;
    }
    let left =
        (barcode_multiformat::numeric::f32_isize((x - radius).floor()).max(1)).cast_unsigned();
    let right = (barcode_multiformat::numeric::f32_isize((x + radius).ceil())
        .clamp(1, (width).cast_signed() - 1))
    .cast_unsigned();
    let top =
        (barcode_multiformat::numeric::f32_isize((y - radius).floor()).max(1)).cast_unsigned();
    let bottom = (barcode_multiformat::numeric::f32_isize((y + radius).ceil())
        .clamp(1, (height).cast_signed() - 1))
    .cast_unsigned();
    let step = (barcode_multiformat::numeric::f32_usize(radius) / 12).max(1);
    let (mut xx, mut yy, mut xy) = (0_i32, 0_i32, 0_i32);
    for y in (top..bottom).step_by(step) {
        for x in (left..right).step_by(step) {
            let at = y * width + x;
            let dx = i32::from(bits(at + 1)) - i32::from(bits(at - 1));
            let dy = i32::from(bits(at + width)) - i32::from(bits(at - width));
            xx += dx * dx;
            yy += dy * dy;
            xy += dx * dy;
        }
    }
    let energy = barcode_multiformat::numeric::f64_f32(f64::from(xx + yy));
    energy < 12.
        || barcode_multiformat::numeric::f64_f32(f64::from(xx - yy))
            .hypot(barcode_multiformat::numeric::f64_f32(f64::from(2 * xy)))
            < energy * 0.90
}
fn has_oblique_rings(
    bits: &impl Fn(usize) -> bool,
    w: usize,
    h: usize,
    center: &Center,
    required: usize,
) -> bool {
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    for (dx, dy) in [(diagonal, diagonal), (-diagonal, diagonal)] {
        for sign in [-1., 1.] {
            let mut previous = None;
            let mut transitions = 0;
            for step in 0..=28 {
                let distance = barcode_multiformat::numeric::f64_f32(f64::from(step))
                    * center.module
                    * 0.25
                    * sign;
                let x = barcode_multiformat::numeric::f32_isize((center.x + dx * distance).floor());
                let y = barcode_multiformat::numeric::f32_isize((center.y + dy * distance).floor());
                if x < 0 || y < 0 || x >= (w).cast_signed() || y >= (h).cast_signed() {
                    break;
                }
                let bit = bits((y).cast_unsigned() * w + (x).cast_unsigned());
                if previous.is_some_and(|p| p != bit) {
                    transitions += 1;
                }
                previous = Some(bit);
                if transitions >= required {
                    break;
                }
            }
            if transitions < required {
                return false;
            }
        }
    }
    true
}
fn transform(c: &Center, theta: f32, module: f32, n: usize) -> [f32; 8] {
    let a = theta.cos() * module;
    let b = theta.sin() * module;
    let half = barcode_multiformat::numeric::usize_f32(n) / 2.;
    [
        a,
        -b,
        c.x - half * (a - b),
        b,
        a,
        c.y - half * (a + b),
        0.,
        0.,
    ]
}
fn rotate(matrix: &[bool], n: usize, turn: usize, mirror: bool) -> Vec<bool> {
    (0..n * n)
        .map(|i| {
            let mut x = i % n;
            let mut y = i / n;
            if mirror {
                std::mem::swap(&mut x, &mut y);
            }
            for _ in 0..turn {
                (x, y) = (n - 1 - y, x);
            }
            matrix[y * n + x]
        })
        .collect()
}
fn template_error(bits: &[bool], w: usize, h: usize, t: &[f32; 8], limit: usize) -> usize {
    let mut errors = 0;
    for y in 0_i32..9 {
        for x in 0_i32..9 {
            let [px, py] = qr_detect::map(
                t,
                barcode_multiformat::numeric::f64_f32(f64::from(x)) + 0.5,
                barcode_multiformat::numeric::f64_f32(f64::from(y)) + 0.5,
            );
            if px < 0.
                || py < 0.
                || px >= barcode_multiformat::numeric::usize_f32(w)
                || py >= barcode_multiformat::numeric::usize_f32(h)
            {
                return 81;
            }
            let ring = (x - 4).abs().max((y - 4).abs());
            errors += usize::from(
                bits[barcode_multiformat::numeric::f32_usize(py) * w
                    + barcode_multiformat::numeric::f32_usize(px)]
                    != (ring % 2 == 0),
            );
            if errors > limit {
                return errors;
            }
        }
    }
    errors
}
fn ring_error(bits: &[bool], w: usize, h: usize, c: &Center, theta: f32, module: f32) -> usize {
    template_error(bits, w, h, &transform(c, theta, module, 9), 81)
}
fn centered(t: &[f32; 8], n: usize) -> [f32; 8] {
    let half = barcode_multiformat::numeric::usize_f32(n) * 0.5;
    let d = 1. - half * (t[6] + t[7]);
    [
        t[0] / d,
        t[1] / d,
        (t[2] - half * (t[0] + t[1])) / d,
        t[3] / d,
        t[4] / d,
        (t[5] - half * (t[3] + t[4])) / d,
        t[6] / d,
        t[7] / d,
    ]
}
fn ring_quads(
    bits: &[bool],
    size: [usize; 2],
    center: &Center,
    theta: f32,
    module: f32,
    ring: usize,
    limited: &mut bool,
) -> Option<Vec<[f32; 8]>> {
    let [w, h] = size;
    let x = barcode_multiformat::numeric::f32_isize(
        (center.x + theta.cos() * barcode_multiformat::numeric::usize_f32(ring) * module).floor(),
    );
    let y = barcode_multiformat::numeric::f32_isize(
        (center.y + theta.sin() * barcode_multiformat::numeric::usize_f32(ring) * module).floor(),
    );
    let color = ring.is_multiple_of(2);
    if x < 0
        || y < 0
        || x >= (w).cast_signed()
        || y >= (h).cast_signed()
        || bits[(y).cast_unsigned() * w + (x).cast_unsigned()] != color
    {
        return None;
    }
    let radius = barcode_multiformat::numeric::f32_isize(
        (module * barcode_multiformat::numeric::usize_f32(ring + 2)).ceil(),
    ) + 2;
    let left = (barcode_multiformat::numeric::f32_isize(center.x.floor()) - radius).max(0);
    let top = (barcode_multiformat::numeric::f32_isize(center.y.floor()) - radius).max(0);
    let right =
        (barcode_multiformat::numeric::f32_isize(center.x.ceil()) + radius).min((w).cast_signed());
    let bottom =
        (barcode_multiformat::numeric::f32_isize(center.y.ceil()) + radius).min((h).cast_signed());
    let expected_area = module * module * 8. * barcode_multiformat::numeric::usize_f32(ring);
    let quads = geometry::quads(
        bits,
        w,
        [(x).cast_unsigned(), (y).cast_unsigned()],
        [
            (left).cast_unsigned(),
            (top).cast_unsigned(),
            (right).cast_unsigned(),
            (bottom).cast_unsigned(),
        ],
        [expected_area * 0.5, expected_area * 1.75],
        limited,
    )?;
    let half = barcode_multiformat::numeric::usize_f32(ring) + 0.5;
    Some(
        quads
            .into_iter()
            .filter_map(|quad| {
                qr_detect::homography(
                    [[-half, -half], [half, -half], [half, half], [-half, half]],
                    quad,
                )
            })
            .collect(),
    )
}
// Full Aztec symbols have alternating reference tracks every sixteen modules.
// These tracks constrain sampling away from the small central finder, where a
// slight projective error otherwise grows across a large symbol.
fn reference_points(n: usize) -> &'static [(f32, f32, bool)] {
    type Points = Vec<(f32, f32, bool)>;
    static GRIDS: [std::sync::OnceLock<Points>; 76] = [const { std::sync::OnceLock::new() }; 76];
    GRIDS[n / 2].get_or_init(|| {
        let center = (n / 2).cast_signed();
        let mut points = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let xx = (x).cast_signed() - center;
                let yy = (y).cast_signed() - center;
                let expected = if xx.abs().max(yy.abs()) <= 4 {
                    xx.abs().max(yy.abs()) % 2 == 0
                } else if xx.abs().max(yy.abs()) <= 7 {
                    continue;
                } else if xx % 16 == 0 {
                    yy % 2 == 0
                } else if yy % 16 == 0 {
                    xx % 2 == 0
                } else {
                    continue;
                };
                points.push((
                    barcode_multiformat::numeric::usize_f32(x) + 0.5,
                    barcode_multiformat::numeric::usize_f32(y) + 0.5,
                    expected,
                ));
            }
        }
        points
    })
}
fn reference_error(bits: &[bool], w: usize, h: usize, t: &[f32; 8], n: usize) -> f32 {
    let points = reference_points(n);
    let mut error = 0.;
    for &(x, y, expected) in points {
        let [px, py] = qr_detect::map(t, x, y);
        let px = px - 0.5;
        let py = py - 0.5;
        let ix = barcode_multiformat::numeric::f32_isize(px.floor());
        let iy = barcode_multiformat::numeric::f32_isize(py.floor());
        if ix < 0 || iy < 0 || ix + 1 >= (w).cast_signed() || iy + 1 >= (h).cast_signed() {
            error += 1.;
            continue;
        }
        let dx = px - barcode_multiformat::numeric::isize_f32(ix);
        let dy = py - barcode_multiformat::numeric::isize_f32(iy);
        let at = (iy).cast_unsigned() * w + (ix).cast_unsigned();
        let value = (f32::from(u8::from(bits[at])) * (1. - dx)
            + f32::from(u8::from(bits[at + 1])) * dx)
            * (1. - dy)
            + (f32::from(u8::from(bits[at + w])) * (1. - dx)
                + f32::from(u8::from(bits[at + w + 1])) * dx)
                * dy;
        error += if expected { 1. - value } else { value };
    }
    error / barcode_multiformat::numeric::usize_f32(points.len())
}
fn refine_reference(
    bits: &[bool],
    w: usize,
    h: usize,
    t: [f32; 8],
    n: usize,
    module: f32,
) -> [f32; 8] {
    let source = [
        [0., 0.],
        [barcode_multiformat::numeric::usize_f32(n), 0.],
        [
            barcode_multiformat::numeric::usize_f32(n),
            barcode_multiformat::numeric::usize_f32(n),
        ],
        [0., barcode_multiformat::numeric::usize_f32(n)],
    ];
    let mut quad = source.map(|[x, y]| qr_detect::map(&t, x, y));
    let mut best = t;
    let mut error = reference_error(bits, w, h, &best, n);
    for scale in [0.8, 0.4, 0.2, 0.1] {
        for _ in 0..3 {
            let mut changed = false;
            for corner in 0..4 {
                for axis in 0..2 {
                    for sign in [-1., 1.] {
                        let mut trial = quad;
                        trial[corner][axis] += module * scale * sign;
                        let Some(t) = qr_detect::homography(source, trial) else {
                            continue;
                        };
                        let score = reference_error(bits, w, h, &t, n);
                        if score + 0.0001 < error {
                            error = score;
                            quad = trial;
                            best = t;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
    best
}
type RowCache<'a> = dyn Fn(usize, &[bool], &mut Vec<usize>) + 'a;

/// Threshold pixels and source grayscale share source-image coordinates.
#[derive(Clone, Copy)]
pub struct Threshold<'a> {
    pub bits: &'a [bool],
    pub gray: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub mode: usize,
}
/// Shared Aztec state across threshold passes for one reader invocation.
/// The facade may invoke the reader again within its own bounded recovery stage.
pub struct Search {
    pub reads: Vec<Detection>,
    pub limited: bool,
    refine_budget: usize,
    curve_budget: usize,
    bow_budget: usize,
    rune_gray_budget: usize,
    adaptive_budget: usize,
}
impl Default for Search {
    fn default() -> Self {
        Self {
            reads: Vec::new(),
            limited: false,
            refine_budget: if cfg!(feature = "mode-low") {
                10_000
            } else {
                100_000
            },
            curve_budget: if cfg!(feature = "mode-low") {
                20_000
            } else {
                150_000
            },
            bow_budget: if cfg!(feature = "mode-low") {
                0
            } else {
                50_000
            },
            rune_gray_budget: 8_192,
            adaptive_budget: if cfg!(feature = "mode-low") {
                0
            } else {
                50_000
            },
        }
    }
}
impl Search {
    pub fn threshold(
        &mut self,
        input: Threshold<'_>,
        regions: &mut barcode_multiformat::regions::Regions,
        row_cache: &RowCache<'_>,
    ) {
        self.threshold_candidates(input, regions, row_cache, None);
    }
    #[expect(
        clippy::too_many_lines,
        reason = "Keeps the ordered, bounded candidate hypotheses together; their order affects recovery and multi-symbol coverage."
    )]
    fn threshold_candidates(
        &mut self,
        input: Threshold<'_>,
        regions: &mut barcode_multiformat::regions::Regions,
        row_cache: &RowCache<'_>,
        prepared: Option<Vec<Center>>,
    ) {
        let Threshold {
            bits,
            gray,
            width: w,
            height: h,
            mode,
        } = input;
        let mut results = std::mem::take(&mut self.reads);
        let mut limited = self.limited;
        let mut refine_budget = self.refine_budget;
        let mut candidates = prepared.unwrap_or_else(|| centers_with_rows(bits, w, h, row_cache));
        candidates.sort_by_cached_key(|c| {
            let error = (0..90)
                .step_by(15)
                .map(|a| {
                    let theta = barcode_multiformat::numeric::f64_f32(f64::from(a)).to_radians();
                    ring_error(
                        bits,
                        w,
                        h,
                        c,
                        theta,
                        c.module * theta.cos().abs().max(theta.sin().abs()),
                    )
                })
                .min()
                .unwrap_or(81);
            (error, std::cmp::Reverse(c.support))
        });
        if candidates.len() > 64 {
            limited = true;
            candidates.truncate(64);
        }
        'candidate: for c in candidates {
            if results.iter().any(|d| {
                let p = d
                    .polygon
                    .iter()
                    .fold([0., 0.], |a, p| [a[0] + p[0] * 0.25, a[1] + p[1] * 0.25]);
                (p[0] - c.x).hypot(p[1] - c.y) < c.module * 3.
            }) {
                continue;
            }
            // A connected inner ring supplies projective geometry even when a
            // square, uniform-scale template cannot fit a perspective view.
            let mut transforms = Vec::new();
            for degrees in [0_f32, 30., 60.] {
                let theta = degrees.to_radians();
                for scale in [1., 0.8, 1.2] {
                    let module = c.module * theta.cos().abs().max(theta.sin().abs()) * scale;
                    for ring in [3, 5, 2] {
                        for t in ring_quads(bits, [w, h], &c, theta, module, ring, &mut limited)
                            .into_iter()
                            .flatten()
                        {
                            let error = template_error(bits, w, h, &centered(&t, 9), 8);
                            if error <= 8
                                && !transforms.iter().any(|previous: &[f32; 8]| {
                                    [[-4., -4.], [4., -4.], [4., 4.], [-4., 4.]].iter().all(
                                        |&[x, y]| {
                                            let a = qr_detect::map(previous, x, y);
                                            let b = qr_detect::map(&t, x, y);
                                            (a[0] - b[0]).hypot(a[1] - b[1]) < module * 0.3
                                        },
                                    )
                                })
                            {
                                transforms.push(t);
                            }
                        }
                    }
                }
            }
            let mut grids = Vec::new();
            for degrees in (0..90).step_by(3) {
                let theta = barcode_multiformat::numeric::f64_f32(f64::from(degrees)).to_radians();
                for scale in [1., 0.9, 1.1] {
                    let module = c.module * theta.cos().abs().max(theta.sin().abs()) * scale;
                    let error = template_error(bits, w, h, &transform(&c, theta, module, 9), 12);
                    if error <= 12 {
                        grids.push((error, theta, module));
                    }
                }
            }
            grids.sort_by_key(|g| g.0);
            limited |= grids.len() > 12;
            for (_, theta, module) in grids.into_iter().take(12) {
                transforms.push(transform(&c, theta, module, 0));
                for t in ring_quads(bits, [w, h], &c, theta, module, 2, &mut limited)
                    .into_iter()
                    .flatten()
                {
                    transforms.push(t);
                }
            }
            let original_transforms = transforms.len();
            for scale in [0.975, 1.025, 0.95, 1.05, 0.92, 1.08] {
                for index in 0..original_transforms {
                    let mut t = transforms[index];
                    for coefficient in [0, 1, 3, 4, 6, 7] {
                        t[coefficient] *= scale;
                    }
                    if template_error(bits, w, h, &centered(&t, 9), 8) <= 8 {
                        transforms.push(t);
                    }
                }
            }
            let mut reference_retries = Vec::new();
            let mut curve_seeds = Vec::new();
            let mut pending_rune = None;
            let mut refine_seeds = Vec::new();
            let mut next_transform = 0;
            let original_count = transforms.len();
            loop {
                if next_transform == original_count {
                    let mut ranked: Vec<_> = transforms
                        .iter()
                        .take(12)
                        .map(|t| (refinement::score(gray, w, h, t, mode), *t))
                        .collect();
                    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
                    refine_seeds.extend(ranked.into_iter().take(2).map(|(_, t)| t));
                }
                if next_transform >= original_count + refine_seeds.len() {
                    break;
                }
                let base_transform = if next_transform < original_count {
                    transforms[next_transform]
                } else {
                    let seed = refine_seeds[next_transform - original_count];
                    let Some(refined) =
                        refinement::refine(gray, w, h, seed, c.module, mode, &mut refine_budget)
                    else {
                        limited = true;
                        break;
                    };
                    refined
                };
                next_transform += 1;
                if pending_rune.is_none() {
                    let rune_transform = centered(&base_transform, 11);
                    if let Some(grid) = qr_detect::sample(bits, w, h, 11, &rune_transform, 0.) {
                        for mirror in [false, true] {
                            for turn in 0..4 {
                                if !aztec::orientation_valid(&grid, 11, true, turn, mirror) {
                                    continue;
                                }
                                if let Some(read) =
                                    aztec::decode_rune(&rotate(&grid, 11, turn, mirror), 11)
                                {
                                    if template_error(bits, w, h, &centered(&base_transform, 9), 8)
                                        + read.corrected * 3
                                        > 8
                                    {
                                        regions.add(
                                            "Aztec",
                                            [[0., 0.], [11., 0.], [11., 11.], [0., 11.]].map(
                                                |[x, y]| qr_detect::map(&rune_transform, x, y),
                                            ),
                                            0.5,
                                            c.support,
                                        );
                                        limited = true;
                                        continue;
                                    }
                                    if mode >= 4 && read.corrected > 1 {
                                        continue;
                                    }
                                    if read.corrected > 0 {
                                        let agree = [-0.12, 0.12].into_iter().any(|offset| {
                                            qr_detect::sample(
                                                bits,
                                                w,
                                                h,
                                                11,
                                                &rune_transform,
                                                offset,
                                            )
                                            .and_then(|grid| {
                                                aztec::decode_rune(
                                                    &rotate(&grid, 11, turn, mirror),
                                                    11,
                                                )
                                            })
                                            .is_some_and(|other| {
                                                other.bytes == read.bytes
                                                    && other.corrected <= read.corrected
                                            })
                                        });
                                        let gray_agree = agree
                                            || curves::sample_rune(
                                                curves::Input {
                                                    gray,
                                                    width: w,
                                                    height: h,
                                                    mode,
                                                },
                                                &rune_transform,
                                                &mut self.rune_gray_budget,
                                                &mut limited,
                                            )
                                            .and_then(|grid| {
                                                aztec::decode_rune(
                                                    &rotate(&grid, 11, turn, mirror),
                                                    11,
                                                )
                                            })
                                            .is_some_and(|other| {
                                                other.bytes == read.bytes
                                                    && other.corrected < read.corrected
                                            });
                                        if !gray_agree {
                                            regions.add(
                                                "Aztec",
                                                [[0., 0.], [11., 0.], [11., 11.], [0., 11.]].map(
                                                    |[x, y]| qr_detect::map(&rune_transform, x, y),
                                                ),
                                                0.5,
                                                c.support,
                                            );
                                            limited = true;
                                            continue;
                                        }
                                    }
                                    pending_rune = Some(Detection {
                                        bytes: Some(read.bytes),
                                        structured_append: read.structured_append,
                                        reader_initialization: read.reader_initialization,
                                        addon: None,
                                        format: "Aztec".into(),
                                        text: read.text,
                                        polygon: [[0., 0.], [11., 0.], [11., 11.], [0., 11.]]
                                            .map(|[x, y]| qr_detect::map(&rune_transform, x, y)),
                                        support: c.support,
                                        error: barcode_multiformat::numeric::usize_f32(
                                            read.corrected,
                                        ),
                                        gs1: false,
                                    });
                                    if pending_rune.as_ref().is_some_and(|r| r.error == 0.) {
                                        results.push(pending_rune.take().expect("accepted Rune"));
                                        continue 'candidate;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                let t = centered(&base_transform, 15);
                let Some(mode_grid) = qr_detect::sample(bits, w, h, 15, &t, 0.) else {
                    continue;
                };
                for mirror in [false, true] {
                    for turn in 0..4 {
                        if ![true, false].into_iter().any(|compact| {
                            aztec::orientation_valid(&mode_grid, 15, compact, turn, mirror)
                        }) {
                            continue;
                        }
                        let mode_grid = rotate(&mode_grid, 15, turn, mirror);
                        for compact in [true, false] {
                            let Some(descriptor) = aztec::read_mode(&mode_grid, 15, compact) else {
                                continue;
                            };
                            let layers = descriptor.layers;
                            let base = layers * 4 + if compact { 11 } else { 14 };
                            let n = if compact {
                                base
                            } else {
                                base + 1 + 2 * ((base / 2 - 1) / 15)
                            };
                            let t = centered(&base_transform, n);
                            let Some(grid) = qr_detect::sample(bits, w, h, n, &t, 0.) else {
                                continue;
                            };
                            regions.add(
                                "Aztec",
                                [
                                    [0., 0.],
                                    [barcode_multiformat::numeric::usize_f32(n), 0.],
                                    [
                                        barcode_multiformat::numeric::usize_f32(n),
                                        barcode_multiformat::numeric::usize_f32(n),
                                    ],
                                    [0., barcode_multiformat::numeric::usize_f32(n)],
                                ]
                                .map(|[x, y]| qr_detect::map(&t, x, y)),
                                1.,
                                c.support,
                            );
                            if let Some(read) =
                                aztec::decode_matrix(&rotate(&grid, n, turn, mirror), n)
                            {
                                results.push(Detection {
                                    bytes: Some(read.bytes),
                                    structured_append: read.structured_append,
                                    reader_initialization: read.reader_initialization,
                                    addon: None,
                                    format: "Aztec".into(),
                                    text: read.text,
                                    polygon: [
                                        [0., 0.],
                                        [barcode_multiformat::numeric::usize_f32(n), 0.],
                                        [
                                            barcode_multiformat::numeric::usize_f32(n),
                                            barcode_multiformat::numeric::usize_f32(n),
                                        ],
                                        [0., barcode_multiformat::numeric::usize_f32(n)],
                                    ]
                                    .map(|[x, y]| qr_detect::map(&t, x, y)),
                                    support: c.support,
                                    error: barcode_multiformat::numeric::usize_f32(read.corrected),
                                    gs1: read.gs1,
                                });
                                continue 'candidate;
                            }
                            if n <= 63 {
                                curves::retain(
                                    &mut curve_seeds,
                                    curves::Input {
                                        gray,
                                        width: w,
                                        height: h,
                                        mode,
                                    },
                                    curves::Grid {
                                        t,
                                        n,
                                        turn,
                                        mirror,
                                        compact,
                                        descriptor,
                                    },
                                );
                            }
                            if !compact && n >= 35 {
                                let score = reference_error(bits, w, h, &t, n);
                                if score < 0.35 {
                                    reference_retries.push((score, t, n, turn, mirror));
                                }
                            }
                        }
                    }
                }
            }
            reference_retries.sort_by(|a, b| a.0.total_cmp(&b.0));
            limited |= reference_retries.len() > 6;
            for (_, t, n, turn, mirror) in reference_retries.into_iter().take(6) {
                let t = refine_reference(bits, w, h, t, n, c.module);
                let Some(grid) = qr_detect::sample(bits, w, h, n, &t, 0.) else {
                    continue;
                };
                if let Some(read) = aztec::decode_matrix(&rotate(&grid, n, turn, mirror), n) {
                    results.push(Detection {
                        bytes: Some(read.bytes),
                        structured_append: read.structured_append,
                        reader_initialization: read.reader_initialization,
                        addon: None,
                        format: "Aztec".into(),
                        text: read.text,
                        polygon: [
                            [0., 0.],
                            [barcode_multiformat::numeric::usize_f32(n), 0.],
                            [
                                barcode_multiformat::numeric::usize_f32(n),
                                barcode_multiformat::numeric::usize_f32(n),
                            ],
                            [0., barcode_multiformat::numeric::usize_f32(n)],
                        ]
                        .map(|[x, y]| qr_detect::map(&t, x, y)),
                        support: c.support,
                        error: barcode_multiformat::numeric::usize_f32(read.corrected),
                        gs1: read.gs1,
                    });
                    continue 'candidate;
                }
            }
            if let Some(read) = curves::recover(
                curves::Input {
                    gray,
                    width: w,
                    height: h,
                    mode,
                },
                &curve_seeds,
                c.support,
                &mut self.curve_budget,
                &mut self.bow_budget,
                &mut self.adaptive_budget,
                &mut limited,
            ) {
                results.push(read);
                continue 'candidate;
            }
            if let Some(rune) = pending_rune {
                results.push(rune);
            }
        }
        self.reads = results;
        self.limited = limited;
        self.refine_budget = refine_budget;
    }
    /// Additional thresholds reuse one histogram and skip unavailable splits.
    pub fn contrast(
        &mut self,
        gray: &[u8],
        w: usize,
        h: usize,
        regions: &mut barcode_multiformat::regions::Regions,
    ) {
        let mut images = binarization::Images::new(gray, w, h);
        for mode in [4, 6] {
            let Some(cut) = images.foreground_cut(mode) else {
                continue;
            };
            let candidates = centers_gray(gray, w, h, cut);
            let unresolved = candidates.iter().any(|c| {
                !self.reads.iter().any(|d| {
                    let p = d
                        .polygon
                        .iter()
                        .fold([0., 0.], |a, p| [a[0] + p[0] * 0.25, a[1] + p[1] * 0.25]);
                    (p[0] - c.x).hypot(p[1] - c.y) < c.module * 3.
                })
            });
            if !unresolved {
                self.limited |= candidates.len() > 64;
                continue;
            }
            let bits = images.get(mode);
            self.threshold_candidates(
                Threshold {
                    bits,
                    gray,
                    width: w,
                    height: h,
                    mode,
                },
                regions,
                &|_, row, out| crate::qr_frontend::transition_offsets_into(row, out),
                Some(candidates),
            );
        }
    }
}
fn detect(
    w: usize,
    h: usize,
    regions: &mut barcode_multiformat::regions::Regions,
    gray: &[u8],
    images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let mut search = Search::default();
    for mode in 0..4 {
        if images.is_duplicate(mode) {
            continue;
        }
        let (bits, rows) = images.get_with_runs(mode, (h / 600).max(1));
        search.threshold(
            Threshold {
                bits,
                gray,
                width: w,
                height: h,
                mode,
            },
            regions,
            &|y, row, out| rows.copy_or_extract(y, row, out),
        );
    }
    search.contrast(gray, w, h, regions);
    (search.reads, search.limited)
}

/// Scan Aztec with the retained threshold and candidate policies.
#[must_use]
pub fn scan(image: &[u8], width: usize, height: usize) -> barcode_multiformat::Scan {
    if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > image.len()) {
        return barcode_multiformat::Scan {
            barcodes: vec![],
            regions: vec![],
            unfinished: false,
            lines: 0,
        };
    }
    let mut regions = barcode_multiformat::regions::Regions::default();
    let mut binary = binarization::Images::new(image, width, height);
    let (mut reads, limited) = detect(width, height, &mut regions, image, &mut binary);
    reads.sort_by_key(|r| std::cmp::Reverse(r.support));
    let mut distinct: Vec<Detection> = Vec::new();
    for read in reads {
        if !distinct.iter().any(|prior| {
            prior.format == read.format
                && prior.text == read.text
                && prior.addon == read.addon
                && prior.structured_append == read.structured_append
                && prior.reader_initialization == read.reader_initialization
                && barcode_multiformat::regions::overlap(&prior.polygon, &read.polygon) >= 0.65
        }) {
            distinct.push(read);
        }
    }
    let (regions, region_limited) = regions.finish(&distinct);
    barcode_multiformat::Scan {
        barcodes: distinct,
        regions,
        unfinished: limited || region_limited,
        lines: 0,
    }
}

#[cfg(test)]
mod lazy_threshold_tests {
    use super::*;
    fn compare(gray: &[u8], w: usize, h: usize, cut: u8) -> usize {
        let bits: Vec<_> = gray.iter().map(|&v| v <= cut).collect();
        let eager = centers_with_rows(&bits, w, h, &|_, row, out| {
            crate::qr_frontend::transition_offsets_into(row, out);
        });
        let lazy = centers_gray(gray, w, h, cut);
        let values = |v: Vec<Center>| {
            v.into_iter()
                .map(|c| (c.x, c.y, c.module, c.support))
                .collect::<Vec<_>>()
        };
        let count = eager.len();
        assert_eq!(values(eager), values(lazy));
        count
    }
    #[test]
    fn lazy_threshold_preserves_bullseyes_and_exact_cluster_order() {
        let (w, h) = (147, 123);
        for scale in [1_usize, 2, 3, 5] {
            let mut gray = vec![240_u8; w * h];
            for [cx, cy] in [[29_usize, 29], [105, 85]] {
                for y in cy - 5 * scale..=cy + 5 * scale {
                    for x in cx - 5 * scale..=cx + 5 * scale {
                        let r = x.abs_diff(cx).max(y.abs_diff(cy)) / scale;
                        gray[y * w + x] = if r <= 4 && r % 2 == 0 { 32 } else { 160 };
                    }
                }
            }
            for cut in [0, 31, 32, 80, 159, 160, 239, 240, 255] {
                let count = compare(&gray, w, h, cut);
                if cut == 80 {
                    assert!(count >= 2, "two bullseye proposals at scale {scale}");
                }
            }
        }
    }
    #[test]
    fn lazy_threshold_preserves_dense_noise_and_sparse_row_schedules() {
        let mut seed = 123_456_u32;
        for (w, h) in [(1, 1), (3, 9), (97, 121), (129, 1204)] {
            let gray: Vec<_> = (0..w * h)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    seed.to_le_bytes()[3]
                })
                .collect();
            for cut in [0, 17, 80, 128, 209, 255] {
                compare(&gray, w, h, cut);
            }
        }
    }
}

//! Bullseye localization and independent Aztec grid sampling.
//!
//! One detector serves every caller. Mode-dependent recovery arrives as a
//! [`Policy`], since this crate has no mode features: Low selects
//! [`Policy::LOW`], the other modes [`Policy::STANDARD`].
use crate::{Detection, aztec, binarization, qr_detect};
mod center_index;
mod curves;
mod refinement;
mod upscale;

/// Mode-dependent Aztec recovery and its work budgets.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Source-gray recovery: refined transforms, confirmation of corrected Runes
    /// and curved-grid sampling. Without it a Rune is accepted on its first decode.
    pub gray_recovery: bool,
    /// Retry unread bullseyes on the foreground thresholds of the source gray.
    pub contrast: bool,
    /// Sampled pixels for refining the strongest transforms on source gray.
    pub refine_budget: usize,
    /// Modules sampled for curved-grid hypotheses.
    pub curve_budget: usize,
    /// Modules sampled for bowed-grid hypotheses.
    pub bow_budget: usize,
    /// Modules sampled with module-local thresholds.
    pub adaptive_budget: usize,
}

impl Policy {
    /// Low: live-camera budgets, without bowed or adaptive sampling.
    pub const LOW: Self = Self {
        gray_recovery: true,
        contrast: true,
        refine_budget: 10_000,
        curve_budget: 20_000,
        bow_budget: 0,
        adaptive_budget: 0,
    };
    /// Medium, High and Very High.
    pub const STANDARD: Self = Self {
        gray_recovery: true,
        contrast: true,
        refine_budget: 100_000,
        curve_budget: 150_000,
        bow_budget: 50_000,
        adaptive_budget: 50_000,
    };
    /// The policy of scanner mode `mode` (0 Low, 1 Medium, 2 High, 3 Very High).
    #[must_use]
    pub const fn for_mode(mode: usize) -> Self {
        if mode == 0 { Self::LOW } else { Self::STANDARD }
    }
}
#[derive(Clone)]
struct Center {
    x: f32,
    y: f32,
    module: f32,
    support: usize,
    /// Whether every row window that found this center lay inside a texture.
    texture: bool,
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
        crate::count_finder_work(1);
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
    let maximum = crate::numeric::f32_usize((expected * 2.9).ceil());
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
    let center = crate::numeric::usize_f32(start + end) / 2.;
    let mut widths = [0.; 7];
    widths[0] = crate::numeric::usize_f32(end - start);
    for ring in 1..=3 {
        let expect = ring % 2 == 0;
        let old = start;
        while start > 0 && pixel(start - 1) == expect {
            start -= 1;
            if old - start > maximum {
                return None;
            }
        }
        widths[2 * ring - 1] = crate::numeric::usize_f32(old - start);
        let old = end;
        while end < limit && pixel(end) == expect {
            end += 1;
            if end - old > maximum {
                return None;
            }
        }
        widths[2 * ring] = crate::numeric::usize_f32(end - old);
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
/// Reads that mark a sheet of symbols, whose rows between the scanned ones are searched too.
const SHEET_READS: usize = 8;
/// Bullseye proposals by support, not yet checked with [`is_bullseye`] (callers
/// verify only the proposals they use), and whether the check budget cut rows short.
fn centers_with_rows(
    bits: &[bool],
    w: usize,
    h: usize,
    row_cache: &RowCache<'_>,
) -> (Vec<Center>, bool) {
    centers_from_runs(
        w,
        h,
        &|i| bits[i],
        |y, out| {
            row_cache(y, &bits[y * w..(y + 1) * w], out);
        },
        false,
    )
}
/// Verified bullseye proposals by support for one foreground cut, and whether the
/// check budget cut rows short.
fn centers_gray(gray: &[u8], w: usize, h: usize, cut: u8) -> (Vec<Center>, bool) {
    let mut row = vec![false; w];
    let bits = |i: usize| gray[i] <= cut;
    let (mut found, limited) = centers_from_runs(
        w,
        h,
        &bits,
        |y, out| {
            for (target, &value) in row.iter_mut().zip(&gray[y * w..(y + 1) * w]) {
                *target = value <= cut;
            }
            crate::transition_offsets_into(&row, out);
        },
        false,
    );
    found.retain(|c| is_bullseye(&bits, w, h, c));
    (found, limited)
}
/// Two gradient directions and rings along both diagonals. The oblique test reads
/// at most 116 pixels, the direction test about 600, so it runs second.
fn is_bullseye(bits: &impl Fn(usize) -> bool, w: usize, h: usize, c: &Center) -> bool {
    has_oblique_rings(bits, w, h, c, 3)
        && binarization::has_two_directions_by(bits, w, h, c.x, c.y, c.module * 5.)
}
/// Bullseye centers on the rows of the row step, or with `skipped` on the rows between them.
fn centers_from_runs(
    w: usize,
    h: usize,
    bits: &impl Fn(usize) -> bool,
    mut copy_rows: impl FnMut(usize, &mut Vec<usize>),
    skipped: bool,
) -> (Vec<Center>, bool) {
    let mut found = center_index::Index::new(w, h);
    let mut refined = center_index::Index::new(w, h);
    let step = row_step(w, h);
    let mut offsets = Vec::new();
    let mut runs = Vec::new();
    let mut windows = Vec::new();
    let mut keys = Vec::new();
    let mut streaks = crate::finder_budget::Streaks::default();
    let mut starts = Vec::new();
    let mut plans = crate::finder_budget::Rows::default();
    // Cross checks are budgeted per scanned pixel. Half of the budget is available
    // from the start and each row adds its share of the other half; unused checks
    // carry over, so a textured band cannot exhaust the rows below it.
    let scanned = |y: &usize| y.is_multiple_of(step) != skipped;
    let rows = (0..h).filter(scanned).count();
    let mut available = w * rows;
    let mut limited = false;
    for y in (0..h).filter(scanned) {
        available += w;
        copy_rows(y, &mut offsets);
        runs.clear();
        #[expect(
            clippy::cast_precision_loss,
            reason = "This preserves the existing usize-to-f32 run-width conversion after exact transition extraction."
        )]
        let widths = offsets.windows(2).map(|pair| (pair[1] - pair[0]) as f32);
        runs.extend(widths);
        // A short row skips windows inside periodic textures and checks the
        // windows whose equal runs end first. A bullseye's end one run (compact)
        // or three runs (full) beyond the window, unless the reference grid
        // through a full symbol's center continues them to its edge.
        let allowed = available / (2 * PIXELS_PER_CHECK);
        let (first_dark, end) = (usize::from(!bits(y * w)), runs.len().saturating_sub(8));
        let plan = plans.starts(&mut starts, &offsets, first_dark, end, (1, 7), allowed, y);
        let short = plan.is_some();
        limited |= plan == Some(true);
        windows.clear();
        crate::finder_budget::for_each_start(plan, &starts, first_dark, end, |i| {
            if let Some(module) = equal_runs(&runs, i) {
                windows.push((i, module));
            }
        });
        let mut measured = false;
        if windows.len() > allowed {
            limited = true;
            measured = thin_windows(&mut windows, allowed, y, &offsets, &mut streaks, &mut keys);
        }
        // A short row spends its whole allowance, also what skipped texture windows
        // leave over: banked, it would pay for checking the texture of later rows.
        available -= if short { allowed } else { windows.len() } * 2 * PIXELS_PER_CHECK;
        let mut inserts = 0;
        for &(i, module) in &windows {
            let x = offsets[i + 4] + crate::numeric::f32_usize(runs[i + 4]) / 2;
            let Some((cy, my)) = cross(bits, w, h, x, y, true, module) else {
                continue;
            };
            if my / module < 0.6 || my / module > 1.7 {
                continue;
            }
            // Centers remember whether texture windows found them. Rows with many
            // centers measure their streaks once.
            inserts += 1;
            if inserts == 8 && !measured {
                streaks.fill(&offsets);
                measured = true;
            }
            let texture = if measured {
                streaks.is_texture(i + 1, i + 7)
            } else {
                crate::finder_budget::inside_texture(&offsets, i + 1, i + 7)
            };
            found.insert(
                crate::numeric::usize_f32(offsets[i + 4]) + runs[i + 4] * 0.5,
                cy,
                f32::midpoint(my, module),
                module * 1.5,
                texture,
            );
            let Some((cx, mx)) = cross(
                bits,
                w,
                h,
                x,
                crate::numeric::f32_usize(cy.floor().min(crate::numeric::usize_f32(h - 1))),
                false,
                my,
            ) else {
                continue;
            };
            if !(0.6..=1.7).contains(&(mx / my)) {
                continue;
            }
            let module = f32::midpoint(mx, my);
            refined.insert(cx, cy, module, module * 1.5, texture);
        }
    }
    let mut found = merge_refined(found.centers, refined.centers, w, h);
    found.sort_by_key(|c| std::cmp::Reverse(c.support));
    if limited || found.len() > MAX_FITTED {
        // Texture centers can outnumber the proposals that are fitted: rank the
        // centers that some window outside a texture found first, by support.
        found.sort_by_key(|c| c.texture);
    }
    (found, limited)
}
/// Keep the `allowed` windows whose equal runs end first ([`thin`]); returns
/// whether it measured the row's streaks, which many windows do once.
///
/// [`thin`]: crate::finder_budget::thin
fn thin_windows(
    windows: &mut Vec<(usize, f32)>,
    allowed: usize,
    y: usize,
    offsets: &[usize],
    streaks: &mut crate::finder_budget::Streaks,
    keys: &mut Vec<usize>,
) -> bool {
    let phase = crate::finder_budget::phase(y);
    if windows.len() > 32 {
        streaks.fill(offsets);
        let streaks = &*streaks;
        crate::finder_budget::thin(windows, allowed, phase, keys, |&(i, _)| {
            streaks.around(i + 1, i + 7)
        });
        true
    } else {
        crate::finder_budget::thin(windows, allowed, phase, keys, |&(i, _)| {
            crate::finder_budget::around(offsets, i + 1, i + 7, crate::finder_budget::STREAK_CAP)
        });
        false
    }
}
/// `found` followed by the `refined` centers that duplicate none of them.
fn merge_refined(mut found: Vec<Center>, refined: Vec<Center>, w: usize, h: usize) -> Vec<Center> {
    // Bucket kept centers in cells: a duplicate lies within `radius` on both
    // axes, so only the cells spanning that box can hold one.
    let columns = w.div_ceil(center_index::CELL).max(1);
    let rows = h.div_ceil(center_index::CELL).max(1);
    let cell =
        |v: f32, n: usize| (crate::numeric::f32_usize(v.max(0.)) / center_index::CELL).min(n - 1);
    let mut cells: Vec<Vec<usize>> = vec![Vec::new(); columns * rows];
    for (i, c) in found.iter().enumerate() {
        cells[cell(c.y, rows) * columns + cell(c.x, columns)].push(i);
    }
    for c in refined {
        let radius = c.module * 0.2;
        let duplicate = (cell(c.y - radius, rows)..=cell(c.y + radius, rows)).any(|row| {
            (cell(c.x - radius, columns)..=cell(c.x + radius, columns)).any(|column| {
                cells[row * columns + column].iter().any(|&i| {
                    let old = &found[i];
                    let (dx, dy) = (old.x - c.x, old.y - c.y);
                    // hypot is at least the larger coordinate difference.
                    dx.abs() < radius
                        && dy.abs() < radius
                        && (old.module / c.module - 1.).abs() < 0.05
                        && dx.hypot(dy) < radius
                })
            })
        });
        if !duplicate {
            cells[cell(c.y, rows) * columns + cell(c.x, columns)].push(found.len());
            found.push(c);
        }
    }
    found
}
/// The module of the seven equal runs after run `i`, if they are equal enough.
fn equal_runs(runs: &[f32], i: usize) -> Option<f32> {
    let module = runs[i + 1..i + 8].iter().sum::<f32>() / 7.;
    (module >= 0.8
        && runs[i + 1..i + 8]
            .iter()
            .all(|v| (*v - module).abs() <= module * 0.7))
    .then_some(module)
}
/// Row step of the bullseye scans.
fn row_step(w: usize, h: usize) -> usize {
    crate::finder_budget::row_step(w, h, 600)
}
/// Scanned row pixels per bullseye cross check. Real images need at most one check
/// per 35 pixels; checkerboards and stripes pass the row test at nearly every run.
const PIXELS_PER_CHECK: usize = 16;
// Parallel stripes can satisfy both axis scans at 45 degrees. A bullseye also
// crosses several rings in both diagonal directions, independent of its pose.
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
                let distance =
                    crate::numeric::f64_f32(f64::from(step)) * center.module * 0.25 * sign;
                let x = crate::numeric::f32_isize((center.x + dx * distance).floor());
                let y = crate::numeric::f32_isize((center.y + dy * distance).floor());
                if x < 0 || y < 0 || x >= (w).cast_signed() || y >= (h).cast_signed() {
                    break;
                }
                crate::count_finder_work(1);
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
    transform_turned(c, [theta.cos(), theta.sin()], module, n)
}
fn transform_turned(c: &Center, [cos, sin]: [f32; 2], module: f32, n: usize) -> [f32; 8] {
    let a = cos * module;
    let b = sin * module;
    let half = crate::numeric::usize_f32(n) / 2.;
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
                crate::numeric::f64_f32(f64::from(x)) + 0.5,
                crate::numeric::f64_f32(f64::from(y)) + 0.5,
            );
            if px < 0.
                || py < 0.
                || px >= crate::numeric::usize_f32(w)
                || py >= crate::numeric::usize_f32(h)
            {
                return 81;
            }
            crate::count_finder_work(1);
            let ring = (x - 4).abs().max((y - 4).abs());
            errors += usize::from(
                bits[crate::numeric::f32_usize(py) * w + crate::numeric::f32_usize(px)]
                    != (ring % 2 == 0),
            );
            if errors > limit {
                return errors;
            }
        }
    }
    errors
}
/// Smallest ring mismatch of a bullseye candidate over the sampled poses.
fn ring_fit(bits: &[bool], w: usize, h: usize, c: &Center) -> usize {
    ring_fit_within(bits, w, h, c, 81)
}
/// [`ring_fit`] when it is at most `limit`; otherwise some value above `limit`.
///
/// An early-stopped pose count is above its bound and at most the pose's full
/// count, so bounding each pose by the best count so far keeps the minimum exact.
fn ring_fit_within(bits: &[bool], w: usize, h: usize, c: &Center, limit: usize) -> usize {
    // The six poses' cosine, sine and the larger of their magnitudes.
    static POSES: std::sync::OnceLock<[[f32; 3]; 6]> = std::sync::OnceLock::new();
    let poses = POSES.get_or_init(|| {
        std::array::from_fn(|i| {
            let theta = crate::numeric::usize_f32(i * 15).to_radians();
            [
                theta.cos(),
                theta.sin(),
                theta.cos().abs().max(theta.sin().abs()),
            ]
        })
    });
    let mut best = usize::MAX;
    for &[cos, sin, scale] in poses {
        if best == 0 {
            break;
        }
        let module = c.module * scale;
        let bound = limit.min(best - 1);
        best = best.min(template_error(
            bits,
            w,
            h,
            &transform_turned(c, [cos, sin], module, 9),
            bound,
        ));
    }
    best
}
/// Bullseye candidates verified per threshold image.
const MAX_CANDIDATES: usize = 64;
/// The [`MAX_CANDIDATES`] candidates accepted by `verify` with the smallest ring
/// fit, larger support first among equal fits, in the order of a stable sort by
/// that key. Textures such as dot grids yield tens of thousands of candidates, so
/// a fit is measured only up to the current worst kept one, and only candidates
/// that would be kept are verified.
fn best_candidates(
    bits: &[bool],
    w: usize,
    h: usize,
    candidates: Vec<Center>,
    mut verify: impl FnMut(usize, &Center) -> bool,
) -> Vec<Center> {
    type Key = (usize, std::cmp::Reverse<usize>, usize);
    let mut kept: std::collections::BinaryHeap<Key> = std::collections::BinaryHeap::new();
    for (index, c) in candidates.iter().enumerate() {
        let full = kept.len() == MAX_CANDIDATES;
        let limit = kept.peek().filter(|_| full).map_or(81, |worst| worst.0);
        let fit = ring_fit_within(bits, w, h, c, limit);
        let key = (fit, std::cmp::Reverse(c.support), index);
        if fit > limit || kept.peek().is_some_and(|worst| full && key > *worst) {
            continue;
        }
        if !verify(index, c) {
            continue;
        }
        if full {
            kept.pop();
        }
        kept.push(key);
    }
    let mut candidates: Vec<Option<Center>> = candidates.into_iter().map(Some).collect();
    kept.into_sorted_vec()
        .into_iter()
        .filter_map(|(_, _, index)| candidates[index].take())
        .collect()
}
/// The candidates [`best_candidates`] keeps among unverified `proposals`, and
/// whether more than [`MAX_CANDIDATES`] proposals are bullseyes.
fn select_candidates(
    bits: &[bool],
    w: usize,
    h: usize,
    proposals: Vec<Center>,
) -> (Vec<Center>, bool) {
    // Whether more than MAX_CANDIDATES proposals are bullseyes needs only the
    // first ones checked; the selection checks the rest when they would be kept.
    let mut checked: Vec<Option<bool>> = vec![None; proposals.len()];
    let mut verified = 0;
    for (c, check) in proposals.iter().zip(&mut checked) {
        if verified > MAX_CANDIDATES {
            break;
        }
        let bullseye = is_bullseye(&|i| bits[i], w, h, c);
        *check = Some(bullseye);
        verified += usize::from(bullseye);
    }
    let (mut proposals, checked): (Vec<Center>, Vec<Option<bool>>) = proposals
        .into_iter()
        .zip(checked)
        .filter(|(_, check)| *check != Some(false))
        .unzip();
    let more = verified > MAX_CANDIDATES || proposals.len() > MAX_FITTED;
    proposals.truncate(MAX_FITTED);
    let kept = best_candidates(bits, w, h, proposals, |index, c| {
        checked[index].unwrap_or_else(|| is_bullseye(&|i| bits[i], w, h, c))
    });
    (kept, more)
}
/// Proposals ring-fitted per threshold image, by support. Real images yield at
/// most about 1,300 proposals; noise yields tens of thousands that all need a fit.
const MAX_FITTED: usize = 4096;
fn centered(t: &[f32; 8], n: usize) -> [f32; 8] {
    let half = crate::numeric::usize_f32(n) * 0.5;
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
    let x = crate::numeric::f32_isize(
        (center.x + theta.cos() * crate::numeric::usize_f32(ring) * module).floor(),
    );
    let y = crate::numeric::f32_isize(
        (center.y + theta.sin() * crate::numeric::usize_f32(ring) * module).floor(),
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
    let radius =
        crate::numeric::f32_isize((module * crate::numeric::usize_f32(ring + 2)).ceil()) + 2;
    let left = (crate::numeric::f32_isize(center.x.floor()) - radius).max(0);
    let top = (crate::numeric::f32_isize(center.y.floor()) - radius).max(0);
    let right = (crate::numeric::f32_isize(center.x.ceil()) + radius).min((w).cast_signed());
    let bottom = (crate::numeric::f32_isize(center.y.ceil()) + radius).min((h).cast_signed());
    let expected_area = module * module * 8. * crate::numeric::usize_f32(ring);
    let quads = crate::component_geometry::quads(
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
    let half = crate::numeric::usize_f32(ring) + 0.5;
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
/// The reference points of a full symbol of side `n` (module center, expected
/// color, ring), ordered by ring: their Chebyshev distance from the center.
fn reference_points(n: usize) -> &'static [(f32, f32, bool, usize)] {
    type Points = Vec<(f32, f32, bool, usize)>;
    static GRIDS: [std::sync::OnceLock<Points>; 76] = [const { std::sync::OnceLock::new() }; 76];
    GRIDS[n / 2].get_or_init(|| {
        let center = (n / 2).cast_signed();
        let mut points = Vec::new();
        for y in 0..n {
            for x in 0..n {
                let xx = (x).cast_signed() - center;
                let yy = (y).cast_signed() - center;
                let ring = xx.unsigned_abs().max(yy.unsigned_abs());
                let expected = if ring <= 4 {
                    ring.is_multiple_of(2)
                } else if ring <= 7 {
                    continue;
                } else if xx % 16 == 0 {
                    yy % 2 == 0
                } else if yy % 16 == 0 {
                    xx % 2 == 0
                } else {
                    continue;
                };
                points.push((
                    crate::numeric::usize_f32(x) + 0.5,
                    crate::numeric::usize_f32(y) + 0.5,
                    expected,
                    ring,
                ));
            }
        }
        points.sort_by_key(|p| p.3);
        points
    })
}
/// Mean mismatch of the reference points up to ring `rings` under `t`.
fn reference_error(bits: &[bool], w: usize, h: usize, t: &[f32; 8], n: usize, rings: usize) -> f32 {
    let all = reference_points(n);
    let points = &all[..all.partition_point(|p| p.3 <= rings)];
    let mut error = 0.;
    for &(x, y, expected, _) in points {
        let [px, py] = qr_detect::map(t, x, y);
        let px = px - 0.5;
        let py = py - 0.5;
        let ix = crate::numeric::f32_isize(px.floor());
        let iy = crate::numeric::f32_isize(py.floor());
        if ix < 0 || iy < 0 || ix + 1 >= (w).cast_signed() || iy + 1 >= (h).cast_signed() {
            error += 1.;
            continue;
        }
        let dx = px - crate::numeric::isize_f32(ix);
        let dy = py - crate::numeric::isize_f32(iy);
        let at = (iy).cast_unsigned() * w + (ix).cast_unsigned();
        let value = (f32::from(u8::from(bits[at])) * (1. - dx)
            + f32::from(u8::from(bits[at + 1])) * dx)
            * (1. - dy)
            + (f32::from(u8::from(bits[at + w])) * (1. - dx)
                + f32::from(u8::from(bits[at + w + 1])) * dx)
                * dy;
        error += if expected { 1. - value } else { value };
    }
    error / crate::numeric::usize_f32(points.len())
}
/// Rings around the center whose reference points judge a transform from the
/// bullseye: the first reference ring at 16 modules and the modules beside it.
/// A transform fitted to the 9-module bullseye is reliable there; a small
/// rotation or scale error moves the outer rings of a large symbol by a module
/// or more, so the whole-symbol error cannot tell it from a wrong transform.
const INNER_RINGS: usize = 20;
/// Refine a transform by moving the symbol corners to fit the reference points,
/// growing the fitted region outward one reference ring (16 modules) at a time
/// so that each stage starts within a module of the points it adds.
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
        [crate::numeric::usize_f32(n), 0.],
        [crate::numeric::usize_f32(n), crate::numeric::usize_f32(n)],
        [0., crate::numeric::usize_f32(n)],
    ];
    let mut quad = source.map(|[x, y]| qr_detect::map(&t, x, y));
    let mut best = t;
    let half = n / 2;
    let mut rings = INNER_RINGS.min(half);
    loop {
        let mut error = reference_error(bits, w, h, &best, n, rings);
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
                            let score = reference_error(bits, w, h, &t, n, rings);
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
        if rings == half {
            return best;
        }
        rings = (rings + 16).min(half);
    }
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
    /// Bullseye candidates that no threshold pass turned into a read.
    unresolved: Vec<(usize, Center)>,
    refine_budget: usize,
    curve_budget: usize,
    bow_budget: usize,
    rune_gray_budget: usize,
    adaptive_budget: usize,
    policy: Policy,
}
impl Search {
    #[must_use]
    pub fn new(policy: Policy) -> Self {
        Self {
            reads: Vec::new(),
            limited: false,
            unresolved: Vec::new(),
            refine_budget: policy.refine_budget,
            curve_budget: policy.curve_budget,
            bow_budget: policy.bow_budget,
            rune_gray_budget: 8_192,
            adaptive_budget: policy.adaptive_budget,
            policy,
        }
    }
    pub fn threshold(
        &mut self,
        input: Threshold<'_>,
        regions: &mut crate::regions::Regions,
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
        regions: &mut crate::regions::Regions,
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
        let (candidates, more) = if let Some(verified) = prepared {
            let more = verified.len() > MAX_CANDIDATES;
            (best_candidates(bits, w, h, verified, |_, _| true), more)
        } else {
            let (proposals, cut) = centers_with_rows(bits, w, h, row_cache);
            let (kept, more) = select_candidates(bits, w, h, proposals);
            (kept, more || cut)
        };
        limited |= more;
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
                let theta = crate::numeric::f64_f32(f64::from(degrees)).to_radians();
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
                if next_transform == original_count && self.policy.gray_recovery {
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
                                    if !self.policy.gray_recovery {
                                        results.push(Detection {
                                            assembled_rows: false,
                                            bytes: Some(read.bytes),
                                            structured_append: read.structured_append,
                                            reader_initialization: read.reader_initialization,
                                            addon: None,
                                            format: "Aztec".into(),
                                            text: read.text,
                                            polygon: [[0., 0.], [11., 0.], [11., 11.], [0., 11.]]
                                                .map(|[x, y]| {
                                                    qr_detect::map(&rune_transform, x, y)
                                                }),
                                            support: c.support,
                                            error: crate::numeric::usize_f32(read.corrected),
                                            gs1: false,
                                        });
                                        continue 'candidate;
                                    }
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
                                        assembled_rows: false,
                                        bytes: Some(read.bytes),
                                        structured_append: read.structured_append,
                                        reader_initialization: read.reader_initialization,
                                        addon: None,
                                        format: "Aztec".into(),
                                        text: read.text,
                                        polygon: [[0., 0.], [11., 0.], [11., 11.], [0., 11.]]
                                            .map(|[x, y]| qr_detect::map(&rune_transform, x, y)),
                                        support: c.support,
                                        error: crate::numeric::usize_f32(read.corrected),
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
                                    [crate::numeric::usize_f32(n), 0.],
                                    [crate::numeric::usize_f32(n), crate::numeric::usize_f32(n)],
                                    [0., crate::numeric::usize_f32(n)],
                                ]
                                .map(|[x, y]| qr_detect::map(&t, x, y)),
                                1.,
                                c.support,
                            );
                            if let Some(read) =
                                aztec::decode_matrix(&rotate(&grid, n, turn, mirror), n)
                            {
                                results.push(Detection {
                                    assembled_rows: false,
                                    bytes: Some(read.bytes),
                                    structured_append: read.structured_append,
                                    reader_initialization: read.reader_initialization,
                                    addon: None,
                                    format: "Aztec".into(),
                                    text: read.text,
                                    polygon: [
                                        [0., 0.],
                                        [crate::numeric::usize_f32(n), 0.],
                                        [
                                            crate::numeric::usize_f32(n),
                                            crate::numeric::usize_f32(n),
                                        ],
                                        [0., crate::numeric::usize_f32(n)],
                                    ]
                                    .map(|[x, y]| qr_detect::map(&t, x, y)),
                                    support: c.support,
                                    error: crate::numeric::usize_f32(read.corrected),
                                    gs1: read.gs1,
                                });
                                continue 'candidate;
                            }
                            if n <= 63 && self.policy.gray_recovery {
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
                                let score = reference_error(bits, w, h, &t, n, INNER_RINGS);
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
                        assembled_rows: false,
                        bytes: Some(read.bytes),
                        structured_append: read.structured_append,
                        reader_initialization: read.reader_initialization,
                        addon: None,
                        format: "Aztec".into(),
                        text: read.text,
                        polygon: [
                            [0., 0.],
                            [crate::numeric::usize_f32(n), 0.],
                            [crate::numeric::usize_f32(n), crate::numeric::usize_f32(n)],
                            [0., crate::numeric::usize_f32(n)],
                        ]
                        .map(|[x, y]| qr_detect::map(&t, x, y)),
                        support: c.support,
                        error: crate::numeric::usize_f32(read.corrected),
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
            } else if self.unresolved.len() < upscale::MAX_UNRESOLVED {
                let fit = ring_fit(bits, w, h, &c);
                self.unresolved.push((fit, c));
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
        regions: &mut crate::regions::Regions,
    ) {
        if !self.policy.contrast {
            return;
        }
        let mut images = binarization::Images::new(gray, w, h);
        for mode in [4, 6] {
            let Some(cut) = images.foreground_cut(mode) else {
                continue;
            };
            let (candidates, cut_short) = centers_gray(gray, w, h, cut);
            self.limited |= cut_short;
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
                &|_, row, out| crate::transition_offsets_into(row, out),
                Some(candidates),
            );
        }
    }
}
/// Read Aztec symbols with the shared threshold images: the four base
/// thresholds, then the policy's contrast retries and, for `effort` above 0, the
/// enlarged retry of [`Search::upscaled`].
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    gray: &[u8],
    images: &mut binarization::Images<'_>,
    policy: Policy,
    effort: usize,
) -> (Vec<Detection>, bool) {
    let mut search = Search::new(policy);
    for mode in 0..4 {
        if images.is_duplicate(mode) {
            continue;
        }
        let (bits, rows) = images.get_with_runs(mode, row_step(w, h));
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
    // A bullseye's equal runs show only on rows through its one-module center, which a
    // row step above one module crosses or misses by phase. A sheet that read several
    // symbols also searches the rows between.
    if row_step(w, h) > 1 && search.reads.len() >= SHEET_READS {
        for mode in 0..4 {
            if images.is_duplicate(mode) {
                continue;
            }
            let bits = images.get(mode);
            let (centers, _) = centers_from_runs(
                w,
                h,
                &|i| bits[i],
                |y, out| crate::transition_offsets_into(&bits[y * w..(y + 1) * w], out),
                true,
            );
            let centers: Vec<Center> = centers
                .into_iter()
                .filter(|c| is_bullseye(&|i| bits[i], w, h, c))
                .collect();
            if !centers.is_empty() {
                search.threshold_candidates(
                    Threshold {
                        bits,
                        gray,
                        width: w,
                        height: h,
                        mode,
                    },
                    regions,
                    &|_, row, out| crate::transition_offsets_into(row, out),
                    Some(centers),
                );
            }
        }
    }
    search.contrast(gray, w, h, regions);
    search.upscaled(gray, w, h, effort);
    (search.reads, search.limited)
}

/// Native diagnostic only; exposes the actual production bullseye proposals.
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_centers(bits: &[bool], w: usize, h: usize) -> serde_json::Value {
    serde_json::Value::Array(
        centers_with_rows(bits, w, h, &|_, row, out| {
            crate::transition_offsets_into(row, out);
        })
        .0
        .iter()
        .filter(|c| is_bullseye(&|i| bits[i], w, h, c))
        .map(|c| serde_json::json!({"x":c.x,"y":c.y,"module":c.module,"support":c.support}))
        .collect(),
    )
}

#[cfg(test)]
mod lazy_threshold_tests {
    use super::*;
    fn compare(gray: &[u8], w: usize, h: usize, cut: u8) -> usize {
        let bits: Vec<_> = gray.iter().map(|&v| v <= cut).collect();
        let (mut eager, eager_cut) = centers_with_rows(&bits, w, h, &|_, row, out| {
            crate::transition_offsets_into(row, out);
        });
        eager.retain(|c| is_bullseye(&|i| bits[i], w, h, c));
        let (lazy, lazy_cut) = centers_gray(gray, w, h, cut);
        assert_eq!(eager_cut, lazy_cut);
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

#[cfg(test)]
mod bounded_work_tests {
    use super::*;
    /// Whether a texture pixel is dark.
    type Texture = fn(usize, usize) -> bool;
    fn pattern(w: usize, h: usize, dark: impl Fn(usize, usize) -> bool) -> Vec<bool> {
        (0..w * h).map(|i| dark(i % w, i / w)).collect()
    }
    #[test]
    fn bullseyes_right_of_and_inside_textures_are_checked() {
        let (w, h) = (640, 360);
        let (cx, cy) = (500_usize, 200_usize);
        // A compact bullseye of 4-pixel modules (dark center, five rings) with a
        // light margin.
        let bullseye = |x: usize, y: usize| {
            let ring = (x / 4).abs_diff(cx / 4).max((y / 4).abs_diff(cy / 4));
            (ring <= 6).then_some(ring.is_multiple_of(2) && ring <= 4)
        };
        let textures: [(&str, Texture); 3] = [
            ("checkerboard", |x, y| (x / 2 + y / 2) % 2 == 0),
            ("vertical stripes", |x, _| x / 2 % 2 == 0),
            ("dot grid", |x, y| x % 4 < 2 && y % 4 < 2),
        ];
        for (name, texture) in textures {
            for (place, covered) in [
                ("right of", (|x, _| x < 440) as fn(usize, usize) -> bool),
                ("inside", |_, _| true),
            ] {
                let bits = pattern(w, h, |x, y| {
                    bullseye(x, y).unwrap_or_else(|| covered(x, y) && texture(x, y))
                });
                let (proposals, limited) = centers_with_rows(&bits, w, h, &|_, row, out| {
                    crate::transition_offsets_into(row, out);
                });
                assert!(limited, "{name}, {place}: the texture exhausts the checks");
                let center = crate::numeric::usize_f32(cx / 4 * 4 + 2);
                let middle = crate::numeric::usize_f32(cy / 4 * 4 + 2);
                assert!(
                    proposals
                        .iter()
                        .any(|c| (c.x - center).abs() < 2. && (c.y - middle).abs() < 2.),
                    "{name}, {place}: bullseye among {} proposals",
                    proposals.len()
                );
            }
        }
    }
    #[test]
    fn selection_matches_a_stable_sort_of_verified_candidates() {
        let (w, h) = (160, 120);
        let mut seed = 77_003_u32;
        let mut random = move |n: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) % n
        };
        // Blocky noise gives a spread of ring fits, with many ties.
        let blocks: Vec<bool> = (0..20 * 15).map(|_| random(2) == 1).collect();
        let bits = pattern(w, h, |x, y| blocks[y / 8 * 20 + x / 8]);
        for count in [0, 1, 63, 64, 65, 400] {
            let candidates: Vec<Center> = (0..count)
                .map(|_| Center {
                    x: crate::numeric::usize_f32(20 + random(120) as usize),
                    y: crate::numeric::usize_f32(20 + random(80) as usize),
                    module: 1. + crate::numeric::usize_f32(random(3) as usize),
                    support: 1 + random(4) as usize,
                    texture: false,
                })
                .collect();
            let accept = |i: usize| i % 3 != 1;
            let mut expected: Vec<(usize, &Center)> = candidates
                .iter()
                .enumerate()
                .filter(|(i, _)| accept(*i))
                .map(|(_, c)| (ring_fit(&bits, w, h, c), c))
                .collect();
            expected.sort_by_key(|(fit, c)| (*fit, std::cmp::Reverse(c.support)));
            expected.truncate(MAX_CANDIDATES);
            let kept = best_candidates(&bits, w, h, candidates.clone(), |i, _| accept(i));
            let values = |c: &Center| (c.x, c.y, c.module, c.support);
            assert_eq!(
                kept.iter().map(values).collect::<Vec<_>>(),
                expected.iter().map(|(_, c)| values(c)).collect::<Vec<_>>(),
                "{count} candidates"
            );
        }
    }
    #[test]
    fn periodic_textures_spend_bounded_bullseye_work() {
        let (w, h) = (640, 360);
        let tilt = |x: usize, y: usize| {
            let t = crate::numeric::usize_f32(x) * 0.866 + crate::numeric::usize_f32(y) * 0.5;
            crate::numeric::f32_usize(t / 3.).is_multiple_of(2)
        };
        for (name, bits) in [
            ("dot grid", pattern(w, h, |x, y| x % 5 < 2 && y % 5 < 2)),
            ("tilted stripes", pattern(w, h, tilt)),
            (
                "checkerboard",
                pattern(w, h, |x, y| (x / 5 + y / 5) % 2 == 0),
            ),
        ] {
            let (proposals, _) = centers_with_rows(&bits, w, h, &|_, row, out| {
                crate::transition_offsets_into(row, out);
            });
            crate::take_finder_work();
            let _ = select_candidates(&bits, w, h, proposals);
            let work = crate::take_finder_work();
            // Verifying and fitting every proposal reads 26 and 42 million pixels on
            // the dot grid and the tilted stripes.
            assert!(work < 32 * w * h, "{name}: {work} pixel reads");
        }
    }
}

#[cfg(test)]
mod reference_grid_tests {
    use super::*;
    /// A 71-module full symbol (15 layers) written by ZXing-C++ for [`text`],
    /// rows first, as hexadecimal bits.
    const SYMBOL: [&str; 15] = [
        "8008412d452923d680b375311abb57a446f687ad02b2375211ab3aaaaaaaaaaaaaaaaaa6911ad87002104b52",
        "17abc623770e02421740a923d6815933a9106d09eda0ae4dda440d87e221bb0700410966b6c7202124b534a2",
        "3a82ac8dd4846aad5f95b85ade229bb0f076d41752493cb414c99096d2a31ab7578a455f1859025a92524ba1",
        "42dd495ecdddcaaaf16947346f7747572ce121f1f6fb193540e27b26be108d68ae1e9f1b2aaaaaaaaaaaaaaa",
        "aaa437b26770fde0f6522317dfd19e42eaa8fe818140d678c21a4a28fc359af91c452faa9c0f812bdc6f0548",
        "42c230f0a1673e7d314a60383ca7088bafa03b15b3499568db568c49d4168289d8222e515a83920f83fffdce",
        "7d7a20114141001ad3b26a19855fc6ffb00b2c4fac20ec7d0152e50248e2ab254afa88e3702282659af51583",
        "3cf432aaaaaaaaaaaaaaaaa8b142ce545706902cb62662a7afaba2d6cec343113d405f6b2091592ad9e2ffa5",
        "581bb9812cc58c0055288c847620274fffeee354f480928c49846787e0b4f0e7f907e057f83ad6370cc80ee4",
        "72eb3c2e55264b1e84bbc416b22d393ae05c7821d0d9c9ed9febfdbb3acb0b2b48eb00898601a1606ced24e1",
        "e337696424358b76aef3a01412aaaaaaaaaaaaaaaaaea10cbc1ea8d0225a2e35afef9a53c68f5e832610f56a",
        "ac42a90aab2bea4a595a48912946edcc446d5b4b5426f1c06d793294b606a60a210401a1bb082f828944d764",
        "ea0bafacec2a0aaf245294d20816b377111f56d6c44e43088bab696222bb0148af2a529ad218400a616bc494",
        "a2b482104d2ec463d5bab1295cee200e0dd8847a9b5602aaaaaaaaaaaaaaaaaa764ca0b4f24a2958d2f56eac",
        "4657666a0b224a515a410801c3738",
    ];
    const SIDE: usize = 71;
    fn text() -> String {
        "REFERENCE-GRID-0123456789-".repeat(22)[..560].to_owned()
    }
    fn module(x: usize, y: usize) -> bool {
        let bit = y * SIDE + x;
        let digit = SYMBOL.concat().as_bytes()[bit / 4];
        let value = char::from(digit).to_digit(16).expect("hexadecimal fixture");
        value >> (3 - bit % 4) & 1 == 1
    }
    /// The symbol at `pixels` per module, turned by `degrees` about the frame
    /// center and averaged over 3x3 samples per pixel.
    fn frame(pixels: f32, degrees: f32) -> (Vec<u8>, usize) {
        let n = crate::numeric::usize_f32(SIDE);
        let side = crate::numeric::f32_usize((n * pixels * 1.3).floor()) + 40;
        let center = crate::numeric::usize_f32(side) / 2.;
        let (sin, cos) = degrees.to_radians().sin_cos();
        let matrix: Vec<bool> = (0..SIDE * SIDE)
            .map(|i| module(i % SIDE, i / SIDE))
            .collect();
        let dark = |x: f32, y: f32| {
            let (dx, dy) = (x - center, y - center);
            let u = (cos * dx + sin * dy) / pixels + n / 2.;
            let v = (cos * dy - sin * dx) / pixels + n / 2.;
            (0. ..n).contains(&u)
                && (0. ..n).contains(&v)
                && matrix[crate::numeric::f32_usize(v) * SIDE + crate::numeric::f32_usize(u)]
        };
        let gray = (0..side * side)
            .map(|i| {
                let (x, y) = (
                    crate::numeric::usize_f32(i % side),
                    crate::numeric::usize_f32(i / side),
                );
                let covered = (0..9)
                    .filter(|&s| {
                        dark(
                            x + (crate::numeric::usize_f32(s % 3) + 0.5) / 3.,
                            y + (crate::numeric::usize_f32(s / 3) + 0.5) / 3.,
                        )
                    })
                    .count();
                crate::numeric::f32_u8(
                    (215. - 175. * crate::numeric::usize_f32(covered) / 9.).round(),
                )
            })
            .collect();
        (gray, side)
    }
    #[test]
    fn slightly_turned_full_symbol_reads_from_the_reference_grid() {
        // A 1.5 degree turn that the bullseye fit misses moves the outer layers
        // by about half a module, so the whole-symbol reference error rejects
        // the transform although it is right near the center.
        let (gray, side) = frame(3.2, 1.5);
        for (policy, effort) in [(Policy::LOW, 0), (Policy::STANDARD, 1)] {
            let mut images = binarization::Images::new(&gray, side, side);
            let (reads, _) = detect(
                side,
                side,
                &mut crate::regions::Regions::default(),
                &gray,
                &mut images,
                policy,
                effort,
            );
            assert_eq!(reads.len(), 1, "effort {effort}");
            assert_eq!(reads[0].text, text());
        }
    }
}

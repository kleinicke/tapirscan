//! Micro QR proposals from single finders. A Micro QR symbol has one finder, so
//! each finder that no decoded symbol claims is tried in its eight orientations:
//! the format bits beside it name the symbol size, the timing and separator
//! modules rank the orientations, and the Micro QR decoder's Reed-Solomon check
//! validates the sampled grid.
use super::{Finder, distance, map, sample, single};
use crate::{Detection, micro_qr};

/// Grid decodes per detection pass.
const ATTEMPTS: usize = 64;
/// Finder quads whose orientations are scored in the standard threshold modes of
/// a detection pass, and again in its foreground modes; a finder's
/// central and outer quads count separately. Each quad checks the format and
/// timing modules of eight orientations at every turn and scale correction, so
/// this bounds a pass over texture with hundreds of finder-like squares.
const SCORED_QUADS: usize = 256;
/// Grid decodes per finder, best-ranked orientations first.
const FINDER_CANDIDATES: usize = 4;
/// Module-size corrections of the finder estimate.
const SCALES: [f32; 5] = [1., 0.94, 1.06, 0.88, 1.12];
/// Rotations, in radians, of the finder's orientation estimate, which a center
/// square a few pixels across gives only to several degrees.
const TURNS: [f32; 5] = [0., -0.07, 0.07, -0.14, 0.14];
/// Sampling offsets, in modules, of the grids tried per orientation.
const OFFSETS: [f32; 3] = [0., -0.2, 0.2];

/// Work of the Micro QR search in one detection pass, shared by its threshold
/// modes; the foreground modes renew the scoring allowance.
#[derive(Default)]
pub(super) struct Budget {
    /// Grid decodes, at most [`ATTEMPTS`].
    attempts: usize,
    /// Scored finder quads, at most [`SCORED_QUADS`].
    scored: usize,
}

impl Budget {
    /// Allow [`SCORED_QUADS`] more scored quads, keeping the decode count.
    pub(super) fn renew_scoring(&mut self) {
        self.scored = 0;
    }
}

struct Candidate {
    score: usize,
    transform: [f32; 8],
    symbol: usize,
    mask: usize,
    /// Localization evidence when the orientation fails to decode: exact format
    /// bits and at most a tenth of the timing and separator modules wrong.
    localization: Option<f32>,
}

/// Module-to-image transform with the finder center at module (3.5, 3.5), the
/// frame's axes scaled by `scale` and turned by `turn` radians.
fn transform(frame: &[f32; 6], scale: f32, turn: f32) -> [f32; 8] {
    let (sin, cos) = turn.sin_cos();
    let rotate = |x: f32, y: f32| [(x * cos - y * sin) * scale, (x * sin + y * cos) * scale];
    let [ax, ay] = rotate(frame[0], frame[1]);
    let [bx, by] = rotate(frame[2], frame[3]);
    [
        ax,
        bx,
        frame[4] - 3.5 * (ax + bx),
        ay,
        by,
        frame[5] - 3.5 * (ay + by),
        0.,
        0.,
    ]
}

fn module(image: &[bool], w: usize, h: usize, grid: &[f32; 8], x: usize, y: usize) -> Option<bool> {
    let point = map(
        grid,
        crate::numeric::usize_f32(x) + 0.5,
        crate::numeric::usize_f32(y) + 0.5,
    );
    let xx = crate::numeric::f32_isize(point[0].floor());
    let yy = crate::numeric::f32_isize(point[1].floor());
    if xx < 0 || yy < 0 || xx >= w.cast_signed() || yy >= h.cast_signed() {
        return None;
    }
    Some(image[yy.cast_unsigned() * w + xx.cast_unsigned()])
}

/// Module-to-image transform with the finder center `center` at module (3.5, 3.5)
/// and per-module axis vectors `u` (along row 0) and `v` (along column 0).
fn axes_transform(center: [f32; 2], u: [f32; 2], v: [f32; 2]) -> [f32; 8] {
    [
        u[0],
        v[0],
        center[0] - 3.5 * (u[0] + v[0]),
        u[1],
        v[1],
        center[1] - 3.5 * (u[1] + v[1]),
        0.,
        0.,
    ]
}

/// Wrong modules of the timing pattern along row 0 (`axis` 0) or column 0.
fn timing_errors(
    image: &[bool],
    w: usize,
    h: usize,
    grid: &[f32; 8],
    size: usize,
    axis: usize,
) -> usize {
    (8..size)
        .map(|i| {
            let (x, y) = if axis == 0 { (i, 0) } else { (0, i) };
            module(image, w, h, grid, x, y).map_or(1, |dark| usize::from(dark != (i % 2 == 0)))
        })
        .sum()
}

/// Pitch and direction of each axis fitted to its timing pattern. The finder
/// fixes the origin; over 11 to 17 modules a few percent of scale or a degree of
/// angle moves the far modules by half a module. A coarse search over scale and
/// angle is followed by a fine one around its result; each takes the mean of
/// its best-fitting (scale, angle) pairs.
fn refine(image: &[bool], w: usize, h: usize, grid: &[f32; 8], size: usize) -> [f32; 8] {
    let center = map(grid, 3.5, 3.5);
    let mut axes = [[grid[0], grid[3]], [grid[1], grid[4]]];
    for (scale_unit, angle_unit, steps) in [(0.04, 0.035, (3_i16, 2_i16)), (0.015, 0.012, (1, 1))] {
        for axis in 0..2 {
            let base = axes[axis];
            let mut best = (usize::MAX, 0., 0., 0_u16);
            for scale_step in -steps.0..=steps.0 {
                for angle_step in -steps.1..=steps.1 {
                    let scale = 1. + scale_unit * f32::from(scale_step);
                    let (sin, cos) = (f32::from(angle_step) * angle_unit).sin_cos();
                    let candidate = [
                        (base[0] * cos - base[1] * sin) * scale,
                        (base[0] * sin + base[1] * cos) * scale,
                    ];
                    let mut trial = axes;
                    trial[axis] = candidate;
                    let wrong = timing_errors(
                        image,
                        w,
                        h,
                        &axes_transform(center, trial[0], trial[1]),
                        size,
                        axis,
                    );
                    if wrong < best.0 {
                        best = (wrong, 0., 0., 0);
                    }
                    if wrong == best.0 {
                        best.1 += scale;
                        best.2 += f32::from(angle_step) * angle_unit;
                        best.3 += 1;
                    }
                }
            }
            let count = f32::from(best.3);
            let (scale, angle) = (best.1 / count, best.2 / count);
            let (sin, cos) = angle.sin_cos();
            axes[axis] = [
                (base[0] * cos - base[1] * sin) * scale,
                (base[0] * sin + base[1] * cos) * scale,
            ];
        }
    }
    axes_transform(center, axes[0], axes[1])
}

/// The orientation's format reading, if close to a format word, ranked by format
/// bit errors and wrong timing/separator modules.
fn candidate(image: &[bool], w: usize, h: usize, grid: [f32; 8]) -> Option<Candidate> {
    let mut word = 0_u16;
    for i in 0..15 {
        let (x, y) = micro_qr::format_position(i);
        word = (word << 1) | u16::from(module(image, w, h, &grid, x, y)?);
    }
    let (symbol, mask, errors) = micro_qr::read_format(word)?;
    if errors > 2 {
        return None;
    }
    let size = micro_qr::size(symbol);
    let mut wrong = 0;
    for i in 8..size {
        let dark = i % 2 == 0;
        wrong += usize::from(module(image, w, h, &grid, i, 0)? != dark);
        wrong += usize::from(module(image, w, h, &grid, 0, i)? != dark);
    }
    for i in 0..8 {
        wrong += usize::from(module(image, w, h, &grid, i, 7)?);
        wrong += usize::from(module(image, w, h, &grid, 7, i)?);
    }
    // A quarter of the timing and separator modules may be wrong.
    let checked = 2 * (size - 8) + 16;
    (wrong * 4 <= checked).then_some(Candidate {
        score: 2 * usize::try_from(errors).unwrap_or(15) + wrong,
        transform: grid,
        symbol,
        mask,
        localization: (errors == 0 && wrong * 10 <= checked)
            .then(|| 1. - crate::numeric::usize_f32(wrong) / crate::numeric::usize_f32(checked)),
    })
}

pub(super) fn inside(polygon: &[[f32; 2]; 4], x: f32, y: f32) -> bool {
    let side = |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    let sides: [f32; 4] = std::array::from_fn(|i| side(polygon[i], polygon[(i + 1) % 4]));
    sides.iter().all(|&s| s >= 0.) || sides.iter().all(|&s| s <= 0.)
}

fn decode_finder(
    image: &[bool],
    w: usize,
    h: usize,
    finder: &Finder,
    budget: &mut Budget,
    limited: &mut bool,
    regions: &mut crate::regions::Regions,
) -> Option<Detection> {
    let mut quads = vec![(finder.quad?, 3.)];
    let mut region: Option<(f32, [[f32; 2]; 4])> = None;
    let mut tried_outer = false;
    while let Some((quad, span)) = quads.pop() {
        if budget.scored >= SCORED_QUADS {
            *limited = true;
            break;
        }
        budget.scored += 1;
        let mut candidates: Vec<Candidate> = Vec::new();
        for rotation in 0..4 {
            for mirror in [false, true] {
                let frame = single::frame(quad, rotation, mirror, span);
                // The best similarity correction of this orientation.
                if let Some(best) = TURNS
                    .iter()
                    .flat_map(|&turn| SCALES.iter().map(move |&scale| (turn, scale)))
                    .filter_map(|(turn, scale)| {
                        candidate(image, w, h, transform(&frame, scale, turn))
                    })
                    .min_by_key(|c| c.score)
                {
                    candidates.push(best);
                }
            }
        }
        candidates.sort_by_key(|c| c.score);
        for c in candidates.iter().take(FINDER_CANDIDATES) {
            let n = micro_qr::size(c.symbol);
            // An orientation whose score, with format bit errors counting double,
            // stays within a quarter of its checked modules is evidence for the Low
            // and Medium retry even when it fails to decode.
            regions.near_micro_qr |= c.score * 4 <= 2 * (n - 8) + 16;
            if let Some(score) = c.localization
                && region.is_none_or(|(best, _)| score > best)
            {
                let nf = crate::numeric::usize_f32(n);
                let polygon =
                    [[0., 0.], [nf, 0.], [nf, nf], [0., nf]].map(|[x, y]| map(&c.transform, x, y));
                region = Some((score, polygon));
            }
            let mut refined = None;
            // The finder's grid first; the timing-fitted one only when it fails.
            for k in 0..=OFFSETS.len() {
                if budget.attempts >= ATTEMPTS {
                    *limited = true;
                    return None;
                }
                budget.attempts += 1;
                let (t, offset) = if k == 0 {
                    (&c.transform, 0.)
                } else {
                    (
                        &*refined.get_or_insert_with(|| refine(image, w, h, &c.transform, n)),
                        OFFSETS[k - 1],
                    )
                };
                let Some(grid) = sample(image, w, h, n, t, offset) else {
                    continue;
                };
                if let Some(read) = micro_qr::decode_matrix(&grid, n, c.symbol, c.mask) {
                    let nf = crate::numeric::usize_f32(n);
                    return Some(Detection {
                        assembled_rows: false,
                        bytes: Some(read.bytes),
                        structured_append: None,
                        reader_initialization: false,
                        addon: None,
                        format: "MicroQRCode".into(),
                        text: read.text,
                        polygon: [[0., 0.], [nf, 0.], [nf, nf], [0., nf]]
                            .map(|[x, y]| map(t, x, y)),
                        support: finder.support,
                        error: crate::numeric::usize_f32(read.corrected),
                        codabar_start_stop: None,
                        gs1: false,
                    });
                }
            }
        }
        // The outer ring gives a longer orientation and scale baseline.
        if !tried_outer {
            tried_outer = true;
            if let Some(outer) = single::outer_quad(image, w, h, finder) {
                quads.push((outer, 7.));
            }
        }
    }
    // Undecoded coverage: the best-localized orientation that failed to decode.
    if let Some((score, polygon)) = region {
        regions.add("MicroQRCode", polygon, score, finder.support);
    }
    None
}

/// Try every finder that no read claims as a Micro QR finder; well-localized
/// symbols that fail to decode become undecoded regions. Finders are tried in
/// the given order, by support. Returns whether the decode or scoring budget cut
/// the search.
#[expect(
    clippy::too_many_arguments,
    reason = "The pass shares its image, finders, reads, claimed finders, work budget and undecoded regions."
)]
pub(super) fn recover(
    image: &[bool],
    w: usize,
    h: usize,
    finders: &[Finder],
    results: &mut Vec<Detection>,
    used: &mut Vec<Finder>,
    budget: &mut Budget,
    regions: &mut crate::regions::Regions,
) -> bool {
    let mut limited = false;
    for finder in finders {
        if used
            .iter()
            .any(|prior| distance(finder, prior) < finder.module.min(prior.module) * 2.)
            || results
                .iter()
                .any(|read| inside(&read.polygon, finder.x, finder.y))
        {
            continue;
        }
        if let Some(read) = decode_finder(image, w, h, finder, budget, &mut limited, regions) {
            results.push(read);
            used.push(finder.clone());
        }
        if budget.attempts >= ATTEMPTS {
            break;
        }
    }
    limited
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{Budget, SCORED_QUADS, recover};

    /// A frame of `count` isolated finder patterns with two-pixel modules.
    fn finder_grid(count: usize) -> (Vec<bool>, usize, usize) {
        let (columns, cell) = (25, 23);
        let (width, height) = (columns * cell, count.div_ceil(columns) * cell + 8);
        let mut bits = vec![false; width * height];
        for i in 0..count {
            let (left, top) = ((i % columns) * cell + 4, (i / columns) * cell + 4);
            for y in 0..14 {
                for x in 0..14 {
                    let (a, b) = (x / 2, y / 2);
                    bits[(top + y) * width + left + x] = a == 0
                        || a == 6
                        || b == 0
                        || b == 6
                        || ((2..=4).contains(&a) && (2..=4).contains(&b));
                }
            }
        }
        (bits, width, height)
    }

    fn run(count: usize) -> (bool, Budget, usize) {
        let (bits, w, h) = finder_grid(count);
        let (finders, _) = super::super::finders(&bits, w, h);
        let with_quads = finders.iter().filter(|f| f.quad.is_some()).count();
        let mut budget = Budget::default();
        let mut results = Vec::new();
        let limited = recover(
            &bits,
            w,
            h,
            &finders,
            &mut results,
            &mut Vec::new(),
            &mut budget,
            &mut crate::regions::Regions::default(),
        );
        assert!(results.is_empty());
        (limited, budget, with_quads)
    }

    #[test]
    fn finder_heavy_frames_report_the_scoring_cut() {
        let (limited, budget, with_quads) = run(500);
        assert!(with_quads > SCORED_QUADS);
        assert!(limited);
        assert_eq!(budget.scored, SCORED_QUADS);
        assert_eq!(budget.attempts, 0);
    }

    #[test]
    fn few_finders_are_scored_without_a_cut() {
        let (limited, budget, with_quads) = run(40);
        assert!(with_quads > 0);
        assert!(!limited);
        assert!(budget.scored >= with_quads && budget.scored <= 2 * with_quads);
    }
}

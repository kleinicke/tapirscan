//! Piecewise sampling of large QR symbols between their alignment patterns.
//!
//! One homography from three finders and one alignment pattern drifts by more
//! than half a module across a version 25-40 symbol on paper that is not flat.
//! This retry locates every alignment pattern near where its found neighbours
//! put it and samples each cell of the alignment grid with its own homography.
//! It only proposes a grid; the QR payload and error correction validate it.
use super::{homography, map};
use crate::qr;

/// The smallest version with a full grid of alignment patterns (3 x 3).
pub(super) const MIN_VERSION: usize = 7;

/// Search radius around a predicted alignment centre, in modules.
const SEARCH_MODULES: f32 = 2.5;

/// Mismatches of the 5 x 5 alignment pattern accepted at a found centre.
const MAX_ERRORS: usize = 3;

fn pixel(image: &[bool], [w, h]: [usize; 2], p: [f32; 2]) -> Option<bool> {
    if !p[0].is_finite() || !p[1].is_finite() {
        return None;
    }
    let x = crate::numeric::f32_isize(p[0].floor());
    let y = crate::numeric::f32_isize(p[1].floor());
    (x >= 0 && y >= 0 && x < w.cast_signed() && y < h.cast_signed())
        .then(|| image[y.cast_unsigned() * w + x.cast_unsigned()])
}

/// Local module axes of `t` at module coordinate `(x, y)`.
fn axes(t: &[f32; 8], x: f32, y: f32) -> ([f32; 2], [f32; 2]) {
    let p = map(t, x, y);
    let px = map(t, x + 1., y);
    let py = map(t, x, y + 1.);
    ([px[0] - p[0], px[1] - p[1]], [py[0] - p[0], py[1] - p[1]])
}

/// The alignment centre nearest `guess` with the fewest pattern mismatches.
fn find(
    image: &[bool],
    size: [usize; 2],
    guess: [f32; 2],
    ex: [f32; 2],
    ey: [f32; 2],
) -> Option<[f32; 2]> {
    let module = f32::midpoint(ex[0].hypot(ex[1]), ey[0].hypot(ey[1]));
    let step = (module / 3.).max(1.);
    let reach = SEARCH_MODULES * module;
    let steps = crate::numeric::f32_isize((reach / step).ceil());
    let mut best: Option<(f32, [f32; 2])> = None;
    for sy in -steps..=steps {
        for sx in -steps..=steps {
            let dx = crate::numeric::isize_f32(sx) * step;
            let dy = crate::numeric::isize_f32(sy) * step;
            let distance = dx.hypot(dy);
            if distance > reach {
                continue;
            }
            let centre = [guess[0] + dx, guess[1] + dy];
            let mut errors = 0;
            'pattern: for my in -2_i8..=2 {
                for mx in -2_i8..=2 {
                    let (fx, fy) = (f32::from(mx), f32::from(my));
                    let p = [
                        centre[0] + fx * ex[0] + fy * ey[0],
                        centre[1] + fx * ex[1] + fy * ey[1],
                    ];
                    let expected = mx.abs().max(my.abs()) != 1;
                    if pixel(image, size, p) != Some(expected) {
                        errors += 1;
                        if errors > MAX_ERRORS {
                            break 'pattern;
                        }
                    }
                }
            }
            if errors > MAX_ERRORS {
                continue;
            }
            let score = crate::numeric::usize_f32(errors) * 3. + distance / module;
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, centre));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Pixel positions of the alignment grid's centres (finder corners included),
/// row by row, located from `t` and the displacement of found neighbours.
fn locate(image: &[bool], size: [usize; 2], t: &[f32; 8], centres: &[usize]) -> Vec<[f32; 2]> {
    let k = centres.len();
    let module = |i: usize| crate::numeric::usize_f32(centres[i]) + 0.5;
    let mut points = vec![[0.; 2]; k * k];
    let mut found = vec![false; k * k];
    // Grid positions sorted outwards from the top-left finder, so neighbours
    // above and to the left come first.
    let mut order: Vec<(usize, usize)> = (0..k).flat_map(|j| (0..k).map(move |i| (i, j))).collect();
    order.sort_by_key(|&(i, j)| (i + j, j));
    for (i, j) in order {
        let (x, y) = (module(i), module(j));
        let predicted = map(t, x, y);
        // The three corners beside the finders are where `t` was fitted.
        if (i == 0 || j == 0) && (i == 0 || i == k - 1) && (j == 0 || j == k - 1) {
            points[j * k + i] = predicted;
            found[j * k + i] = true;
            continue;
        }
        let at = |ni: usize, nj: usize| found[nj * k + ni].then(|| points[nj * k + ni]);
        // Found neighbours to the left, above and diagonally predict a centre
        // with the local scale and skew of the grid; otherwise `t` shifted by
        // the displacement of whichever neighbours were found.
        let parallelogram = (i > 0 && j > 0)
            .then(|| Some((at(i - 1, j)?, at(i, j - 1)?, at(i - 1, j - 1)?)))
            .flatten()
            .map(|(l, u, d)| [l[0] + u[0] - d[0], l[1] + u[1] - d[1]]);
        let mut shift = [0.; 2];
        let mut count = 0.;
        for (ni, nj) in [(-1, 0), (0, -1), (-1, -1), (1, -1), (-1, 1)] {
            let (Some(ni), Some(nj)) = (i.checked_add_signed(ni), j.checked_add_signed(nj)) else {
                continue;
            };
            if ni < k
                && nj < k
                && let Some(p) = at(ni, nj)
            {
                let q = map(t, module(ni), module(nj));
                shift[0] += p[0] - q[0];
                shift[1] += p[1] - q[1];
                count += 1.;
            }
        }
        let guess = parallelogram.unwrap_or(if count > 0. {
            [
                predicted[0] + shift[0] / count,
                predicted[1] + shift[1] / count,
            ]
        } else {
            predicted
        });
        let (ex, ey) = axes(t, x, y);
        match find(image, size, guess, ex, ey) {
            Some(p) => {
                points[j * k + i] = p;
                found[j * k + i] = true;
            }
            None => points[j * k + i] = guess,
        }
    }
    points
}

/// Versions a read version may differ from the finder estimate.
const MAX_VERSION_SHIFT: usize = 2;

/// Bit distance up to which the BCH(18, 6) version word corrects errors.
const VERSION_ERRORS: u32 = 3;

/// The version read from either version-information block of a symbol that
/// `t` maps as `n` modules wide. The blocks are read relative to the top-right
/// and bottom-left finder centres, so a nearby wrong `n` still finds them.
pub(super) fn read_version(
    image: &[bool],
    size: [usize; 2],
    n: usize,
    t: &[f32; 8],
) -> Option<usize> {
    let far = crate::numeric::usize_f32(n) - 3.5;
    let mut best: Option<(u32, usize)> = None;
    for transposed in [false, true] {
        let mut word = 0_u32;
        for bit in 0..18_u8 {
            // Bit i lies at column i % 3 - 7 and row i / 3 - 3 from the top-right
            // finder centre; the bottom-left copy is its transpose.
            let along = f32::from(bit % 3) - 7.;
            let across = f32::from(bit / 3) - 3.;
            let p = if transposed {
                map(t, 3.5 + across, far + along)
            } else {
                map(t, far + along, 3.5 + across)
            };
            word |= u32::from(pixel(image, size, p)?) << bit;
        }
        for (i, &expected) in super::single::WORDS.iter().enumerate() {
            let distance = (word ^ expected).count_ones();
            if distance <= VERSION_ERRORS && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, i + MIN_VERSION));
            }
        }
    }
    best.map(|(_, version)| version)
}

/// Samples the symbol that `t` maps as a `candidate` version as `version`
/// (read with [`read_version`]), cell by cell between its alignment patterns,
/// and decodes it in both orientations.
pub(super) fn recover(
    image: &[bool],
    size: [usize; 2],
    candidate: usize,
    version: usize,
    t: &[f32; 8],
) -> Option<(qr::Payload, [[f32; 2]; 4])> {
    // The finder distance bounds the size: a version far from it belongs to a
    // false triple.
    if version < MIN_VERSION || version.abs_diff(candidate) > MAX_VERSION_SHIFT {
        return None;
    }
    let old = 17 + 4 * candidate;
    let n = 17 + 4 * version;
    // Re-index `t` to the read size at the finder centres and the bottom-right
    // alignment centre.
    let reindex = |m: usize| {
        let m = crate::numeric::usize_f32(m);
        [
            [3.5, 3.5],
            [m - 3.5, 3.5],
            [m - 6.5, m - 6.5],
            [3.5, m - 3.5],
        ]
    };
    let t = &homography(reindex(n), reindex(old).map(|[x, y]| map(t, x, y)))?;
    let centres = qr::alignment_positions(version);
    let k = centres.len();
    let points = locate(image, size, t, &centres);
    if std::env::var_os("QRDBG").is_some() {
        eprintln!("grid v{version} at {:?}", map(t, 0., 0.));
    }
    let module = |i: usize| crate::numeric::usize_f32(centres[i]) + 0.5;
    let mut cells = Vec::with_capacity((k - 1) * (k - 1));
    for j in 0..k - 1 {
        for i in 0..k - 1 {
            let src = [
                [module(i), module(j)],
                [module(i + 1), module(j)],
                [module(i + 1), module(j + 1)],
                [module(i), module(j + 1)],
            ];
            let dst = [
                points[j * k + i],
                points[j * k + i + 1],
                points[(j + 1) * k + i + 1],
                points[(j + 1) * k + i],
            ];
            cells.push(homography(src, dst)?);
        }
    }
    // Modules beyond the outer centres use the outermost cell.
    let cell_of = |m: usize| centres[1..k - 1].iter().take_while(|&&c| c <= m).count();
    let lookup: Vec<usize> = (0..n).map(cell_of).collect();
    let mut matrix = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let cell = &cells[lookup[y] * (k - 1) + lookup[x]];
            let p = map(
                cell,
                crate::numeric::usize_f32(x) + 0.5,
                crate::numeric::usize_f32(y) + 0.5,
            );
            matrix.push(pixel(image, size, p)?);
        }
    }
    let nf = crate::numeric::usize_f32(n);
    let last = k - 2;
    let polygon = [
        map(&cells[0], 0., 0.),
        map(&cells[last], nf, 0.),
        map(&cells[last * (k - 1) + last], nf, nf),
        map(&cells[last * (k - 1)], 0., nf),
    ];
    if std::env::var_os("QRDBG").is_some() {
        eprintln!(
            "  sampled; format_intact={}",
            qr::format_intact(n, |x, y| Some(matrix[y * n + x]))
        );
    }
    [false, true].into_iter().find_map(|mirror| {
        let matrix = super::transpose_if_mirrored(&matrix, n, mirror);
        qr::decode_matrix(&matrix, n).map(|read| (read, polygon))
    })
}

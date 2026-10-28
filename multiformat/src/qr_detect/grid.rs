//! Piecewise sampling of large QR symbols between their alignment patterns.
//!
//! One homography from three finders and one alignment pattern drifts by more
//! than half a module across a version 25-40 symbol on paper that is not flat.
//! This retry locates every alignment pattern near where its found neighbours
//! put it and samples each cell of the alignment grid with its own homography.
//! It only proposes a grid; the QR payload and error correction validate it.
use super::{homography, map, pixel};
use crate::qr;

/// The smallest version with a full grid of alignment patterns (3 x 3).
pub(super) const MIN_VERSION: usize = 7;

/// Search radius around a predicted alignment centre, in modules.
const SEARCH_MODULES: f32 = 2.5;

/// Mismatches of the 5 x 5 alignment pattern accepted at a found centre.
const MAX_ERRORS: usize = 3;

/// Local module axes of `t` at module coordinate `(x, y)`.
fn axes(t: &[f32; 8], x: f32, y: f32) -> ([f32; 2], [f32; 2]) {
    let p = map(t, x, y);
    let px = map(t, x + 1., y);
    let py = map(t, x, y + 1.);
    ([px[0] - p[0], px[1] - p[1]], [py[0] - p[0], py[1] - p[1]])
}

/// The alignment centre nearest `guess` with the fewest pattern mismatches. Unlike
/// [`super::alignment_cached::search`], which looks 8 modules around a guess from the
/// finders alone, this looks 2.5 modules around a guess from found neighbours, so a
/// pattern in the data area nearby does not win.
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
                    if pixel(image, size[0], size[1], p) != Some(expected) {
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
/// row by row, located from `transform` and the displacement of found neighbours.
fn locate(
    image: &[bool],
    size: [usize; 2],
    transform: &[f32; 8],
    centres: &[usize],
) -> Vec<[f32; 2]> {
    let span = centres.len();
    let last = span - 1;
    let module = |index: usize| crate::numeric::usize_f32(centres[index]) + 0.5;
    let mut points = vec![[0.; 2]; span * span];
    let mut found = vec![false; span * span];
    // Grid positions sorted outwards from the top-left finder, so neighbours
    // above and to the left come first.
    let mut order: Vec<(usize, usize)> = (0..span)
        .flat_map(|row| (0..span).map(move |column| (column, row)))
        .collect();
    order.sort_by_key(|&(column, row)| (column + row, row));
    for (column, row) in order {
        let (mx, my) = (module(column), module(row));
        let predicted = map(transform, mx, my);
        let cell = row * span + column;
        // The three corners beside the finders lie 3 modules from the finder centres
        // `transform` was fitted to; no alignment pattern marks them.
        if (column == 0 && (row == 0 || row == last)) || (row == 0 && column == last) {
            points[cell] = predicted;
            found[cell] = true;
            continue;
        }
        let at = |c: usize, r: usize| found[r * span + c].then(|| points[r * span + c]);
        // Found neighbours to the left, above and diagonally predict a centre
        // with the local scale and skew of the grid; otherwise `transform` shifted
        // by the displacement of whichever neighbours were found.
        let parallelogram = (column > 0 && row > 0)
            .then(|| {
                Some((
                    at(column - 1, row)?,
                    at(column, row - 1)?,
                    at(column - 1, row - 1)?,
                ))
            })
            .flatten()
            .map(|(left, up, diagonal)| {
                [left[0] + up[0] - diagonal[0], left[1] + up[1] - diagonal[1]]
            });
        let mut shift = [0.; 2];
        let mut count = 0.;
        for (dc, dr) in [(-1, 0), (0, -1), (-1, -1), (1, -1)] {
            let (Some(c), Some(r)) = (column.checked_add_signed(dc), row.checked_add_signed(dr))
            else {
                continue;
            };
            if c < span
                && r < span
                && let Some(point) = at(c, r)
            {
                let expected = map(transform, module(c), module(r));
                shift[0] += point[0] - expected[0];
                shift[1] += point[1] - expected[1];
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
        let (ex, ey) = axes(transform, mx, my);
        match find(image, size, guess, ex, ey) {
            Some(point) => {
                points[cell] = point;
                found[cell] = true;
            }
            None => points[cell] = guess,
        }
    }
    points
}

/// Versions the symbol may differ from the finder estimate.
const MAX_VERSION_SHIFT: usize = 2;

/// Bit distance up to which the BCH(18, 6) version word corrects errors.
const VERSION_ERRORS: u32 = 3;

/// `t`, which maps a symbol `from` modules wide, re-indexed to `to` modules at
/// the finder centres and the bottom-right alignment centre.
fn reindex(t: &[f32; 8], from: usize, to: usize) -> Option<[f32; 8]> {
    let centres = |n: usize| {
        let n = crate::numeric::usize_f32(n);
        [
            [3.5, 3.5],
            [n - 3.5, 3.5],
            [n - 6.5, n - 6.5],
            [3.5, n - 3.5],
        ]
    };
    homography(centres(to), centres(from).map(|[x, y]| map(t, x, y)))
}

/// The version within [`MAX_VERSION_SHIFT`] of `candidate` whose word one of
/// the version-information blocks (beside the top-right and bottom-left finders)
/// holds; `t` maps the symbol as a `candidate` version. The blocks are read at
/// the module size of every version in that range: the finder estimate can be a
/// module off near a finder, so the block of one size may be read at another.
pub(super) fn read_version(
    image: &[bool],
    size: [usize; 2],
    candidate: usize,
    t: &[f32; 8],
) -> Option<usize> {
    let mut best: Option<(u32, usize)> = None;
    let lowest = candidate.saturating_sub(MAX_VERSION_SHIFT).max(MIN_VERSION);
    let highest = (candidate + MAX_VERSION_SHIFT).min(40);
    for scale in lowest..=highest {
        let n = 17 + 4 * scale;
        let Some(t) = reindex(t, 17 + 4 * candidate, n) else {
            continue;
        };
        let far = crate::numeric::usize_f32(n) - 3.5;
        for transposed in [false, true] {
            let mut word = 0_u32;
            for bit in 0..18_u8 {
                // Bit i lies at column i % 3 - 7 and row i / 3 - 3 from the
                // top-right finder centre; the bottom-left copy is its transpose.
                let along = f32::from(bit % 3) - 7.;
                let across = f32::from(bit / 3) - 3.;
                let p = if transposed {
                    map(&t, 3.5 + across, far + along)
                } else {
                    map(&t, far + along, 3.5 + across)
                };
                word |= u32::from(pixel(image, size[0], size[1], p).unwrap_or(false)) << bit;
            }
            for version in lowest..=highest {
                let expected = super::single::WORDS[version - MIN_VERSION];
                let distance = (word ^ expected).count_ones();
                if distance <= VERSION_ERRORS && best.is_none_or(|(d, _)| distance < d) {
                    best = Some((distance, version));
                }
            }
        }
    }
    best.map(|(_, version)| version)
}

/// Samples the symbol that `t` maps as a `candidate` version as `version`
/// (see [`read_version`]), cell by cell between its alignment patterns, and
/// decodes it in both orientations.
pub(super) fn recover(
    image: &[bool],
    size: [usize; 2],
    candidate: usize,
    version: usize,
    t: &[f32; 8],
) -> Option<(qr::Payload, [[f32; 2]; 4])> {
    // A version far from the one whose header passed belongs to a false triple.
    if version < MIN_VERSION || version.abs_diff(candidate) > MAX_VERSION_SHIFT {
        return None;
    }
    let n = 17 + 4 * version;
    let t = &reindex(t, 17 + 4 * candidate, n)?;
    let centres = qr::alignment_positions(version);
    let k = centres.len();
    let points = locate(image, size, t, &centres);
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
            matrix.push(pixel(image, size[0], size[1], p)?);
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
    [false, true].into_iter().find_map(|mirror| {
        let matrix = super::transpose_if_mirrored(&matrix, n, mirror);
        qr::decode_matrix(&matrix, n).map(|read| (read, polygon))
    })
}

#[cfg(test)]
mod tests {
    use super::{read_version, recover};
    use crate::numeric::{f32_usize, usize_f32};
    use crate::qr_detect::{Pass, Policy, detect_with, homography};
    use crate::{binarization::Images, regions::Regions};

    const TEXT: &str = "TAPIRSCAN ALIGNMENT GRID TEST: VERSION SEVEN";
    /// Version 7-M `TEXT`, one row of 45 modules per entry, left-aligned in 48 bits.
    const SEVEN: [&str; 45] = [
        "fe386b380bf8",
        "829da0939208",
        "ba0c8bca92e8",
        "ba59a1e8dae8",
        "ba86afc23ae8",
        "823d48c88208",
        "feaaaaaaabf8",
        "005f18823000",
        "aa7abff8a890",
        "840e935a2c28",
        "2f24e96a8488",
        "94b88bc52a38",
        "83dbe36f18a8",
        "806f0c45b240",
        "7a39deef3ae8",
        "5046f436d540",
        "e64ede91e768",
        "c4a6533bcdb0",
        "c6572f366710",
        "8c488dc23a38",
        "cfbeafe88fb0",
        "38ca88c238f0",
        "5aeadaeabaa8",
        "f8c2e88568b0",
        "6fbccfdf0f88",
        "24ed50c5cf40",
        "ab8cd5cf75f8",
        "b5fb27325558",
        "4f64a590e7f8",
        "98cd95ba54b0",
        "aa7ba3b2ea10",
        "8007d1463e38",
        "f75c0bef1ab0",
        "fd48794728c0",
        "0a0f9cec3268",
        "79df0eb540c0",
        "9a491f976fe8",
        "00ea68bdc8b8",
        "fe21aab77ac0",
        "825fd8ea48c0",
        "ba908ff8bfc8",
        "ba6823fa6220",
        "ba8a0b128888",
        "82286c452238",
        "fe88b4ef0aa8",
    ];
    const SCALE: usize = 6;
    const QUIET: usize = 4;

    fn dark(x: usize, y: usize) -> bool {
        u64::from_str_radix(SEVEN[y], 16).unwrap() >> (47 - x) & 1 == 1
    }

    /// The symbol with `SCALE`-pixel modules in a square gray frame, pushed down by
    /// a bump of `bump` modules inside its lower right quarter, as on paper that is
    /// not flat; the finders, timing patterns and format information stay in place.
    /// `mirror` draws the symbol transposed, as seen through the back of a label.
    fn draw(bump: f32, mirror: bool) -> (Vec<u8>, usize) {
        let size = (45 + 2 * QUIET + 4) * SCALE;
        let scale = usize_f32(SCALE);
        let mut gray = vec![220_u8; size * size];
        for py in 0..size {
            for px in 0..size {
                let u = (usize_f32(px) + 0.5) / scale - usize_f32(QUIET);
                let v0 = (usize_f32(py) + 0.5) / scale - usize_f32(QUIET);
                let (a, b) = (u / 45., v0 / 45.);
                let v = v0 - bump * (-((a - 0.62).powi(2) + (b - 0.62).powi(2)) / 0.045).exp();
                if (0. ..45.).contains(&u)
                    && (0. ..45.).contains(&v)
                    && if mirror {
                        dark(f32_usize(v), f32_usize(u))
                    } else {
                        dark(f32_usize(u), f32_usize(v))
                    }
                {
                    gray[py * size + px] = 30;
                }
            }
        }
        (gray, size)
    }

    fn medium_reads(gray: &[u8], size: usize, grid_budget: usize) -> Vec<String> {
        let mut images = Images::new(gray, size, size);
        let policy = Policy {
            grid_budget,
            ..Policy::for_mode(1)
        };
        detect_with(
            size,
            size,
            &mut Regions::default(),
            &mut images,
            policy,
            Pass::effort(false, false),
        )
        .0
        .into_iter()
        .map(|read| read.text)
        .collect()
    }

    #[test]
    fn bent_symbols_read_only_cell_by_cell() {
        // The bump bends the rows between the alignment patterns; one homography
        // through the finders and an alignment pattern misses it by a module.
        for mirror in [false, true] {
            let (gray, size) = draw(2., mirror);
            assert_eq!(medium_reads(&gray, size, 0), Vec::<String>::new());
            assert_eq!(
                medium_reads(&gray, size, Policy::for_mode(1).grid_budget),
                [TEXT],
                "mirror {mirror}"
            );
        }
        // A flat symbol needs no retry.
        let (flat, size) = draw(0., false);
        assert_eq!(medium_reads(&flat, size, 0), [TEXT]);
    }

    #[test]
    fn the_version_comes_from_the_symbol() {
        let (gray, size) = draw(0., false);
        let bits: Vec<bool> = gray.iter().map(|&v| v < 128).collect();
        let pixel = |m: f32| (m + usize_f32(QUIET)) * usize_f32(SCALE);
        for candidate in [5, 6, 8, 9] {
            // Finder and bottom-right alignment centres as a `candidate` grid sees them.
            let n = usize_f32(17 + 4 * candidate);
            let t = homography(
                [
                    [3.5, 3.5],
                    [n - 3.5, 3.5],
                    [n - 6.5, n - 6.5],
                    [3.5, n - 3.5],
                ],
                [[3.5, 3.5], [41.5, 3.5], [38.5, 38.5], [3.5, 41.5]]
                    .map(|[x, y]| [pixel(x), pixel(y)]),
            )
            .unwrap();
            assert_eq!(read_version(&bits, [size, size], candidate, &t), Some(7));
            let (read, polygon) = recover(&bits, [size, size], candidate, 7, &t).unwrap();
            assert_eq!(read.text, TEXT);
            assert!((polygon[2][0] - pixel(45.)).abs() < 1., "{polygon:?}");
        }
        // Too far from the finder estimate: a false triple.
        let t = homography(
            [[3.5, 3.5], [37.5, 3.5], [34.5, 34.5], [3.5, 37.5]],
            [[3.5, 3.5], [41.5, 3.5], [38.5, 38.5], [3.5, 41.5]].map(|[x, y]| [pixel(x), pixel(y)]),
        )
        .unwrap();
        assert!(recover(&bits, [size, size], 4, 7, &t).is_none());
    }
}

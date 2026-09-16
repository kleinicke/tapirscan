//! Bounded two-finder recovery after the ordinary three-finder search finishes.
//! No image labels, supplied dimensions or reference decoder are used.
use super::{distance, homography, map, Finder};
use crate::{qr, Detection};
type Point = [f32; 2];

fn fit(pairs: &[(Point, Point)], dimension: usize, target_scale: f32) -> Option<[f32; 8]> {
    let mut equations = [[0_f64; 9]; 8];
    for &(source, target) in pairs {
        let source_x = f64::from(source[0]) / dimension as f64;
        let source_y = f64::from(source[1]) / dimension as f64;
        let target_x = f64::from(target[0]) / f64::from(target_scale);
        let target_y = f64::from(target[1]) / f64::from(target_scale);
        for (equation_row, equation_value) in [
            (
                [
                    source_x,
                    source_y,
                    1.,
                    0.,
                    0.,
                    0.,
                    -target_x * source_x,
                    -target_x * source_y,
                ],
                target_x,
            ),
            (
                [
                    0.,
                    0.,
                    0.,
                    source_x,
                    source_y,
                    1.,
                    -target_y * source_x,
                    -target_y * source_y,
                ],
                target_y,
            ),
        ] {
            for equation_index in 0..8 {
                for column_index in 0..8 {
                    equations[equation_index][column_index] +=
                        equation_row[equation_index] * equation_row[column_index];
                }
                equations[equation_index][8] += equation_row[equation_index] * equation_value;
            }
        }
    }
    for col in 0..8 {
        let pivot = (col..8)
            .max_by(|&a, &b| equations[a][col].abs().total_cmp(&equations[b][col].abs()))?;
        equations.swap(col, pivot);
        let divisor = equations[col][col];
        if divisor.abs() < 1e-11 {
            return None;
        }
        for value in &mut equations[col][col..] {
            *value /= divisor;
        }
        let pivot_row = equations[col];
        for (row, values) in equations.iter_mut().enumerate() {
            if row == col {
                continue;
            }
            let factor = values[col];
            for i in col..9 {
                values[i] -= factor * pivot_row[i];
            }
        }
    }
    let dimension_f32 = dimension as f32;
    Some(std::array::from_fn(|i| {
        equations[i][8] as f32 * if i < 6 { target_scale } else { 1. }
            / if i == 2 || i == 5 { 1. } else { dimension_f32 }
    }))
}
fn fitted(t: &[f32; 8], known: [(&Finder, Point); 2], n: usize, scale: f32) -> Option<[f32; 8]> {
    let mut pairs = Vec::new();
    for (finder, center) in known {
        let quad = finder.quad?;
        let source = [[-1.5, -1.5], [1.5, -1.5], [1.5, 1.5], [-1.5, 1.5]]
            .map(|[x, y]| [center[0] + x, center[1] + y]);
        let expected = source.map(|[x, y]| map(t, x, y));
        let (_, ordered) = (0..4)
            .flat_map(|start| {
                [false, true].map(move |mirror| {
                    let ordered: [Point; 4] =
                        std::array::from_fn(|i| quad[(start + if mirror { 4 - i } else { i }) % 4]);
                    let error = (0..4)
                        .map(|i| {
                            (ordered[i][0] - expected[i][0]).powi(2)
                                + (ordered[i][1] - expected[i][1]).powi(2)
                        })
                        .sum::<f32>();
                    (error, ordered)
                })
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        pairs.extend(source.into_iter().zip(ordered));
    }
    fit(&pairs, n, scale)
}
fn pitch(finder: &Finder) -> f32 {
    let q = finder.quad.unwrap();
    (0..4)
        .map(|i| (q[i][0] - q[(i + 1) % 4][0]).hypot(q[i][1] - q[(i + 1) % 4][1]))
        .sum::<f32>()
        / 12.
}
fn finder_errors(image: &[bool], w: usize, h: usize, finder: &Finder) -> u32 {
    let Some(t) = homography(
        [[-1.5, -1.5], [1.5, -1.5], [1.5, 1.5], [-1.5, 1.5]],
        finder.quad.unwrap(),
    ) else {
        return 49;
    };
    let mut errors = 0;
    for y in -3_i32..=3 {
        for x in -3_i32..=3 {
            let expected = x.abs().max(y.abs()) != 2;
            errors += u32::from(read_pixel(image, w, h, &t, x as f32, y as f32) != expected);
        }
    }
    errors
}
fn read_pixel(image: &[bool], w: usize, h: usize, transform: &[f32; 8], x: f32, y: f32) -> bool {
    let [mapped_x, mapped_y] = map(transform, x, y);
    let pixel_x = mapped_x.floor() as isize;
    let pixel_y = mapped_y.floor() as isize;
    pixel_x >= 0
        && pixel_y >= 0
        && pixel_x < w as isize
        && pixel_y < h as isize
        && image[pixel_y as usize * w + pixel_x as usize]
}
pub(super) fn recover(
    image: &[bool],
    w: usize,
    h: usize,
    finders: &[Finder],
    results: &mut Vec<Detection>,
    used: &mut Vec<Finder>,
    regions: &mut crate::regions::Regions,
) -> bool {
    let mut remaining: Vec<_> = finders
        .iter()
        .filter(|f| {
            f.support >= 2
                && f.quad.is_some()
                && !used
                    .iter()
                    .any(|q| distance(f, q) < f.module.min(q.module) * 2.)
        })
        .collect();
    if remaining.len() < 2 {
        return false;
    }
    remaining.sort_by_cached_key(|finder| {
        (
            finder_errors(image, w, h, finder),
            std::cmp::Reverse(finder.support),
        )
    });
    let limited = remaining.len() > 12;
    remaining.truncate(12);
    let mut attempts = 0;
    for a in 0..remaining.len() {
        'pair: for b in a + 1..remaining.len() {
            let first = remaining[a];
            let second = remaining[b];
            if [first, second].iter().any(|f| {
                used.iter()
                    .any(|q| distance(f, q) < f.module.min(q.module) * 2.)
            }) {
                continue;
            }
            let ma = pitch(first);
            let mb = pitch(second);
            if !(0.6..=1.7).contains(&(ma / mb)) {
                continue;
            }
            let module = (ma + mb) * 0.5;
            let separation = distance(first, second);
            if separation < module * 10. {
                continue;
            }
            for delta in [0, -1, 1, -2, 2, -3, 3] {
                for role in 0..6 {
                    let (first_finder, second_finder) = if role % 2 == 0 {
                        (first, second)
                    } else {
                        (second, first)
                    };
                    let pa = [first_finder.x, first_finder.y];
                    let pb = [second_finder.x, second_finder.y];
                    let dx = pb[0] - pa[0];
                    let dy = pb[1] - pa[1];
                    let (tl, tr, bl) = match role / 2 {
                        0 => (pa, pb, [pa[0] - dy, pa[1] + dx]),
                        1 => (pa, [pa[0] + dy, pa[1] - dx], pb),
                        _ => (
                            [(pa[0] + pb[0] - dy) * 0.5, (pa[1] + pb[1] + dx) * 0.5],
                            pa,
                            pb,
                        ),
                    };
                    let leg = separation
                        / if role >= 4 {
                            std::f32::consts::SQRT_2
                        } else {
                            1.
                        };
                    let version = ((leg / module + 7. - 17.) / 4.).round() as i32 + delta;
                    if !(1..=40).contains(&version) {
                        continue;
                    }
                    let n = (17 + 4 * version) as usize;
                    let nf = n as f32;
                    let br = [tr[0] + bl[0] - tl[0], tr[1] + bl[1] - tl[1]];
                    let Some(base) = homography(
                        [
                            [3.5, 3.5],
                            [nf - 3.5, 3.5],
                            [nf - 3.5, nf - 3.5],
                            [3.5, nf - 3.5],
                        ],
                        [tl, tr, br, bl],
                    ) else {
                        continue;
                    };
                    let known = match role / 2 {
                        0 => [(first_finder, [3.5, 3.5]), (second_finder, [nf - 3.5, 3.5])],
                        1 => [(first_finder, [3.5, 3.5]), (second_finder, [3.5, nf - 3.5])],
                        _ => [
                            (first_finder, [nf - 3.5, 3.5]),
                            (second_finder, [3.5, nf - 3.5]),
                        ],
                    };
                    for t in std::iter::once(base).chain(fitted(&base, known, n, w.max(h) as f32)) {
                        attempts += 1;
                        if attempts > 128 {
                            return true;
                        }
                        if !qr::plausible_image_header(n, |x, y| {
                            Some(read_pixel(image, w, h, &t, x as f32 + 0.5, y as f32 + 0.5))
                        }) {
                            continue;
                        }
                        let matrix: Vec<_> = (0..n * n)
                            .map(|i| {
                                read_pixel(
                                    image,
                                    w,
                                    h,
                                    &t,
                                    (i % n) as f32 + 0.5,
                                    (i / n) as f32 + 0.5,
                                )
                            })
                            .collect();
                        for mirror in [false, true] {
                            let grid = if mirror {
                                (0..n * n).map(|i| matrix[(i % n) * n + i / n]).collect()
                            } else {
                                matrix.clone()
                            };
                            if let Some(read) = qr::decode_matrix(&grid, n) {
                                used.extend([first.clone(), second.clone()]);
                                results.push(Detection {
                                    bytes: Some(read.bytes),
                                    structured_append: read.structured_append,
                                    reader_initialization: false,
                                    addon: None,
                                    format: "QRCode".into(),
                                    text: read.text,
                                    polygon: [[0., 0.], [nf, 0.], [nf, nf], [0., nf]]
                                        .map(|[x, y]| map(&t, x, y)),
                                    support: first.support.min(second.support),
                                    error: read.corrected as f32,
                                    gs1: read.gs1,
                                });
                                continue 'pair;
                            }
                            if let Some(score) = qr::localization_score(&grid, n) {
                                regions.add(
                                    "QRCode",
                                    [[0., 0.], [nf, 0.], [nf, nf], [0., nf]]
                                        .map(|[x, y]| map(&t, x, y)),
                                    score,
                                    first.support.min(second.support),
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    limited
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_finder_corner_fit_recovers_a_projective_map() {
        let n = 97;
        let expected = [3.4, 0.7, 35., -0.2, 3., 60., 0.001, -0.0007];
        let points = [
            [2., 2.],
            [5., 2.],
            [5., 5.],
            [2., 5.],
            [92., 2.],
            [95., 2.],
            [95., 5.],
            [92., 5.],
        ];
        let pairs: Vec<_> = points
            .into_iter()
            .map(|p| (p, map(&expected, p[0], p[1])))
            .collect();
        let recovered = fit(&pairs, n, 800.).unwrap();
        for p in [[0., 0.], [97., 0.], [97., 97.], [0., 97.], [48.5, 48.5]] {
            let a = map(&expected, p[0], p[1]);
            let b = map(&recovered, p[0], p[1]);
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 0.1);
        }
    }
}

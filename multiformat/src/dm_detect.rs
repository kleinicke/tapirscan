//! Component hull proposals with ECC200 border scoring and projective sampling.
use crate::{datamatrix, qr_detect::map, Detection};
type Point = [f32; 2];
fn cross(a: Point, b: Point, c: Point) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
pub(crate) fn hull(mut points: Vec<Point>) -> Vec<Point> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 4 {
        return points;
    }
    let mut result = Vec::new();
    for &p in &points {
        while result.len() >= 2
            && cross(result[result.len() - 2], result[result.len() - 1], p) <= 0.
        {
            result.pop();
        }
        result.push(p);
    }
    let len = result.len();
    for &p in points.iter().rev().skip(1) {
        while result.len() > len
            && cross(result[result.len() - 2], result[result.len() - 1], p) <= 0.
        {
            result.pop();
        }
        result.push(p);
    }
    result.pop();
    result
}
pub(crate) fn quad(mut poly: Vec<Point>) -> Option<[Point; 4]> {
    while poly.len() > 4 {
        let i = (0..poly.len()).min_by(|&a, &b| {
            let area = |i: usize| {
                cross(
                    poly[(i + poly.len() - 1) % poly.len()],
                    poly[i],
                    poly[(i + 1) % poly.len()],
                )
                .abs()
            };
            area(a).total_cmp(&area(b))
        })?;
        poly.remove(i);
    }
    if poly.len() != 4 {
        return None;
    }
    Some([poly[0], poly[1], poly[2], poly[3]])
}
// Intersect supporting hull edges to retain a missing white corner. Simply
// deleting hull vertices gives an inscribed quadrilateral and clips that corner.
pub(crate) fn enclosing_quad(
    poly: &[Point],
    pixel_margin: f32,
    limited: &mut bool,
) -> Option<[Point; 4]> {
    if poly.len() < 4 {
        return None;
    }
    let mut edges: Vec<_> = (0..poly.len()).collect();
    edges.sort_by(|&a, &b| {
        let length = |i: usize| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            (p[0] - q[0]).hypot(p[1] - q[1])
        };
        length(b).total_cmp(&length(a))
    });
    *limited |= edges.len() > 12;
    edges.truncate(12);
    edges.sort_unstable();
    let lines: Vec<_> = edges
        .iter()
        .map(|&i| {
            let p = poly[i];
            let q = poly[(i + 1) % poly.len()];
            let a = p[1] - q[1];
            let b = q[0] - p[0];
            [
                a,
                b,
                a * p[0] + b * p[1] - pixel_margin * (a.abs() + b.abs()),
            ]
        })
        .collect();
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for a in 0..lines.len() {
        for b in a + 1..lines.len() {
            for c in b + 1..lines.len() {
                for d in c + 1..lines.len() {
                    let selected = [lines[a], lines[b], lines[c], lines[d]];
                    let mut quad = [[0.; 2]; 4];
                    let mut valid = true;
                    for i in 0..4 {
                        let p = selected[i];
                        let q = selected[(i + 1) % 4];
                        let denominator = p[0] * q[1] - q[0] * p[1];
                        if denominator.abs() < 1e-5 {
                            valid = false;
                            break;
                        }
                        quad[i] = [
                            (p[2] * q[1] - q[2] * p[1]) / denominator,
                            (p[0] * q[2] - q[0] * p[2]) / denominator,
                        ];
                        if selected
                            .iter()
                            .any(|l| l[0] * quad[i][0] + l[1] * quad[i][1] < l[2] - 0.02)
                        {
                            valid = false;
                            break;
                        }
                    }
                    if !valid {
                        continue;
                    }
                    let area = (0..4)
                        .map(|i| {
                            let p = quad[i];
                            let q = quad[(i + 1) % 4];
                            p[0] * q[1] - p[1] * q[0]
                        })
                        .sum::<f32>()
                        * 0.5;
                    if area > 0. && area < best_area {
                        best_area = area;
                        best = Some(quad);
                    }
                }
            }
        }
    }
    best
}
fn rotated_rectangle(poly: &[Point]) -> Option<[Point; 4]> {
    if poly.len() < 4 {
        return None;
    }
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for i in 0..poly.len() {
        let edge_start = poly[i];
        let edge_end = poly[(i + 1) % poly.len()];
        let dx = edge_end[0] - edge_start[0];
        let dy = edge_end[1] - edge_start[1];
        let length = dx.hypot(dy);
        if length < 1e-5 {
            continue;
        }
        let c = dx / length;
        let s = dy / length;
        let mut a0 = f32::INFINITY;
        let mut a1 = f32::NEG_INFINITY;
        let mut b0 = f32::INFINITY;
        let mut b1 = f32::NEG_INFINITY;
        for p in poly {
            let a = p[0] * c + p[1] * s;
            let b = -p[0] * s + p[1] * c;
            a0 = a0.min(a);
            a1 = a1.max(a);
            b0 = b0.min(b);
            b1 = b1.max(b);
        }
        let area = (a1 - a0) * (b1 - b0);
        if area < best_area {
            best_area = area;
            best = Some(
                [[a0, b0], [a1, b0], [a1, b1], [a0, b1]]
                    .map(|[a, b]| [a * c - b * s, a * s + b * c]),
            );
        }
    }
    best
}
fn candidates(image: &[bool], w: usize, h: usize) -> (Vec<[Point; 4]>, bool) {
    let mut seen = vec![false; image.len()];
    let mut components = Vec::new();
    let mut row_min = vec![w; h];
    let mut row_max = vec![0; h];
    for origin in 0..image.len() {
        if !image[origin] || seen[origin] {
            continue;
        }
        let mut stack = vec![origin];
        let mut count = 0;
        let mut touched_rows = Vec::new();
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
        // Scanline flood fill visits the same eight-connected components while
        // marking contiguous spans instead of testing eight neighbors per pixel.
        while let Some(i) = stack.pop() {
            if seen[i] {
                continue;
            }
            let x = i % w;
            let y = i / w;
            let row = y * w;
            let mut left = x;
            let mut right = x + 1;
            while left > 0 && image[row + left - 1] && !seen[row + left - 1] {
                left -= 1;
            }
            while right < w && image[row + right] && !seen[row + right] {
                right += 1;
            }
            seen[row + left..row + right].fill(true);
            count += right - left;
            x0 = x0.min(left);
            x1 = x1.max(right - 1);
            y0 = y0.min(y);
            y1 = y1.max(y);
            if row_min[y] == w {
                touched_rows.push(y);
            }
            row_min[y] = row_min[y].min(left);
            row_max[y] = row_max[y].max(right - 1);
            for adjacent in [y.checked_sub(1), (y + 1 < h).then_some(y + 1)]
                .into_iter()
                .flatten()
            {
                let neighbor = adjacent * w;
                let mut at = left.saturating_sub(1);
                let end = (right + 1).min(w);
                while at < end {
                    if image[neighbor + at] && !seen[neighbor + at] {
                        stack.push(neighbor + at);
                        at += 1;
                        while at < end && image[neighbor + at] && !seen[neighbor + at] {
                            at += 1;
                        }
                    } else {
                        at += 1;
                    }
                }
            }
        }
        let width = x1 - x0 + 1;
        let height = y1 - y0 + 1;
        let eligible = count >= 30
            && width >= 9
            && height >= 7
            && count as f32 / (width * height) as f32 <= 0.92;
        // Every other pixel on a row lies between these extremes, so retaining
        // only its endpoints preserves the exact convex hull.
        let mut boundary = Vec::new();
        for y in touched_rows {
            if eligible {
                boundary.push([row_min[y] as f32 + 0.5, y as f32 + 0.5]);
                boundary.push([row_max[y] as f32 + 0.5, y as f32 + 0.5]);
            }
            row_min[y] = w;
            row_max[y] = 0;
        }
        if !eligible {
            continue;
        }
        let polygon = hull(boundary);
        let mut area = 0_f32;
        let mut perimeter = 0_f32;
        for i in 0..polygon.len() {
            let p = polygon[i];
            let q = polygon[(i + 1) % polygon.len()];
            area += p[0] * q[1] - p[1] * q[0];
            perimeter += (p[0] - q[0]).hypot(p[1] - q[1]);
        }
        // A filled convex stroke has no alternating clock track or data cells.
        // This rejects oblique 1D bars whose axis-aligned boxes look square.
        // The half-pixel perimeter allowance accounts for pixel-center hulls.
        if count as f32 > area.abs() * 0.5 * 0.92 + perimeter * 0.5 + 1. {
            continue;
        }
        components.push((count, polygon, x0, x1, y0, y1));
    }
    // The final proposal order is descending component size. Construct costly
    // enclosing quadrilaterals only until that same ordered budget is filled.
    components.sort_by_key(|p| std::cmp::Reverse(p.0));
    let mut proposals = Vec::new();
    let mut base_count = 0;
    let mut limited = false;
    for (count, polygon, x0, x1, y0, y1) in components {
        let before = proposals.len();
        if let Some(q) = rotated_rectangle(&polygon) {
            proposals.push((count, q));
        }
        if let Some(q) = enclosing_quad(&polygon, 0., &mut limited) {
            proposals.push((count, q));
        }
        let inscribed = quad(polygon);
        if let Some(q) = inscribed {
            proposals.push((count, q));
        }
        proposals.push((
            count,
            [
                [x0 as f32, y0 as f32],
                [x1 as f32 + 1., y0 as f32],
                [x1 as f32 + 1., y1 as f32 + 1.],
                [x0 as f32, y1 as f32 + 1.],
            ],
        ));
        base_count += proposals.len() - before;
        // A clock-track corner may be entirely white/disconnected. Three
        // observed corners still define an affine completion of that corner.
        if let Some(q) = inscribed {
            for i in 0..4 {
                let mut completed = q;
                completed[i] = [
                    q[(i + 1) % 4][0] + q[(i + 3) % 4][0] - q[(i + 2) % 4][0],
                    q[(i + 1) % 4][1] + q[(i + 3) % 4][1] - q[(i + 2) % 4][1],
                ];
                if (completed[i][0] - q[i][0]).hypot(completed[i][1] - q[i][1]) < 1. {
                    continue;
                }
                proposals.push((count, completed));
            }
        }
        if base_count > 100 {
            break;
        }
    }
    limited |= base_count > 100 || proposals.len() > 200;
    proposals.truncate(200);
    (proposals.into_iter().map(|p| p.1).collect(), limited)
}
fn pixel(image: &[bool], w: usize, h: usize, transform: &[f32; 8], x: f32, y: f32) -> Option<bool> {
    let mapped = map(transform, x, y);
    let xx = mapped[0].floor() as isize;
    let yy = mapped[1].floor() as isize;
    if xx < 0 || yy < 0 || xx >= w as isize || yy >= h as isize {
        return None;
    }
    Some(image[yy as usize * w + xx as usize])
}
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_candidates(image: &[bool], w: usize, h: usize) -> serde_json::Value {
    let (proposals, limited) = candidates(image, w, h);
    serde_json::json!({"limited":limited,"proposals":proposals.into_iter().map(|q|
        serde_json::json!({"quad":q,"lBorder":possible_l_border(image,w,h,q)})
    ).collect::<Vec<_>>()})
}
// Before enumerating symbol sizes, require two adjacent mostly solid sides.
// Jittered samples avoid phase-locking to a particular clock-track frequency.
#[cfg(not(target_arch = "wasm32"))]
fn possible_l_border(image: &[bool], w: usize, h: usize, q: [Point; 4]) -> bool {
    let solid = l_border_edges(image, w, h, q);
    (0..4).any(|i| solid[i] >= 18 && solid[(i + 1) % 4] >= 18)
}
fn l_border_edges(image: &[bool], w: usize, h: usize, quad: [Point; 4]) -> [usize; 4] {
    let center = quad
        .iter()
        .fold([0., 0.], |s, p| [s[0] + p[0] * 0.25, s[1] + p[1] * 0.25]);
    let mut solid = [0; 4];
    for edge in 0..4 {
        let edge_start = quad[edge];
        let edge_end = quad[(edge + 1) % 4];
        let inward = [
            center[0] - (edge_start[0] + edge_end[0]) * 0.5,
            center[1] - (edge_start[1] + edge_end[1]) * 0.5,
        ];
        let length = inward[0].hypot(inward[1]);
        if length < 1. {
            return [0; 4];
        }
        for offset in [0.25, 0.75, 1.5, length / 32., length / 12.] {
            let mut black = 0;
            for i in 0..24 {
                let fraction = (i as f32 + 0.2 + (i as f32 * 0.381_966).fract() * 0.6) / 24.;
                let x = (edge_start[0]
                    + fraction * (edge_end[0] - edge_start[0])
                    + inward[0] * offset / length)
                    .floor() as isize;
                let y = (edge_start[1]
                    + fraction * (edge_end[1] - edge_start[1])
                    + inward[1] * offset / length)
                    .floor() as isize;
                if x >= 0
                    && y >= 0
                    && x < w as isize
                    && y < h as isize
                    && image[y as usize * w + x as usize]
                {
                    black += 1;
                }
            }
            solid[edge] = solid[edge].max(black);
            if black >= 18 {
                break;
            }
        }
    }
    solid
}
// Closed-form rectangle-to-quadrilateral mapping. Every size hypothesis has
// this rectangular source, so a general eight-equation elimination is wasteful.
fn rectangle_transform(cols: usize, rows: usize, q: [Point; 4]) -> Option<[f32; 8]> {
    let dx1 = q[1][0] - q[2][0];
    let dx2 = q[3][0] - q[2][0];
    let dy1 = q[1][1] - q[2][1];
    let dy2 = q[3][1] - q[2][1];
    let sx = q[0][0] - q[1][0] + q[2][0] - q[3][0];
    let sy = q[0][1] - q[1][1] + q[2][1] - q[3][1];
    let denominator = dx1 * dy2 - dx2 * dy1;
    if denominator.abs() < 1e-7 || cols == 0 || rows == 0 {
        return None;
    }
    let g = (sx * dy2 - dx2 * sy) / denominator;
    let h = (dx1 * sy - sx * dy1) / denominator;
    Some([
        (q[1][0] - q[0][0] + g * q[1][0]) / cols as f32,
        (q[3][0] - q[0][0] + h * q[3][0]) / rows as f32,
        q[0][0],
        (q[1][1] - q[0][1] + g * q[1][1]) / cols as f32,
        (q[3][1] - q[0][1] + h * q[3][1]) / rows as f32,
        q[0][1],
        g / cols as f32,
        h / rows as f32,
    ])
}
fn border(
    image: &[bool],
    w: usize,
    h: usize,
    transform: &[f32; 8],
    cols: usize,
    rows: usize,
) -> Option<f32> {
    let mut errors = 0;
    let error_limit = (cols + rows) * 2 / 5;
    // The two clock tracks reject solid rectangles sooner. This changes only
    // sampling order: every admitted hypothesis has the same border error sum.
    for (edge, length) in [(0, cols), (1, rows), (2, cols), (3, rows)] {
        for at in 0..length {
            let (x, y, expected) = match edge {
                0 => (at, 0, at % 2 == 0),
                1 => (cols - 1, at, at % 2 == 1),
                2 => (at, rows - 1, true),
                _ => (0, at, true),
            };
            errors += usize::from(
                pixel(image, w, h, transform, x as f32 + 0.5, y as f32 + 0.5)? != expected,
            );
            if errors > error_limit {
                return None;
            }
        }
    }
    Some(errors as f32 / ((cols + rows) * 2) as f32)
}
pub fn detect(
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    binary_images: &mut crate::binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    for mode in 0..4 {
        if binary_images.is_duplicate(mode) {
            continue;
        }
        let image = binary_images.get(mode);
        let (proposals, limited) = candidates(image, w, h);
        regions.limited |= limited;
        for original in proposals {
            let solid = l_border_edges(image, w, h, original);
            if !(0..4).any(|i| solid[i] >= 18 && solid[(i + 1) % 4] >= 18) {
                continue;
            }
            let center = original
                .iter()
                .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
            if results.iter().any(|r| {
                let c = r
                    .polygon
                    .iter()
                    .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
                (center[0] - c[0]).hypot(center[1] - c[1]) < 8.
            }) {
                continue;
            }
            let mut hypotheses = Vec::new();
            for mirror in [false, true] {
                for rotation in 0..4 {
                    // A valid orientation puts the solid L on the bottom and
                    // left. Use a looser threshold than proposal admission to
                    // retain damaged edges and uncertain corner completion.
                    let left_edge = (rotation + if mirror { 0 } else { 3 }) % 4;
                    let bottom_edge = (rotation + if mirror { 1 } else { 2 }) % 4;
                    if solid[left_edge] < 15 || solid[bottom_edge] < 15 {
                        continue;
                    }
                    let q: [Point; 4] = std::array::from_fn(|i| {
                        original[(rotation + if mirror { 4 - i } else { i }) % 4]
                    });
                    let dx = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
                    let dy = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
                    for size in datamatrix::SIZES {
                        let ratio = (dx / dy) / (size.w as f32 / size.h as f32);
                        if !(0.65..=1.55).contains(&ratio)
                            || dx / (size.w as f32) < 0.8
                            || dy / (size.h as f32) < 0.8
                        {
                            continue;
                        }
                        for margin in [0., 0.25, 0.5, 1.] {
                            let scale_x = 1. + margin * 2. / size.w as f32;
                            let scale_y = 1. + margin * 2. / size.h as f32;
                            let ex = [
                                (q[1][0] - q[0][0] + q[2][0] - q[3][0]) * 0.25,
                                (q[1][1] - q[0][1] + q[2][1] - q[3][1]) * 0.25,
                            ];
                            let ey = [
                                (q[3][0] - q[0][0] + q[2][0] - q[1][0]) * 0.25,
                                (q[3][1] - q[0][1] + q[2][1] - q[1][1]) * 0.25,
                            ];
                            let expanded: [Point; 4] = std::array::from_fn(|i| {
                                let sx = if i == 0 || i == 3 { -1. } else { 1. };
                                let sy = if i < 2 { -1. } else { 1. };
                                [
                                    q[i][0]
                                        + sx * (scale_x - 1.) * ex[0]
                                        + sy * (scale_y - 1.) * ey[0],
                                    q[i][1]
                                        + sx * (scale_x - 1.) * ex[1]
                                        + sy * (scale_y - 1.) * ey[1],
                                ]
                            });
                            let Some(t) = rectangle_transform(size.w, size.h, expanded) else {
                                continue;
                            };
                            if let Some(score) = border(image, w, h, &t, size.w, size.h) {
                                if score <= 0.2 {
                                    hypotheses.push((score, *size, t, expanded));
                                }
                            }
                        }
                    }
                }
            }
            hypotheses.sort_by(|a, b| a.0.total_cmp(&b.0));
            regions.limited |= hypotheses.len() > 12;
            for (score, size, t, polygon) in hypotheses.into_iter().take(12) {
                attempts += 1;
                if attempts > 800 {
                    return (results, true);
                }
                let mut matrix = Vec::with_capacity(size.w * size.h);
                let mut valid = true;
                for y in 0..size.h {
                    for x in 0..size.w {
                        if let Some(v) = pixel(image, w, h, &t, x as f32 + 0.5, y as f32 + 0.5) {
                            matrix.push(v);
                        } else {
                            valid = false;
                        }
                    }
                }
                if valid {
                    if score <= 0.08 {
                        regions.add("DataMatrix", polygon, 1. - score, 1);
                    }
                    if let Some(read) = datamatrix::decode_matrix(&matrix, size.w, size.h) {
                        results.push(Detection {
                            bytes: Some(read.bytes),
                            structured_append: read.structured_append,
                            reader_initialization: read.reader_initialization,
                            addon: None,
                            format: "DataMatrix".into(),
                            text: read.text,
                            polygon,
                            support: 1,
                            error: score,
                            gs1: read.gs1,
                        });
                        break;
                    }
                }
            }
        }
    }
    (results, false)
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    #[test]
    fn rectangle_mapping_matches_general_projective_solution() {
        for q in [
            [[10., 20.], [170., 20.], [170., 90.], [10., 90.]],
            [[20., 12.], [154., 43.], [133., 121.], [6., 97.]],
            [[133., 121.], [154., 43.], [20., 12.], [6., 97.]],
        ] {
            let source = [[0., 0.], [32., 0.], [32., 12.], [0., 12.]];
            let general = crate::qr_detect::homography(source, q).unwrap();
            let fast = rectangle_transform(32, 12, q).unwrap();
            for y in 0..=12 {
                for x in 0..=32 {
                    let a = map(&general, x as f32, y as f32);
                    let b = map(&fast, x as f32, y as f32);
                    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 0.001);
                }
            }
        }
    }
    #[test]
    fn enclosing_edge_budget_reports_only_truncation() {
        for count in [12, 13] {
            let points: Vec<_> = (0..count)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / count as f32;
                    [20. * angle.cos(), 20. * angle.sin()]
                })
                .collect();
            let mut limited = false;
            let _ = enclosing_quad(&points, 0., &mut limited);
            assert_eq!(limited, count > 12);
        }
    }
    #[test]
    fn reconstructs_an_unprinted_corner_from_supporting_edges() {
        let q = enclosing_quad(
            &[[0., 0.], [8., 0.], [10., 2.], [10., 10.], [0., 10.]],
            0.,
            &mut false,
        )
        .unwrap();
        for expected in [[0., 0.], [10., 0.], [10., 10.], [0., 10.]] {
            assert!(q
                .iter()
                .any(|p| (p[0] - expected[0]).abs() < 1e-5 && (p[1] - expected[1]).abs() < 1e-5));
        }
    }
}

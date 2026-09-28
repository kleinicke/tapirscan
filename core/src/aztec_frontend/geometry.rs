type Point = [f32; 2];
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
    // The same ordered edge pair appears in many four-edge combinations.
    // Cache its exact intersection and half-plane violations once. Keep both
    // directions: reversing the floating-point arithmetic need not be bitwise identical.
    let mut intersections = [[None; 12]; 12];
    let mut violations = [[0_u16; 12]; 12];
    for (i, p) in lines.iter().enumerate() {
        for (j, q) in lines.iter().enumerate() {
            if i == j {
                continue;
            }
            let denominator = p[0] * q[1] - q[0] * p[1];
            if denominator.abs() < 1e-5 {
                continue;
            }
            let point = [
                (p[2] * q[1] - q[2] * p[1]) / denominator,
                (p[0] * q[2] - q[0] * p[2]) / denominator,
            ];
            intersections[i][j] = Some(point);
            for (k, l) in lines.iter().enumerate() {
                if l[0] * point[0] + l[1] * point[1] < l[2] - 0.02 {
                    violations[i][j] |= 1 << k;
                }
            }
        }
    }
    let mut best = None;
    let mut best_area = f32::INFINITY;
    for a in 0..lines.len() {
        for b in a + 1..lines.len() {
            for c in b + 1..lines.len() {
                for d in c + 1..lines.len() {
                    let selected = [a, b, c, d];
                    let selected_mask = (1 << a) | (1 << b) | (1 << c) | (1 << d);
                    let mut quad = [[0.; 2]; 4];
                    let mut valid = true;
                    for i in 0..4 {
                        let p = selected[i];
                        let q = selected[(i + 1) % 4];
                        let Some(point) = intersections[p][q] else {
                            valid = false;
                            break;
                        };
                        if violations[p][q] & selected_mask != 0 {
                            valid = false;
                            break;
                        }
                        quad[i] = point;
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

pub(crate) fn quads(
    bits: &[bool],
    width: usize,
    seed: [usize; 2],
    bounds: [usize; 4],
    count_range: [f32; 2],
    limited: &mut bool,
) -> Option<Vec<[Point; 4]>> {
    let hull =
        crate::qr_frontend::component_geometry::outline(bits, width, seed, bounds, count_range)?;
    Some(
        crate::qr_frontend::dm_detect::quad(hull.clone())
            .into_iter()
            .chain(enclosing_quad(&hull, 0., limited))
            .collect(),
    )
}

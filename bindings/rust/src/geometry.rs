pub(crate) fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// `hypot` uses only magnitudes, so argument order does not change the result.
pub(crate) fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn signed_area(points: &[[f64; 2]]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = points[(i + 1) % points.len()];
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        / 2.0
}

/// Whether two reads of one payload are adjacent strips of a single symbol: parallel scan axes
/// (longest edges within 5 degrees), sharing at least half the shorter read along that axis and
/// less than 5% of the longer read's length apart across the bars. Separately printed equal
/// symbols sit further apart than that.
pub(crate) fn adjacent_strips(a: &crate::Quad, b: &crate::Quad) -> bool {
    let axis = |q: &crate::Quad| {
        (0..4)
            .map(|k| [q[(k + 1) % 4][0] - q[k][0], q[(k + 1) % 4][1] - q[k][1]])
            .max_by(|x, y| x[0].hypot(x[1]).total_cmp(&y[0].hypot(y[1])))
            .unwrap_or([0., 0.])
    };
    let (ea, eb) = (axis(a), axis(b));
    let (la, lb) = (ea[0].hypot(ea[1]), eb[0].hypot(eb[1]));
    if la < 1e-6
        || lb < 1e-6
        || (ea[0] * eb[1] - ea[1] * eb[0]).abs() / (la * lb) > 5_f64.to_radians().sin()
    {
        return false;
    }
    let (u, n) = ([ea[0] / la, ea[1] / la], [-ea[1] / la, ea[0] / la]);
    let span = |q: &crate::Quad, d: [f64; 2]| {
        q.iter()
            .map(|p| p[0] * d[0] + p[1] * d[1])
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            })
    };
    let ((a0, a1), (b0, b1)) = (span(a, u), span(b, u));
    let shared = a1.min(b1) - a0.max(b0);
    let ((c0, c1), (d0, d1)) = (span(a, n), span(b, n));
    let gap = (d0 - c1).max(c0 - d1);
    shared >= 0.5 * (a1 - a0).min(b1 - b0) && gap < 0.05 * la.max(lb)
}

/// Same convex clipping and 0.65 reconciliation basis as the research host.
pub(crate) fn overlap_quads(first_quad: &crate::Quad, second_quad: &crate::Quad) -> (f64, f64) {
    let first_area = signed_area(first_quad).abs();
    let second_area = signed_area(second_quad).abs();
    if first_area.min(second_area) < 1e-6 {
        return (0.0, 0.0);
    }
    let winding = signed_area(second_quad).signum();
    let mut points = first_quad.to_vec();
    for edge_index in 0..4 {
        if points.is_empty() {
            break;
        }
        let edge_start = second_quad[edge_index];
        let edge_end = second_quad[(edge_index + 1) % 4];
        let side = |point: [f64; 2]| {
            winding
                * ((edge_end[0] - edge_start[0]) * (point[1] - edge_start[1])
                    - (edge_end[1] - edge_start[1]) * (point[0] - edge_start[0]))
        };
        let mut clipped = Vec::new();
        for point_index in 0..points.len() {
            let current = points[point_index];
            let next = points[(point_index + 1) % points.len()];
            let current_side = side(current);
            let next_side = side(next);
            if current_side >= 0.0 {
                clipped.push(current);
            }
            if (current_side >= 0.0) != (next_side >= 0.0) {
                let fraction = current_side / (current_side - next_side);
                clipped.push([
                    current[0] + fraction * (next[0] - current[0]),
                    current[1] + fraction * (next[1] - current[1]),
                ]);
            }
        }
        points = clipped;
    }
    let intersection = first_area.min(second_area).min(signed_area(&points).abs());
    (
        intersection / first_area.min(second_area),
        intersection / first_area,
    )
}

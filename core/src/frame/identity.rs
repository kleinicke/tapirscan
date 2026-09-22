//! Physical-symbol overlap and conservative pending-coverage geometry.
use super::Quad;

pub(super) fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

pub(super) fn signed_area(p: &[[f64; 2]]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .take(p.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        * 0.5
}

pub(super) fn intersection(a: Quad, b: Quad) -> f64 {
    let sign = signed_area(&b).signum();
    let mut polygon = a.to_vec();
    for i in 0..4 {
        let (a, b) = (b[i], b[(i + 1) % 4]);
        let mut out = Vec::with_capacity(8);
        if polygon.is_empty() {
            return 0.;
        }
        for j in 0..polygon.len() {
            let (x, y) = (polygon[j], polygon[(j + 1) % polygon.len()]);
            let (dx, dy) = (sign * cross(a, b, x), sign * cross(a, b, y));
            if dx >= 0. {
                out.push(x);
            }
            if (dx >= 0.) != (dy >= 0.) {
                let fraction = dx / (dx - dy);
                out.push([
                    x[0] + fraction * (y[0] - x[0]),
                    x[1] + fraction * (y[1] - x[1]),
                ]);
            }
        }
        polygon = out;
    }
    signed_area(&polygon).abs()
}

pub(super) fn crossing_read_paths(a: Quad, b: Quad) -> bool {
    let ends = |quad: Quad| {
        [
            [
                0.5 * (quad[0][0] + quad[3][0]),
                0.5 * (quad[0][1] + quad[3][1]),
            ],
            [
                0.5 * (quad[1][0] + quad[2][0]),
                0.5 * (quad[1][1] + quad[2][1]),
            ],
        ]
    };
    let a = ends(a);
    let mut b = ends(b);
    let u = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
    let mut v = [b[1][0] - b[0][0], b[1][1] - b[0][1]];
    if u[0] * v[0] + u[1] * v[1] < 0. {
        b.reverse();
        v = [-v[0], -v[1]];
    }
    let la = u[0].hypot(u[1]);
    let lb = v[0].hypot(v[1]);
    if la < 76.
        || lb < 76.
        || la.min(lb) / la.max(lb) < 0.9
        || (u[0] * v[0] + u[1] * v[1]) / (la * lb) < 20f64.to_radians().cos()
    {
        return false;
    }
    let den = u[0] * v[1] - u[1] * v[0];
    if den.abs() < 1e-9 {
        return false;
    }
    let offset = [b[0][0] - a[0][0], b[0][1] - a[0][1]];
    let first_fraction = (offset[0] * v[1] - offset[1] * v[0]) / den;
    let second_fraction = (offset[0] * u[1] - offset[1] * u[0]) / den;
    // Endpoint-crossing fits are valid only after the mandatory source-pixel
    // identity check in reconcile_image; geometry alone never merges a read.
    (-0.05..=1.05).contains(&first_fraction)
        && (-0.05..=1.05).contains(&second_fraction)
        && (first_fraction - second_fraction).abs() <= 2. / 95.
}

pub(crate) fn same_space(a: Quad, b: Quad) -> bool {
    let (aa, bb) = (signed_area(&a).abs(), signed_area(&b).abs());
    if !aa.is_finite() || !bb.is_finite() || aa <= 0. || bb <= 0. {
        return false;
    }
    let x = intersection(a, b).min(aa.min(bb));
    x / (aa + bb - x) >= 0.5 || x / aa.min(bb) >= 0.85
}

pub(super) fn envelope(q: Quad) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in q {
        if !p[0].is_finite() || !p[1].is_finite() {
            return [
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::INFINITY,
            ];
        }
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}

pub(super) fn envelopes_overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

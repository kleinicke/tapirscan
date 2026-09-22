//! Spatial evidence association. Convex polygon overlap and explicit scanner lines.
#![forbid(unsafe_code)]
use crate::sampling::Error;
type Point = [f64; 2];
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn area(p: &[Point]) -> f64 {
    (0..p.len())
        .map(|i| p[i][0] * p[(i + 1) % p.len()][1] - p[i][1] * p[(i + 1) % p.len()][0])
        .sum::<f64>()
        .abs()
        / 2.
}
fn hull(q: &[Point]) -> Result<Vec<Point>, Error> {
    if !(2..=64).contains(&q.len()) {
        return Err(Error::Geometry);
    }
    if q.iter().flatten().any(|x| !x.is_finite() || x.abs() > 1e9) {
        return Err(Error::Geometry);
    }
    let mut p = q.to_vec();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 2 {
        return Err(Error::Geometry);
    }
    let mut out = Vec::with_capacity(2 * q.len());
    for &v in &p {
        while out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], v) <= 0. {
            out.pop();
        }
        out.push(v);
    }
    let n = out.len();
    for &v in p[..p.len() - 1].iter().rev() {
        while out.len() > n && cross(out[out.len() - 2], out[out.len() - 1], v) <= 0. {
            out.pop();
        }
        out.push(v);
    }
    out.pop();
    Ok(out)
}
fn clipped_polygon(a: &[Point], b: &[Point]) -> Vec<Point> {
    let mut out = a.to_vec();
    for i in 0..b.len() {
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        let (v, width) = (b[i], b[(i + 1) % b.len()]);
        let mut prev = *input.last().unwrap();
        let mut dp = cross(v, width, prev);
        for curr in input {
            let dc = cross(v, width, curr);
            if (dc >= 0.) != (dp >= 0.) {
                let t = dp / (dp - dc);
                out.push([
                    prev[0] + t * (curr[0] - prev[0]),
                    prev[1] + t * (curr[1] - prev[1]),
                ]);
            }
            if dc >= 0. {
                out.push(curr);
            }
            prev = curr;
            dp = dc;
        }
    }
    out
}
fn line_polygon(a: Point, b: Point, polygon: &[Point]) -> f64 {
    let (mut lo, mut hi) = (0_f64, 1_f64);
    for i in 0..polygon.len() {
        let (v, edge_end) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        let f = cross(v, edge_end, a);
        let delta = cross(v, edge_end, b) - f;
        if delta.abs() < 1e-10 {
            if f < 0. {
                return 0.;
            }
        } else if delta > 0. {
            lo = lo.max(-f / delta);
        } else {
            hi = hi.min(-f / delta);
        }
    }
    (hi - lo).clamp(0., 1.)
}
fn line_line(a: &[Point], b: &[Point]) -> f64 {
    let dx = a[1][0] - a[0][0];
    let dy = a[1][1] - a[0][1];
    let len = dx.hypot(dy);
    let bx = b[1][0] - b[0][0];
    let by = b[1][1] - b[0][1];
    let blen = bx.hypot(by);
    if len < 1e-9 || blen < 1e-9 || (dx * bx + dy * by).abs() / (len * blen) < 0.965_925_826 {
        return 0.;
    }
    // Two explicit line observations need coincident scan paths (2 source pixels).
    // A line inside an area observation is handled by clipping, not artificial boxes.
    if b.iter().any(|p| cross(a[0], a[1], *p).abs() / len > 2.) {
        return 0.;
    }
    let project = |p: Point| ((p[0] - a[0][0]) * dx + (p[1] - a[0][1]) * dy) / len;
    let (u, v) = (project(b[0]), project(b[1]));
    (len.min(u.max(v)) - 0_f64.max(u.min(v))).max(0.) / len.min(blen)
}
/// Area intersection / smaller area; line-in-polygon coverage; or collinear
/// line overlap. Scores are symmetric and bounded, not calibrated probabilities.
/// A zero-dimensional point is rejected. Callers retain all original geometry.
/// # Errors
/// Returns `Geometry` for non-finite, degenerate, non-convex or oversized polygons.
pub fn overlap(a: &[Point], b: &[Point]) -> Result<f64, Error> {
    let (a, b) = (hull(a)?, hull(b)?);
    let score = match (a.len(), b.len()) {
        (2, 2) => line_line(&a, &b).min(line_line(&b, &a)),
        (2, _) => line_polygon(a[0], a[1], &b),
        (_, 2) => line_polygon(b[0], b[1], &a),
        _ => area(&clipped_polygon(&a, &b)) / area(&a).min(area(&b)),
    };
    if !score.is_finite() {
        return Err(Error::Geometry);
    }
    Ok(score.clamp(0., 1.))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f64, y: f64) -> [Point; 4] {
        [[x, y], [x + 100., y], [x + 100., y + 40.], [x, y + 40.]]
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn area_overlap_and_disjoint_diagonals() {
        assert_eq!(overlap(&rect(0., 0.), &rect(50., 0.)).unwrap(), 0.5);
        let a = [[0., 0.], [100., 100.], [95., 105.], [-5., 5.]];
        let b = a.map(|p| [p[0], p[1] + 30.]);
        assert_eq!(overlap(&a, &b).unwrap(), 0.);
        assert_eq!(overlap(&a, &a).unwrap(), 1.);
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn explicit_lines() {
        let l = [[1., 20.], [99., 20.], [99., 20.], [1., 20.]];
        assert_eq!(overlap(&l, &rect(0., 0.)).unwrap(), 1.);
        assert_eq!(overlap(&l, &rect(0., 30.)).unwrap(), 0.);
        assert_eq!(overlap(&l, &l).unwrap(), 1.);
        assert_eq!(overlap(&l, &l.map(|p| [p[0], p[1] + 3.])).unwrap(), 0.);
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn variable_polygon_sizes_are_bounded() {
        let a = [
            [0., 0.],
            [50., 0.],
            [100., 0.],
            [100., 20.],
            [100., 40.],
            [50., 40.],
            [0., 40.],
            [0., 20.],
        ];
        assert_eq!(overlap(&a, &rect(0., 0.)).unwrap(), 1.);
        assert_eq!(overlap(&a, &a).unwrap(), 1.);
        assert_eq!(overlap(&[[0., 0.]; 65], &a), Err(Error::Geometry));
        assert_eq!(overlap(&[[0., 0.]], &a), Err(Error::Geometry));
    }
    #[test]
    fn transformations_and_validation() {
        let a = rect(0., 0.);
        let b = rect(50., 0.);
        let rotate = |p: Point| {
            [
                p[0] * 0.5 - p[1] * 0.866_025_4 + 13.,
                p[0] * 0.866_025_4 + p[1] * 0.5 - 7.,
            ]
        };
        let score = overlap(&a.map(rotate), &b.map(rotate)).unwrap();
        assert!((score - 0.5).abs() < 1e-8);
        assert_eq!(overlap(&[[0.; 2]; 4], &a), Err(Error::Geometry));
        assert_eq!(overlap(&[[f64::NAN; 2]; 4], &a), Err(Error::Geometry));
    }
}

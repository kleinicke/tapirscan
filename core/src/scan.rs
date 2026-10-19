//! Candidate quadrilaterals and their projective transform from the unit square.
#![forbid(unsafe_code)]
use crate::sampling::{Error, Transform};
pub type Quad = [[f64; 2]; 4];
/// Unit square to a convex cyclic quad. Both winding directions are accepted.
/// # Errors
/// Returns `Geometry` for non-finite, degenerate or invalid quadrilaterals.
pub fn transform(q: Quad) -> Result<Transform, Error> {
    if q.iter().flatten().any(|v| !v.is_finite()) {
        return Err(Error::Geometry);
    }
    let mut sign = 0.;
    for i in 0..4 {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if !cross.is_finite() || cross.abs() < 1e-9 || (sign != 0. && cross * sign < 0.) {
            return Err(Error::Geometry);
        }
        sign = cross;
    }
    let dx1 = q[1][0] - q[2][0];
    let dx2 = q[3][0] - q[2][0];
    let dy1 = q[1][1] - q[2][1];
    let dy2 = q[3][1] - q[2][1];
    let dx3 = q[0][0] - q[1][0] + q[2][0] - q[3][0];
    let dy3 = q[0][1] - q[1][1] + q[2][1] - q[3][1];
    let (g, h) = if dx3 == 0. && dy3 == 0. {
        (0., 0.)
    } else {
        let den = dx1 * dy2 - dx2 * dy1;
        if den.abs() < 1e-12 {
            return Err(Error::Geometry);
        }
        ((dx3 * dy2 - dx2 * dy3) / den, (dx1 * dy3 - dx3 * dy1) / den)
    };
    Transform::new([
        q[1][0] - q[0][0] + g * q[1][0],
        q[3][0] - q[0][0] + h * q[3][0],
        q[0][0],
        q[1][1] - q[0][1] + g * q[1][1],
        q[3][1] - q[0][1] + h * q[3][1],
        q[0][1],
        g,
        h,
        1.,
    ])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projective_corners_and_nonconvex_validation() {
        let q = [[20., 30.], [220., 30.], [180., 130.], [60., 130.]];
        let m = transform(q).unwrap().0;
        for (p, [x, y]) in q.into_iter().zip([[0., 0.], [1., 0.], [1., 1.], [0., 1.]]) {
            let z = m[6] * x + m[7] * y + 1.;
            assert!(((m[0] * x + m[1] * y + m[2]) / z - p[0]).abs() < 1e-9);
            assert!(((m[3] * x + m[4] * y + m[5]) / z - p[1]).abs() < 1e-9);
        }
        assert!(transform([q[0], q[2], q[1], q[3]]).is_err());
        assert!(transform([[f64::NAN; 2]; 4]).is_err());
    }
}

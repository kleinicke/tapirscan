//! Long-track priority for a short read at the same module scale and source row.
// Preserve validated arithmetic and observation layout. Coordinates are bounded
// by image/profile limits; digit and pixel casts follow explicit clamps.
#![allow(clippy::many_single_char_names, clippy::wildcard_imports)]
use super::*;
fn inside(q: Quad, p: [f64; 2]) -> bool {
    let mut positive = false;
    let mut negative = false;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        positive |= c > 1e-6;
        negative |= c < -1e-6;
    }
    !(positive && negative)
}
impl Collector {
    pub fn set_primary(&mut self, frame: &crate::frame::Frame) {
        if PROTECT && self.mask & 12 != 0 {
            self.primary.extend(
                frame
                    .barcodes
                    .iter()
                    .filter(|b| b.detection.support >= 3)
                    .map(|b| b.detection.polygon),
            );
        }
    }
    fn protected_segment(&self, a: [f64; 2], b: [f64; 2], modules: f64) -> bool {
        let d = [b[0] - a[0], b[1] - a[1]];
        let width = d[0].hypot(d[1]);
        if width < 8. {
            return false;
        }
        self.primary.iter().any(|q| {
            let u = [
                0.5 * (q[1][0] + q[2][0] - q[0][0] - q[3][0]),
                0.5 * (q[1][1] + q[2][1] - q[0][1] - q[3][1]),
            ];
            let w = u[0].hypot(u[1]);
            let ratio = (width / modules) / (w / 95.);
            (0.8..=1.25).contains(&ratio)
                && (d[0] * u[0] + d[1] * u[1]).abs() >= 0.97 * width * w
                && [0.1, 0.5, 0.9]
                    .iter()
                    .all(|t| inside(*q, [a[0] + t * d[0], a[1] + t * d[1]]))
        })
    }
    pub(super) fn protected_detection(&self, d: &crate::experiment::Detection) -> bool {
        let q = d.polygon;
        self.protected_segment(
            [0.5 * (q[0][0] + q[3][0]), 0.5 * (q[0][1] + q[3][1])],
            [0.5 * (q[1][0] + q[2][0]), 0.5 * (q[1][1] + q[2][1])],
            if d.digits[0] == 14 { 67. } else { 51. },
        )
    }
    pub(super) fn protected_observation(&self, q: Quad, o: &Observation) -> bool {
        if self.primary.is_empty() {
            return false;
        }
        let Ok(m) = crate::scan::transform(q) else {
            return false;
        };
        let point = |t: f64| {
            let (u, v) = if o.axis == 0 {
                (t, o.fraction)
            } else {
                (o.fraction, t)
            };
            let den = m.0[6] * u + m.0[7] * v + m.0[8];
            [
                (m.0[0] * u + m.0[1] * v + m.0[2]) / den,
                (m.0[3] * u + m.0[4] * v + m.0[5]) / den,
            ]
        };
        self.protected_segment(
            point(o.left),
            point(o.right),
            if o.digits[0] == 14 { 67. } else { 51. },
        )
    }
}

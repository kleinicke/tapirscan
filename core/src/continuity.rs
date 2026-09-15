//! Conservative image evidence between two EAN read bands. This does not decode
//! text or prove global identity. Call only for equal independently decoded values.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView};
use crate::scan::Quad;
type Point = [f64; 2];
const N: usize = 285;
const MAX_STEPS: usize = 512;
fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]
}
fn distance(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn line(q: Quad, axis: usize) -> [Point; 2] {
    if axis == 0 {
        [lerp(q[0], q[3], 0.5), lerp(q[1], q[2], 0.5)]
    } else {
        [lerp(q[0], q[1], 0.5), lerp(q[3], q[2], 0.5)]
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Evidence {
    pub supported: bool,
    pub paths: usize,
    pub minimum_correlation: f64,
}
#[derive(Clone)]
pub struct Continuity {
    reference: [f64; N],
    current: [f64; N],
}
impl Default for Continuity {
    fn default() -> Self {
        Self {
            reference: [0.; N],
            current: [0.; N],
        }
    }
}
impl Continuity {
    /// Anchor vertices 0→1 and 3→2 must follow the decoded module axis (the
    /// `scan::Read` convention). Other is a cyclic quad, including a collapsed line.
    /// Checks centre and 20%/80% support corridors at <=1 pixel spacing,
    /// each capped at 512 steps (at most three corridors per pair).
    /// Rejection/budget exhaustion preserves both observations. No image clamping.
    pub fn check(
        &mut self,
        image: ImageView<'_>,
        anchor: Quad,
        other: Quad,
    ) -> Result<Evidence, Error> {
        let mut evidence = self.check_with_contrast(image, anchor, other, 0.75)?;
        if !evidence.supported {
            return Ok(evidence);
        }
        // A centre line alone can hide a second symbol inside a broad box.
        // Require continuity to both interior cross-sections as well. Each
        // corridor remains independently bounded by MAX_STEPS (three total).
        let a = line(anchor, 0);
        let av = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
        let alignment = |axis| {
            let b = line(other, axis);
            let len = distance(b[0], b[1]);
            if len < 1. {
                0.
            } else {
                ((b[1][0] - b[0][0]) * av[0] + (b[1][1] - b[0][1]) * av[1]).abs() / len
            }
        };
        let axis = usize::from(alignment(1) > alignment(0));
        for fraction in [0.2, 0.8] {
            let b = if axis == 0 {
                [
                    lerp(other[0], other[3], fraction),
                    lerp(other[1], other[2], fraction),
                ]
            } else {
                [
                    lerp(other[0], other[1], fraction),
                    lerp(other[3], other[2], fraction),
                ]
            };
            let centre = line(other, axis);
            if distance(b[0], centre[0]).max(distance(b[1], centre[1])) < 1e-9 {
                continue;
            }
            let current =
                self.check_with_contrast(image, anchor, [b[0], b[1], b[1], b[0]], 0.75)?;
            evidence.paths += current.paths;
            evidence.minimum_correlation = evidence
                .minimum_correlation
                .min(current.minimum_correlation);
            if !current.supported {
                evidence.supported = false;
                return Ok(evidence);
            }
        }
        Ok(evidence)
    }
    /// Final read consolidation requires at least 75% of anchor contrast at
    /// every bridge sample. The legacy 50% threshold can pass a one-pixel white
    /// gap when bilinear samples straddle it at half-pixel phase.
    pub fn check_strict(
        &mut self,
        image: ImageView<'_>,
        anchor: Quad,
        other: Quad,
    ) -> Result<Evidence, Error> {
        self.check(image, anchor, other)
    }
    fn check_with_contrast(
        &mut self,
        image: ImageView<'_>,
        anchor: Quad,
        other: Quad,
        contrast_ratio: f64,
    ) -> Result<Evidence, Error> {
        let reject = |paths, minimum_correlation| Evidence {
            supported: false,
            paths,
            minimum_correlation,
        };
        for p in anchor.iter().chain(other.iter()) {
            if p.iter().any(|v| !v.is_finite())
                || p[0] < 0.
                || p[1] < 0.
                || p[0] > (image.width - 1) as f64
                || p[1] > (image.height - 1) as f64
            {
                return Err(Error::Geometry);
            }
        }
        crate::scan::transform(anchor)?;
        // Other may be a zero-height reader line, but never a bow-tie or a point.
        if crate::scan::transform(other).is_err() {
            let mut far = (0., other[0]);
            for p in other {
                let d = distance(other[0], p);
                if d > far.0 {
                    far = (d, p);
                }
            }
            if far.0 < 1.
                || other.iter().any(|p| {
                    ((far.1[0] - other[0][0]) * (p[1] - other[0][1])
                        - (far.1[1] - other[0][1]) * (p[0] - other[0][0]))
                        .abs()
                        > 1e-7 * far.0
                })
            {
                return Err(Error::Geometry);
            }
        }
        let a = line(anchor, 0);
        let len = distance(a[0], a[1]);
        if len < 95. {
            return Ok(reject(0, 0.));
        }
        let mut best = None;
        for axis in 0..2 {
            let mut b = line(other, axis);
            if distance(a[0], b[1]) + distance(a[1], b[0])
                < distance(a[0], b[0]) + distance(a[1], b[1])
            {
                b.swap(0, 1);
            }
            let blen = distance(b[0], b[1]);
            if !(0.9..=1.1).contains(&(blen / len)) {
                continue;
            }
            let cosine = ((a[1][0] - a[0][0]) * (b[1][0] - b[0][0])
                + (a[1][1] - a[0][1]) * (b[1][1] - b[0][1]))
                / (len * blen);
            if cosine < 0.995 {
                continue;
            }
            let displacement = distance(a[0], b[0]).max(distance(a[1], b[1]));
            if best.is_none_or(|(d, _)| displacement < d) {
                best = Some((displacement, b));
            }
        }
        let Some((displacement, b)) = best else {
            return Ok(reject(0, 0.));
        };
        let steps = displacement.ceil().max(1.) as usize;
        if steps > MAX_STEPS {
            return Ok(reject(0, 0.));
        }
        let sample = |line: [Point; 2], out: &mut [f64; N]| {
            for (i, v) in out.iter_mut().enumerate() {
                let p = lerp(line[0], line[1], (i as f64 + 0.5) / N as f64);
                let x = p[0].floor();
                let y = p[1].floor();
                let fx = p[0] - x;
                let fy = p[1] - y;
                *v = (image.gray(x, y) * (1. - fx) + image.gray(x + 1., y) * fx) * (1. - fy)
                    + (image.gray(x, y + 1.) * (1. - fx) + image.gray(x + 1., y + 1.) * fx) * fy;
            }
            let mean = out.iter().sum::<f64>() / N as f64;
            for v in out.iter_mut() {
                *v -= mean;
            }
            (out.iter().map(|v| v * v).sum::<f64>() / N as f64).sqrt()
        };
        let std = sample(a, &mut self.reference);
        if std < 25. {
            return Ok(reject(1, 0.));
        }
        let mut minimum = 1_f64;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let current = [lerp(a[0], b[0], t), lerp(a[1], b[1], t)];
            let s = sample(current, &mut self.current);
            if s < 25_f64.max(std * contrast_ratio) {
                return Ok(reject(step + 1, 0.));
            }
            let correlation = self
                .reference
                .iter()
                .zip(self.current.iter())
                .map(|(x, y)| x * y)
                .sum::<f64>()
                / (N as f64 * std * s);
            minimum = minimum.min(correlation);
            if correlation < 0.85 {
                return Ok(reject(step + 1, minimum));
            }
        }
        Ok(Evidence {
            supported: true,
            paths: steps + 1,
            minimum_correlation: minimum.clamp(-1., 1.),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn band(y: f64) -> Quad {
        [[20., y], [400., y], [400., y + 4.], [20., y + 4.]]
    }
    fn pixels() -> Vec<u8> {
        let mut p = vec![255; 420 * 200];
        for y in 10..190 {
            for x in 20..400 {
                if (x / 4) % 3 == 0 || (x / 4) % 7 == 0 {
                    p[y * 420 + x] = 0;
                }
            }
        }
        p
    }
    #[test]
    fn continuous_bars_accept_and_quiet_gap_rejects() {
        let mut p = pixels();
        let mut c = Continuity::default();
        assert!(
            c.check(
                ImageView::new(&p, 420, 200, 1, 420).unwrap(),
                band(20.),
                band(160.)
            )
            .unwrap()
            .supported
        );
        for y in 99..102 {
            p[y * 420..(y + 1) * 420].fill(255);
        }
        assert!(
            !c.check(
                ImageView::new(&p, 420, 200, 1, 420).unwrap(),
                band(20.),
                band(160.)
            )
            .unwrap()
            .supported
        );
    }
    #[test]
    fn rotation_lines_validation_and_reuse() {
        let p = pixels();
        let mut rotated = vec![255; p.len()];
        for y in 0..200 {
            for x in 0..420 {
                rotated[x * 200 + 199 - y] = p[y * 420 + x];
            }
        }
        let rotate = |q: Quad| q.map(|[x, y]| [199. - y, x]);
        let mut c = Continuity::default();
        let im = ImageView::new(&rotated, 200, 420, 1, 200).unwrap();
        let line = [[20., 160.], [400., 160.], [400., 160.], [20., 160.]];
        assert!(
            c.check(im, rotate(band(20.)), rotate(line))
                .unwrap()
                .supported
        );
        assert!(c.check(im, [[f64::NAN; 2]; 4], rotate(line)).is_err());
        assert!(
            c.check(im, rotate(band(20.)), rotate(line))
                .unwrap()
                .supported
        );
        let blank = vec![255; 420 * 200];
        assert!(
            !c.check(
                ImageView::new(&blank, 420, 200, 1, 420).unwrap(),
                band(20.),
                line
            )
            .unwrap()
            .supported
        );
    }
    #[test]
    fn wrong_pattern_and_budget_reject() {
        let mut p = pixels();
        for y in 100..200 {
            for x in 20..400 {
                p[y * 420 + x] = if x % 8 < 4 { 0 } else { 255 };
            }
        }
        let mut c = Continuity::default();
        assert!(
            !c.check(
                ImageView::new(&p, 420, 200, 1, 420).unwrap(),
                band(20.),
                band(160.)
            )
            .unwrap()
            .supported
        );
        let tall = vec![0; 420 * 900];
        let e = c
            .check(
                ImageView::new(&tall, 420, 900, 1, 420).unwrap(),
                band(20.),
                band(800.),
            )
            .unwrap();
        assert!(!e.supported);
        assert_eq!(e.paths, 0);
    }
    #[test]
    fn public_early_path_rejects_fractional_one_pixel_gap() {
        let mut p = pixels();
        for y in 99..100 {
            p[y * 420..(y + 1) * 420].fill(255);
        }
        let im = ImageView::new(&p, 420, 200, 1, 420).unwrap();
        let mut c = Continuity::default();
        assert!(
            c.check_with_contrast(im, band(20.5), band(160.5), 0.5)
                .unwrap()
                .supported
        );
        assert!(!c.check(im, band(20.5), band(160.5)).unwrap().supported);
    }

    #[test]
    fn broad_box_cannot_hide_gap_outside_its_centre_corridor() {
        let mut p = pixels();
        for y in 99..102 {
            p[y * 420..(y + 1) * 420].fill(255);
        }
        let im = ImageView::new(&p, 420, 200, 1, 420).unwrap();
        let mut c = Continuity::default();
        let broad = [[20., 20.], [400., 20.], [400., 180.], [20., 180.]];
        // Centre100 is in the gap and rejects. Move the gap below the centre so only
        // the extra support section detects it from an upper anchor.
        let mut p = pixels();
        for y in 129..132 {
            p[y * 420..(y + 1) * 420].fill(255);
        }
        let im2 = ImageView::new(&p, 420, 200, 1, 420).unwrap();
        assert!(
            c.check_with_contrast(im2, band(20.), broad, 0.75)
                .unwrap()
                .supported
        );
        assert!(!c.check(im2, band(20.), broad).unwrap().supported);
        assert!(!c.check(im, band(20.), broad).unwrap().supported);
    }
}

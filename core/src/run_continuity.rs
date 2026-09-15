//! Experimental source correspondence by independently decoding every bridge strip.
//! No supplied text, geometry inflation, or modification of the original continuity policy.
#![forbid(unsafe_code)]
use crate::{
    sampling::{Error, ImageView},
    scan::{transform, Quad},
};
type Point = [f64; 2];
type Line = [Point; 2];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    None,
    GeometryGate,
    Budget,
    Bounds,
    Contrast,
    Decode,
    Conflict,
    Span,
}
#[derive(Clone, Copy, Debug)]
pub struct Evidence {
    pub supported: bool,
    pub profiles: usize,
    pub rejection: Rejection,
    pub digits: Option<[u8; 13]>,
}
impl Evidence {
    fn reject(profiles: usize, rejection: Rejection) -> Self {
        Self {
            supported: false,
            profiles,
            rejection,
            digits: None,
        }
    }
}
pub struct Correspondence {
    raw: [f64; 512],
    profile: [f32; 512],
}
impl Default for Correspondence {
    fn default() -> Self {
        Self {
            raw: [0.; 512],
            profile: [0.; 512],
        }
    }
}
fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}
fn distance(a: Point, b: Point) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
fn line(q: Quad, axis: usize, t: f64) -> Line {
    if axis == 0 {
        [lerp(q[0], q[3], t), lerp(q[1], q[2], t)]
    } else {
        [lerp(q[0], q[1], t), lerp(q[3], q[2], t)]
    }
}
fn inside(im: ImageView<'_>, p: Point) -> bool {
    p.iter().all(|v| v.is_finite())
        && p[0] >= 0.
        && p[1] >= 0.
        && p[0] <= (im.width - 1) as f64
        && p[1] <= (im.height - 1) as f64
}
impl Correspondence {
    /// All three destination cross-sections (center,20%,80%) are connected at <=1
    /// source-pixel endpoint spacing. Each corridor has <=512 steps; <=1537 profiles.
    /// A failed strip, contrast drop, competing decode, or budget preserves both reads.
    pub fn check(
        &mut self,
        im: ImageView<'_>,
        anchor: Quad,
        other: Quad,
    ) -> Result<Evidence, Error> {
        if anchor.iter().chain(other.iter()).any(|p| !inside(im, *p)) {
            return Err(Error::Geometry);
        }
        transform(anchor)?;
        if transform(other).is_err() {
            let mut far = other[0];
            for p in other {
                if distance(other[0], p) > distance(other[0], far) {
                    far = p;
                }
            }
            if distance(other[0], far) < 1.
                || other.iter().any(|p| {
                    ((far[0] - other[0][0]) * (p[1] - other[0][1])
                        - (far[1] - other[0][1]) * (p[0] - other[0][0]))
                        .abs()
                        > 1e-7 * distance(other[0], far)
                })
            {
                return Err(Error::Geometry);
            }
        }
        let a = line(anchor, 0, 0.5);
        let len = distance(a[0], a[1]);
        if len < 95. {
            return Ok(Evidence::reject(0, Rejection::GeometryGate));
        }
        let mut best = None;
        for axis in 0..2 {
            let mut b = line(other, axis, 0.5);
            let reverse = distance(a[0], b[1]) + distance(a[1], b[0])
                < distance(a[0], b[0]) + distance(a[1], b[1]);
            if reverse {
                b.swap(0, 1);
            }
            let blen = distance(b[0], b[1]);
            if !(0.75..=1.333_333).contains(&(blen / len)) {
                continue;
            }
            let cosine = ((a[1][0] - a[0][0]) * (b[1][0] - b[0][0])
                + (a[1][1] - a[0][1]) * (b[1][1] - b[0][1]))
                / (len * blen);
            if cosine < 0.95 {
                continue;
            }
            let d = distance(a[0], b[0]).max(distance(a[1], b[1]));
            if best.is_none_or(|(v, _, _)| d < v) {
                best = Some((d, axis, reverse));
            }
        }
        let Some((_, axis, reverse)) = best else {
            return Ok(Evidence::reject(0, Rejection::GeometryGate));
        };
        let mut corridors = [(a, 0usize); 3];
        for (i, f) in [0.5, 0.2, 0.8].into_iter().enumerate() {
            let mut b = line(other, axis, f);
            if reverse {
                b.swap(0, 1);
            }
            let steps = distance(a[0], b[0])
                .max(distance(a[1], b[1]))
                .ceil()
                .max(1.) as usize;
            if steps > 512 {
                return Ok(Evidence::reject(0, Rejection::Budget));
            }
            corridors[i] = (b, steps);
        }
        let (reference, std) = match self.decode(im, a, 0.) {
            Ok(v) => v,
            Err(r) => return Ok(Evidence::reject(1, r)),
        };
        let mut profiles = 1;
        for (b, steps) in corridors {
            for step in 1..=steps {
                let t = step as f64 / steps as f64;
                let current = [lerp(a[0], b[0], t), lerp(a[1], b[1], t)];
                profiles += 1;
                match self.decode(im, current, std) {
                    Ok((digits, _)) if digits == reference => {}
                    Ok(_) => return Ok(Evidence::reject(profiles, Rejection::Conflict)),
                    Err(r) => return Ok(Evidence::reject(profiles, r)),
                }
            }
        }
        Ok(Evidence {
            supported: true,
            profiles,
            rejection: Rejection::None,
            digits: Some(reference),
        })
    }
    fn decode(
        &mut self,
        im: ImageView<'_>,
        l: Line,
        anchor_std: f64,
    ) -> Result<([u8; 13], f64), Rejection> {
        if !inside(im, lerp(l[0], l[1], -0.15)) || !inside(im, lerp(l[0], l[1], 1.15)) {
            return Err(Rejection::Bounds);
        }
        let (mut lo, mut hi) = (255_f64, 0_f64);
        for (i, v) in self.raw.iter_mut().enumerate() {
            let p = lerp(l[0], l[1], -0.15 + 1.3 * (i as f64 + 0.5) / 512.);
            let (x, y) = (p[0].floor(), p[1].floor());
            let (fx, fy) = (p[0] - x, p[1] - y);
            *v = (im.gray(x, y) * (1. - fx) + im.gray(x + 1., y) * fx) * (1. - fy)
                + (im.gray(x, y + 1.) * (1. - fx) + im.gray(x + 1., y + 1.) * fx) * fy;
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        let middle = &self.raw[60..452];
        let mean = middle.iter().sum::<f64>() / middle.len() as f64;
        let std =
            (middle.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / middle.len() as f64).sqrt();
        if std < 8_f64.max(anchor_std * 0.75) || hi - lo < 8. {
            return Err(Rejection::Contrast);
        }
        for (p, v) in self.profile.iter_mut().zip(self.raw.iter()) {
            *p = ((hi - v) / (hi - lo)) as f32;
        }
        let r = crate::run_profile::decode(&self.profile)
            .map_err(|_| Rejection::Decode)?
            .ok_or(Rejection::Decode)?;
        if (r.left - (512. * 0.15 / 1.3 - 0.5)).abs() > 16.
            || (r.right - (512. * 1.15 / 1.3 - 0.5)).abs() > 16.
        {
            return Err(Rejection::Span);
        }
        Ok((r.digits, std))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const BITS:&str="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    fn band(y: f64) -> Quad {
        [[70., y], [450., y], [450., y + 4.], [70., y + 4.]]
    }
    #[test]
    fn verified_corridors_reject_one_pixel_gap_and_wrong_value() {
        let mut c = Correspondence::default();
        let mut p = vec![255; 520 * 200];
        for y in 10..190 {
            for x in 70..450 {
                if BITS.as_bytes()[(x - 70) / 4] == b'1' {
                    p[y * 520 + x] = 0;
                }
            }
        }
        assert!(
            c.check(
                ImageView::new(&p, 520, 200, 1, 520).unwrap(),
                band(20.5),
                band(160.5)
            )
            .unwrap()
            .supported
        );
        p[99 * 520..100 * 520].fill(255);
        let e = c
            .check(
                ImageView::new(&p, 520, 200, 1, 520).unwrap(),
                band(20.5),
                band(160.5),
            )
            .unwrap();
        assert!(!e.supported);
        assert_eq!(e.rejection, Rejection::Contrast);
        assert!(c
            .check(
                ImageView::new(&p, 520, 200, 1, 520).unwrap(),
                [[f64::NAN; 2]; 4],
                band(160.)
            )
            .is_err());
    }
    #[test]
    fn tilted_paths_and_broad_destination_do_not_hide_gap() {
        let mut p = vec![255; 520 * 200];
        for y in 10..190 {
            for x in 70..450 {
                if BITS.as_bytes()[(x - 70) / 4] == b'1' {
                    p[y * 520 + x] = 0;
                }
            }
        }
        let a = [[70., 20.], [450., 75.], [450., 79.], [70., 24.]];
        let b = band(140.);
        let mut c = Correspondence::default();
        assert!(
            c.check(ImageView::new(&p, 520, 200, 1, 520).unwrap(), a, b)
                .unwrap()
                .supported
        );
        p[129 * 520..130 * 520].fill(255);
        let broad = [[70., 20.], [450., 20.], [450., 180.], [70., 180.]];
        assert!(
            !c.check(
                ImageView::new(&p, 520, 200, 1, 520).unwrap(),
                band(20.),
                broad
            )
            .unwrap()
            .supported
        );
    }
    #[test]
    fn rotated_sloped_gap_conflict_and_budget_regressions() {
        for turn in 0..4 {
            for gap in [false, true] {
                let (mut w, mut h) = (520usize, 200usize);
                let mut p = vec![255; w * h];
                for y in 10..190 {
                    if gap && y == 99 {
                        continue;
                    }
                    for x in 70..450 {
                        if BITS.as_bytes()[(x - 70) / 4] == b'1' {
                            p[y * w + x] = 0;
                        }
                    }
                }
                let mut a = [[70., 20.5], [450., 75.5], [450., 79.5], [70., 24.5]];
                let mut b = band(160.5);
                for _ in 0..turn {
                    let mut next = vec![255; p.len()];
                    for y in 0..h {
                        for x in 0..w {
                            next[x * h + h - 1 - y] = p[y * w + x];
                        }
                    }
                    a = a.map(|[x, y]| [(h - 1) as f64 - y, x]);
                    b = b.map(|[x, y]| [(h - 1) as f64 - y, x]);
                    p = next;
                    (w, h) = (h, w);
                }
                let e = Correspondence::default()
                    .check(ImageView::new(&p, w, h, 1, w).unwrap(), a, b)
                    .unwrap();
                assert_eq!(e.supported, !gap, "rotation {turn} gap {gap} {e:?}");
                assert!(e.profiles <= 1537);
            }
        }
        let mut p = vec![255; 520 * 200];
        let b = crate::ean::encode(&[4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1]);
        for y in 10..190 {
            for x in 70..450 {
                if if y < 100 {
                    BITS.as_bytes()[(x - 70) / 4] == b'1'
                } else {
                    b[(x - 70) / 4] > 0.5
                } {
                    p[y * 520 + x] = 0;
                }
            }
        }
        assert!(
            !Correspondence::default()
                .check(
                    ImageView::new(&p, 520, 200, 1, 520).unwrap(),
                    band(20.),
                    band(160.)
                )
                .unwrap()
                .supported
        );
        let p = vec![255; 520 * 900];
        let e = Correspondence::default()
            .check(
                ImageView::new(&p, 520, 900, 1, 520).unwrap(),
                band(20.),
                band(800.),
            )
            .unwrap();
        assert_eq!(e.rejection, Rejection::Budget);
        assert_eq!(e.profiles, 0);
    }
}

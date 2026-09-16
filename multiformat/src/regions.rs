//! Localized candidates survive payload failure. Scores describe geometry/template
//! agreement and are not calibrated probabilities of a correct decode.
use crate::Detection;
use serde::Serialize;
type Quad = [[f32; 2]; 4];
fn area(p: &[[f32; 2]]) -> f32 {
    if p.len() < 3 {
        return 0.;
    }
    p.iter()
        .enumerate()
        .map(|(i, a)| {
            let b = p[(i + 1) % p.len()];
            a[0] * b[1] - a[1] * b[0]
        })
        .sum::<f32>()
        / 2.
}
#[must_use]
pub fn overlap(first: &Quad, second: &Quad) -> f32 {
    let first_area = area(first).abs();
    let second_area = area(second).abs();
    if first_area.min(second_area) < 0.001 {
        return 0.;
    }
    let sign = area(second).signum();
    let mut points = first.to_vec();
    for i in 0..4 {
        let p = second[i];
        let q = second[(i + 1) % 4];
        let side =
            |r: [f32; 2]| sign * ((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]));
        let mut clipped = Vec::new();
        for j in 0..points.len() {
            let u = points[j];
            let v = points[(j + 1) % points.len()];
            let su = side(u);
            let sv = side(v);
            if su >= 0. {
                clipped.push(u);
            }
            if (su >= 0.) != (sv >= 0.) {
                let fraction = su / (su - sv);
                clipped.push([
                    u[0] + fraction * (v[0] - u[0]),
                    u[1] + fraction * (v[1] - u[1]),
                ]);
            }
        }
        points = clipped;
    }
    area(&points).abs().min(first_area.min(second_area)) / first_area.min(second_area)
}

// `overlap` is normalized by the smaller input polygon.  Suppression needs
// the fraction of the localized region covered by a decoded result instead.
fn region_coverage(region: &Quad, decoded: &Quad) -> f32 {
    if region
        .iter()
        .chain(decoded.iter())
        .flatten()
        .any(|v| !v.is_finite())
    {
        return 0.;
    }
    let region_area = area(region).abs();
    let decoded_area = area(decoded).abs();
    if !region_area.is_finite()
        || !decoded_area.is_finite()
        || region_area < 0.001
        || decoded_area < 0.001
    {
        return 0.;
    }
    overlap(decoded, region) * region_area.min(decoded_area) / region_area
}
#[derive(Clone, Serialize)]
pub struct Region {
    pub format: String,
    pub text: String,
    pub polygon: Quad,
    #[serde(rename = "localizationScore")]
    pub localization_score: f32,
    pub support: usize,
}
#[derive(Default)]
pub struct Regions {
    pub candidates: Vec<Region>,
    pub limited: bool,
}
impl Regions {
    pub fn add(&mut self, format: &str, polygon: Quad, score: f32, support: usize) {
        if polygon.iter().flatten().any(|v| !v.is_finite()) || area(&polygon).abs() < 4. {
            return;
        }
        let region = Region {
            format: format.into(),
            text: String::new(),
            polygon,
            localization_score: score,
            support,
        };
        if let Some(old) = self
            .candidates
            .iter_mut()
            .find(|r| r.format == format && overlap(&r.polygon, &polygon) >= 0.7)
        {
            if score > old.localization_score {
                *old = region;
            }
        } else if self.candidates.len() < 256 {
            self.candidates.push(region);
        } else {
            self.limited = true;
        }
    }
    #[must_use]
    pub fn finish(mut self, decoded: &[Detection]) -> (Vec<Region>, bool) {
        self.candidates.retain(|r| {
            !decoded
                .iter()
                .any(|d| d.format == r.format && region_coverage(&r.polygon, &d.polygon) >= 0.7)
        });
        (self.candidates, self.limited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detection(format: &str, polygon: Quad) -> Detection {
        Detection {
            bytes: None,
            structured_append: None,
            reader_initialization: false,
            addon: None,
            format: format.into(),
            text: String::new(),
            polygon,
            support: 1,
            error: 0.,
            gs1: false,
        }
    }

    const REGION: Quad = [[0., 0.], [100., 0.], [100., 100.], [0., 100.]];

    #[test]
    fn small_decoded_result_inside_aggregate_keeps_region() {
        let small = [[0., 0.], [20., 0.], [20., 20.], [0., 20.]];
        let mut regions = Regions::default();
        regions.add("QRCode", REGION, 1., 1);
        let (remaining, limited) = regions.finish(&[detection("QRCode", small)]);
        assert_eq!(remaining.len(), 1);
        assert!(!limited);
        assert!(region_coverage(&REGION, &small) < 0.7);
    }

    #[test]
    fn fully_covered_region_is_removed() {
        let mut regions = Regions::default();
        regions.add("QRCode", REGION, 1., 1);
        let (remaining, _) = regions.finish(&[detection("QRCode", REGION)]);
        assert!(remaining.is_empty());
    }

    #[test]
    fn different_format_does_not_suppress_region() {
        let mut regions = Regions::default();
        regions.add("QRCode", REGION, 1., 1);
        let (remaining, _) = regions.finish(&[detection("DataMatrix", REGION)]);
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn reversed_quad_with_same_geometry_suppresses_region() {
        let reversed = [[0., 100.], [100., 100.], [100., 0.], [0., 0.]];
        assert!(region_coverage(&REGION, &reversed) >= 0.99);
        let mut regions = Regions::default();
        regions.add("QRCode", REGION, 1., 1);
        let (remaining, _) = regions.finish(&[detection("QRCode", reversed)]);
        assert!(remaining.is_empty());
    }
}

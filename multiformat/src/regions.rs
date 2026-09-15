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
                .any(|d| d.format == r.format && overlap(&d.polygon, &r.polygon) >= 0.7)
        });
        (self.candidates, self.limited)
    }
}

//! Measured source-bar tracks. The enclosing display quad is never ownership evidence.
use super::{distance, line, Evidence};
use crate::Quad;

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Rounded finite coordinates are checked against image bounds."
)]
fn sample(e: &mut Evidence<'_>, p: [f64; 2]) -> Option<f64> {
    if e.remaining == 0 || !p.iter().all(|x| x.is_finite()) {
        return None;
    }
    e.remaining -= 1;
    let x = (p[0] + 0.5).floor();
    let y = (p[1] + 0.5).floor();
    if x < 0.
        || y < 0.
        || x >= f64::from(u32::try_from(e.image.width).ok()?)
        || y >= f64::from(u32::try_from(e.image.height).ok()?)
    {
        return None;
    }
    let at = y as usize * e.image.stride + x as usize * e.image.channels;
    let data = e.image.data;
    Some(if e.image.channels == 1 {
        f64::from(data[at])
    } else {
        (77. * f64::from(data[at]) + 150. * f64::from(data[at + 1]) + 29. * f64::from(data[at + 2]))
            / 256.
    })
}

struct Track {
    fraction: f64,
    width: f64,
    points: Vec<[f64; 2]>,
}
pub(super) struct Footprint {
    pub(super) polygon: Quad,
    axis: [f64; 2],
    tracks: Vec<Track>,
}

fn point(origin: [f64; 2], u: [f64; 2], x: f64, y: f64) -> [f64; 2] {
    [
        origin[0] + x * u[0] - y * u[1],
        origin[1] + x * u[1] + y * u[0],
    ]
}
fn project(p: [f64; 2], origin: [f64; 2], u: [f64; 2]) -> [f64; 2] {
    let d = [p[0] - origin[0], p[1] - origin[1]];
    [d[0] * u[0] + d[1] * u[1], -d[0] * u[1] + d[1] * u[0]]
}

enum CrossSection {
    End,
    Ink {
        center: f64,
        ceiling: f64,
        intensity: f64,
    },
}

// Recenter inside the same dark run. A light gap ends a track; no dilation or
// gap bridging can silently connect two physical labels.
fn center(
    e: &mut Evidence<'_>,
    origin: [f64; 2],
    u: [f64; 2],
    at: [f64; 2],
    width: f64,
) -> Option<CrossSection> {
    let mut x = at[0];
    let mut dark = sample(e, point(origin, u, x, at[1]))?;
    for shift in [-0.5, 0.5] {
        let v = sample(e, point(origin, u, at[0] + shift, at[1]))?;
        if v < dark {
            dark = v;
            x = at[0] + shift;
        }
    }
    let mut paper = dark;
    for sign in [-1., 1.] {
        for radius in [0.65, 1., 1.5, 2.] {
            paper = paper.max(sample(
                e,
                point(origin, u, x + sign * (width * radius + 0.5), at[1]),
            )?);
        }
    }
    if paper - dark < 48. {
        return Some(CrossSection::End);
    }
    let cut = dark.midpoint(paper);
    let mut edges = [0.; 2];
    for (index, sign) in [-1., 1.].into_iter().enumerate() {
        let mut radius = 0.5;
        loop {
            if radius > width * 2.5 + 1. {
                return Some(CrossSection::End);
            }
            if sample(e, point(origin, u, x + sign * radius, at[1]))? >= cut {
                edges[index] = x + sign * (radius - 0.25);
                break;
            }
            radius += 0.75;
        }
    }
    let w = edges[1] - edges[0];
    Some(if w >= width * 0.35 && w <= width * 2.5 + 1. {
        let center = edges[0].midpoint(edges[1]);
        let intensity = e.smooth(point(origin, u, center, at[1]))?;
        CrossSection::Ink {
            center,
            ceiling: intensity + (paper - intensity) * 0.3,
            intensity,
        }
    } else {
        CrossSection::End
    })
}

fn follow(
    e: &mut Evidence<'_>,
    origin: [f64; 2],
    u: [f64; 2],
    x: f64,
    width: f64,
    direction: f64,
) -> Option<Vec<[f64; 2]>> {
    let mut points = vec![point(origin, u, x, 0.)];
    let mut current = x;
    let mut previous_ceiling = None;
    for step in 1..=1536 {
        let y = direction * f64::from(step) * 0.5;
        let CrossSection::Ink {
            center: next,
            ceiling: ink_cut,
            intensity,
        } = center(e, origin, u, [current, y], width)?
        else {
            return Some(points);
        };
        let at = point(origin, u, next, y);
        let previous = *points.last()?;
        let middle = [previous[0].midpoint(at[0]), previous[1].midpoint(at[1])];
        let ceiling = previous_ceiling.unwrap_or(ink_cut);
        if intensity >= ceiling || e.smooth(middle)? >= ceiling {
            // A lighting step can leave continuous ink beyond this point.
            // Reject a persistent sharp change instead of certifying a partial extent.
            if ink_cut > ceiling + 24. {
                if let CrossSection::Ink { ceiling: ahead, .. } =
                    center(e, origin, u, [next, y + direction * 2.], width)?
                {
                    if ahead > ceiling + 24. {
                        return None;
                    }
                }
            }
            return Some(points);
        }
        previous_ceiling = Some(ceiling * 0.9 + ink_cut * 0.1);
        current = next;
        points.push(at);
    }
    // Hitting a work limit does not establish a physical end.
    None
}

fn boundary(points: &[[f64; 2]], lower: bool) -> Option<(f64, f64)> {
    let n = f64::from(u32::try_from(points.len()).ok()?);
    let mx = points.iter().map(|p| p[0]).sum::<f64>() / n;
    let my = points.iter().map(|p| p[1]).sum::<f64>() / n;
    let variance = points.iter().map(|p| (p[0] - mx).powi(2)).sum::<f64>();
    if variance < 1. {
        return None;
    }
    let slope = points
        .iter()
        .map(|p| (p[0] - mx) * (p[1] - my))
        .sum::<f64>()
        / variance;
    let offset = points
        .iter()
        .map(|p| p[1] - slope * p[0])
        .reduce(if lower { f64::min } else { f64::max })?;
    Some((slope, offset))
}
fn envelope(tracks: &[Track], origin: [f64; 2], u: [f64; 2]) -> Option<Quad> {
    let top: Vec<_> = tracks
        .iter()
        .map(|t| project(t.points[0], origin, u))
        .collect();
    let bottom: Vec<_> = tracks
        .iter()
        .map(|t| project(*t.points.last().unwrap(), origin, u))
        .collect();
    let lo = boundary(&top, true)?;
    let hi = boundary(&bottom, false)?;
    let end = |t: &Track, lower| {
        let points: Vec<_> = t
            .points
            .iter()
            .map(|&p| {
                let p = project(p, origin, u);
                [p[1], p[0]]
            })
            .collect();
        boundary(&points, lower)
    };
    let mut first = end(tracks.first()?, true)?;
    let mut last = end(tracks.last()?, false)?;
    first.1 -= tracks.first()?.width * 0.5;
    last.1 += tracks.last()?.width * 0.5;
    let intersection = |a: (f64, f64), b: (f64, f64)| {
        let denom = 1. - a.0 * b.0;
        if denom.abs() < 0.2 {
            return None;
        }
        let x = (b.0 * a.1 + b.1) / denom;
        Some(point(origin, u, x, a.0 * x + a.1))
    };
    Some([
        intersection(lo, first)?,
        intersection(lo, last)?,
        intersection(hi, last)?,
        intersection(hi, first)?,
    ])
}

fn coherent(tracks: &[Track], origin: [f64; 2], u: [f64; 2]) -> bool {
    // A seed line may splice two labels. Their individually valid bar tracks
    // must not be enclosed as one symbol across a light separator.
    for pair in tracks.windows(2) {
        let bounds = |track: &Track| {
            let low = project(track.points[0], origin, u)[1];
            let high = project(*track.points.last().unwrap(), origin, u)[1];
            (low, high)
        };
        let a = bounds(&pair[0]);
        let b = bounds(&pair[1]);
        if a.1.min(b.1) - a.0.max(b.0) < 0.2 * (a.1 - a.0).max(b.1 - b.0) {
            return false;
        }
    }
    true
}

pub(super) fn measure(evidence: &mut Evidence<'_>, q: Quad) -> Option<Footprint> {
    let seed = line(q);
    let w = distance(seed[0], seed[1]);
    if !(48. ..=4096.).contains(&w) {
        return None;
    }
    let u = [(seed[1][0] - seed[0][0]) / w, (seed[1][1] - seed[0][1]) / w];
    let mut values = [0.; 1024];
    let mut ordered = [0.; 1024];
    for (i, value) in values.iter_mut().enumerate() {
        *value = sample(
            evidence,
            point(
                seed[0],
                u,
                (f64::from(u32::try_from(i).ok()?) + 0.5) * w * 1.06 / 1024. - w * 0.03,
                0.,
            ),
        )?;
    }
    ordered.copy_from_slice(&values);
    ordered.sort_by(f64::total_cmp);
    if ordered[921] - ordered[102] < 48. {
        return None;
    }
    let cut = ordered[102].midpoint(ordered[921]);
    let mut seeds = Vec::new();
    let mut i = 1;
    while i < 1023 {
        if values[i] >= cut || values[i - 1] < cut {
            i += 1;
            continue;
        }
        let start = i;
        while i < 1023 && values[i] < cut {
            i += 1;
        }
        let width = f64::from(u32::try_from(i - start).ok()?) * w * 1.06 / 1024.;
        if i < 1023 && (0.8..=w * 0.07).contains(&width) {
            seeds.push((
                (f64::from(u32::try_from(start + i).ok()?) * 0.5) * 1.06 / 1024. - 0.03,
                width,
            ));
        }
    }
    if seeds.len() < 12 {
        return None;
    }
    let mut tracks = Vec::new();
    for index in 0..12 {
        let (fraction, width) = seeds[index * (seeds.len() - 1) / 11];
        let Some(mut left) = follow(evidence, seed[0], u, fraction * w, width, -1.) else {
            continue;
        };
        let Some(right) = follow(evidence, seed[0], u, fraction * w, width, 1.) else {
            continue;
        };
        left.reverse();
        left.extend(right.into_iter().skip(1));
        if left.len() >= 9 {
            tracks.push(Track {
                fraction,
                width,
                points: left,
            });
        }
    }
    if tracks.len() < 9 || tracks.last()?.fraction - tracks.first()?.fraction < 0.8 {
        return None;
    }
    if !coherent(&tracks, seed[0], u) {
        return None;
    }
    let polygon = envelope(&tracks, seed[0], u)?;
    // Reject unstable line intersections rather than extrapolate a large search box.
    let mut bounds = [[f64::INFINITY, f64::NEG_INFINITY]; 2];
    for p in tracks.iter().flat_map(|t| &t.points) {
        for dim in 0..2 {
            bounds[dim][0] = bounds[dim][0].min(p[dim]);
            bounds[dim][1] = bounds[dim][1].max(p[dim]);
        }
    }
    let slack = 2. + w * 0.1;
    if polygon.iter().any(|p| {
        (0..2).any(|dim| {
            !p[dim].is_finite()
                || p[dim] < bounds[dim][0] - slack
                || p[dim] > bounds[dim][1] + slack
        })
    }) {
        return None;
    }
    Some(Footprint {
        polygon,
        axis: u,
        tracks,
    })
}

impl Footprint {
    pub(super) fn owns(&self, q: Quad) -> bool {
        let l = line(q);
        let v = [l[1][0] - l[0][0], l[1][1] - l[0][1]];
        let width = v[0].hypot(v[1]);
        if width < 24. || (v[0] * self.axis[0] + v[1] * self.axis[1]).abs() / width < 0.8 {
            return false;
        }
        let cross = |p: [f64; 2]| ((p[0] - l[0][0]) * v[1] - (p[1] - l[0][1]) * v[0]) / width;
        let along =
            |p: [f64; 2]| ((p[0] - l[0][0]) * v[0] + (p[1] - l[0][1]) * v[1]) / (width * width);
        let mut hits = Vec::new();
        for t in &self.tracks {
            if t.points
                .windows(2)
                .any(|p| cross(p[0]) * cross(p[1]) <= 0. && (0.0..=1.0).contains(&along(p[0])))
            {
                hits.push(t.fraction);
            }
        }
        hits.len() >= 6
            && hits
                .last()
                .zip(hits.first())
                .is_some_and(|(hi, lo)| hi - lo >= 0.6)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pixels() -> Vec<u8> {
        let mut pixels = vec![240; 320 * 260];
        for y in 20..240 {
            for x in 60..252 {
                if (x - 60) / 3 % 3 == 0 {
                    pixels[y * 320 + x] = 20;
                }
            }
        }
        pixels
    }
    fn measure_pixels(pixels: &[u8], y: f64) -> Footprint {
        let mut e = Evidence {
            image: crate::Image {
                data: pixels,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
            remaining: 262_144,
        };
        measure(
            &mut e,
            [[60., y - 2.], [252., y - 2.], [252., y + 2.], [60., y + 2.]],
        )
        .unwrap()
    }
    #[test]
    fn follows_to_physical_ends_not_the_decoding_band() {
        let p = pixels();
        let f = measure_pixels(&p, 70.);
        assert!(f.polygon[0][1] < 22. && f.polygon[2][1] > 237.);
        assert!(f.owns([[60., 178.], [252., 178.], [252., 182.], [60., 182.]]));
    }
    #[test]
    fn one_pixel_separator_ends_ownership_even_inside_an_enclosing_box() {
        let mut p = pixels();
        p[120 * 320..121 * 320].fill(240);
        let f = measure_pixels(&p, 70.);
        assert!(f.polygon[2][1] < 121.);
        assert!(!f.owns([[60., 178.], [252., 178.], [252., 182.], [60., 182.]]));
    }
    #[test]
    fn exhausted_budget_does_not_certify_an_extent() {
        let p = pixels();
        let mut e = Evidence {
            image: crate::Image {
                data: &p,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
            remaining: 1100,
        };
        assert!(measure(&mut e, [[60., 68.], [252., 68.], [252., 72.], [60., 72.]]).is_none());
    }
    #[test]
    fn slowly_changing_illumination_does_not_truncate_bars() {
        let mut p = pixels();
        for y in 20..240 {
            for x in 60..252 {
                p[y * 320 + x] = if (x - 60) / 3 % 3 == 0 {
                    20 + u8::try_from(y / 3).unwrap()
                } else {
                    120 + u8::try_from(y / 3).unwrap()
                };
            }
        }
        let f = measure_pixels(&p, 70.);
        assert!(f.polygon[0][1] < 22. && f.polygon[2][1] > 237.);
    }
    #[test]
    fn abrupt_illumination_step_does_not_certify_a_partial_extent() {
        let mut p = pixels();
        for y in 120..240 {
            for x in 60..252 {
                if p[y * 320 + x] < 100 {
                    p[y * 320 + x] = 110;
                }
            }
        }
        let mut e = Evidence {
            image: crate::Image {
                data: &p,
                width: 320,
                height: 260,
                channels: 1,
                stride: 320,
            },
            remaining: 262_144,
        };
        assert!(measure(&mut e, [[60., 68.], [252., 68.], [252., 72.], [60., 72.]]).is_none());
    }
    #[test]
    fn uncertain_bars_do_not_discard_other_independent_tracks() {
        let mut p = pixels();
        for y in 120..240 {
            for start in [87, 141, 204] {
                p[y * 320 + start..y * 320 + start + 3].fill(110);
            }
        }
        let f = measure_pixels(&p, 70.);
        assert!(f.polygon[0][1] < 22. && f.polygon[2][1] > 237.);
        assert!(f.owns([[60., 178.], [252., 178.], [252., 182.], [60., 182.]]));
    }
}

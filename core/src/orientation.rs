//! Bounded source-gradient orientation provider. It proposes sampling geometry,
//! never text, and preserves the identity of every supplied candidate.
#![forbid(unsafe_code)]
use crate::{
    experiment::{self, Experiment},
    frame::Frame,
    multi_scan::Policy,
    sampling::{Error, ImageView},
    scan::{self, Quad},
};
#[derive(Debug, Default)]
pub struct OrientationWork {
    pub pixels: usize,
    pub tiles: usize,
    pub directions: usize,
    pub proposals: usize,
    pub pending: usize,
    pub truncated: bool,
}
#[derive(Debug)]
pub struct OrientedFrame {
    pub frame: Frame,
    pub proposal_sources: Vec<usize>,
    pub orientation: OrientationWork,
}
fn angle_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % std::f64::consts::PI;
    d.min(std::f64::consts::PI - d)
}
fn envelope(q: Quad, angle: f64) -> Quad {
    let (s, c) = angle.sin_cos();
    let mut bounds = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in q {
        let (u, v) = (c * x + s * y, -s * x + c * y);
        bounds[0] = bounds[0].min(u);
        bounds[1] = bounds[1].max(u);
        bounds[2] = bounds[2].min(v);
        bounds[3] = bounds[3].max(v);
    }
    let [l, r, t, b] = bounds;
    [[l, t], [r, t], [r, b], [l, b]].map(|[u, v]| [c * u - s * v, s * u + c * v])
}
/// At most 64*48*48*8 source reads; no image allocation or rescaling.
fn directions(im: ImageView<'_>, q: Quad, work: &mut OrientationWork) -> Vec<f64> {
    let Ok(m) = scan::transform(q) else {
        return vec![];
    };
    let mut tensors = [[0.; 4]; 16];
    for y in 0..48 {
        for x in 0..48 {
            let Ok([px, py]) =
                experiment::point(m.0, 0, (x as f64 + 0.5) / 48., (y as f64 + 0.5) / 48.)
            else {
                continue;
            };
            if px < 2. || py < 2. || px > im.width as f64 - 3. || py > im.height as f64 - 3. {
                continue;
            }
            // Sobel derivative at original-image scale. A symmetric stencil avoids a
            // one-sided phase offset; the tensor does not depend on light/dark polarity.
            let a = im.gray(px - 1., py - 1.);
            let b = im.gray(px, py - 1.);
            let c = im.gray(px + 1., py - 1.);
            let d = im.gray(px - 1., py);
            let e = im.gray(px + 1., py);
            let f = im.gray(px - 1., py + 1.);
            let g = im.gray(px, py + 1.);
            let h = im.gray(px + 1., py + 1.);
            work.pixels += 8;
            let gx = c + 2. * e + h - a - 2. * d - f;
            let gy = f + 2. * g + h - a - 2. * b - c;
            if gx * gx + gy * gy < 1600. {
                continue;
            }
            let t = &mut tensors[(y / 12) * 4 + x / 12];
            t[0] += gx * gx;
            t[1] += gx * gy;
            t[2] += gy * gy;
            t[3] += 1.;
        }
    }
    let mut groups: Vec<(f64, f64, f64)> = vec![];
    for [xx, xy, yy, n] in tensors {
        work.tiles += 1;
        let energy = xx + yy;
        if n < 12. || energy <= 0. {
            continue;
        }
        let coherence = ((xx - yy).hypot(2. * xy)) / energy;
        if coherence < 0.75 {
            continue;
        }
        let angle = 0.5 * (2. * xy).atan2(xx - yy);
        // Original geometry already samples both axes. Add only genuinely tilted
        // source directions; this is a work allocation gate, not a decoder gate.
        let aligned = (0..2).any(|axis| {
            let a = q[axis];
            let b = q[axis + 1];
            angle_distance(angle, (b[1] - a[1]).atan2(b[0] - a[0])) < 5f64.to_radians()
        });
        if aligned {
            continue;
        }
        if let Some(g) = groups
            .iter_mut()
            .find(|g| angle_distance(angle, 0.5 * g.1.atan2(g.0)) < 10f64.to_radians())
        {
            g.0 += energy * (2. * angle).cos();
            g.1 += energy * (2. * angle).sin();
            g.2 += energy;
        } else {
            groups.push((
                energy * (2. * angle).cos(),
                energy * (2. * angle).sin(),
                energy,
            ));
        }
    }
    groups.sort_by(|a, b| b.2.total_cmp(&a.2));
    work.directions += groups.len();
    if groups.len() > 2 {
        work.pending += groups.len() - 2;
        work.truncated = true;
    }
    groups
        .into_iter()
        .take(2)
        .map(|(x, y, _)| 0.5 * y.atan2(x))
        .collect()
}
impl Experiment {
    /// Keeps originals first, then round-robin orientation retries. All supplied
    /// and derived candidates receive a cheap pass before decoder retry allocation.
    pub fn scan_oriented_frame(
        &mut self,
        im: ImageView<'_>,
        quads: &[Quad],
        policy: Policy,
    ) -> Result<OrientedFrame, Error> {
        if quads.len() > 64 {
            return Err(Error::Parameters);
        }
        let mut work = OrientationWork::default();
        let mut expanded = quads.to_vec();
        let mut sources: Vec<_> = (0..quads.len()).collect();
        let plans: Vec<_> = quads
            .iter()
            .map(|&q| directions(im, q, &mut work))
            .collect();
        for rank in 0..2 {
            for (i, angles) in plans.iter().enumerate() {
                let Some(&angle) = angles.get(rank) else {
                    continue;
                };
                if expanded.len() == 64 {
                    work.pending += 1;
                    work.truncated = true;
                    continue;
                }
                expanded.push(envelope(quads[i], angle));
                sources.push(i);
                work.proposals += 1;
            }
        }
        let mut frame = self.scan_frame(im, &expanded, policy)?;
        for barcode in &mut frame.barcodes {
            barcode.candidate_indices = barcode
                .candidate_indices
                .iter()
                .map(|&i| sources[i])
                .collect();
            barcode.candidate_indices.sort_unstable();
            barcode.candidate_indices.dedup();
        }
        frame.unfinished |= work.truncated;
        Ok(OrientedFrame {
            frame,
            proposal_sources: sources,
            orientation: work,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotated_envelope_contains_original_and_has_requested_axis() {
        let q = [[10., 20.], [410., 20.], [410., 220.], [10., 220.]];
        let a = 0.47;
        let r = envelope(q, a);
        let (s, c) = a.sin_cos();
        assert!(angle_distance((r[1][1] - r[0][1]).atan2(r[1][0] - r[0][0]), a) < 1e-12);
        let us: Vec<_> = r.iter().map(|p| p[0] * c + p[1] * s).collect();
        let vs: Vec<_> = r.iter().map(|p| -p[0] * s + p[1] * c).collect();
        for p in q {
            let u = p[0] * c + p[1] * s;
            let v = -p[0] * s + p[1] * c;
            assert!(
                u >= us[0] - 1e-9 && u <= us[1] + 1e-9 && v >= vs[0] - 1e-9 && v <= vs[2] + 1e-9
            );
        }
    }
    #[test]
    fn source_tensor_finds_tilt_without_label_and_rejects_blank() {
        let a = 0.47f64;
        let mut pixels = vec![255; 512 * 512];
        for y in 0..512 {
            for x in 0..512 {
                if ((x as f64 * a.cos() + y as f64 * a.sin()) / 8.).floor() as i64 % 2 == 0 {
                    pixels[y * 512 + x] = 0;
                }
            }
        }
        let q = [[0., 0.], [512., 0.], [512., 512.], [0., 512.]];
        let im = ImageView::new(&pixels, 512, 512, 1, 512).unwrap();
        let mut work = OrientationWork::default();
        let ds = directions(im, q, &mut work);
        assert!(!ds.is_empty());
        assert!(angle_distance(ds[0], a) < 0.08);
        assert!(work.pixels <= 48 * 48 * 8);
        pixels.fill(255);
        assert!(directions(
            ImageView::new(&pixels, 512, 512, 1, 512).unwrap(),
            q,
            &mut work
        )
        .is_empty());
    }
    #[test]
    fn original_candidates_survive_orientation_capacity_exhaustion() {
        let mut pixels = vec![255; 256 * 256];
        for y in 0..256 {
            for x in 0..256 {
                if (x + y / 2) % 12 < 6 {
                    pixels[y * 256 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, 256, 256, 1, 256).unwrap();
        let q = [[0., 0.], [256., 0.], [256., 256.], [0., 256.]];
        let f = Experiment::default()
            .scan_oriented_frame(
                im,
                &[q; 64],
                Policy {
                    max_retry_paths_per_candidate: 0,
                    max_retry_paths_per_frame: 0,
                    ..Policy::default()
                },
            )
            .unwrap();
        assert_eq!(f.frame.candidates.len(), 64);
        assert_eq!(f.proposal_sources, (0..64).collect::<Vec<_>>());
        assert!(f.orientation.pending >= 64);
        assert!(f.orientation.truncated && f.frame.unfinished);
        assert!(f.orientation.pixels <= 64 * 48 * 48 * 8);
        for c in f.frame.candidates {
            assert_eq!(c.coverage, q);
            assert_eq!(c.work.paths, 10);
        }
        assert!(Experiment::default()
            .scan_oriented_frame(im, &[q; 65], Policy::default())
            .is_err());
    }
}

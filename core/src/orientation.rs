//! Bounded source-gradient orientation provider. It proposes sampling geometry,
//! never text, and preserves the identity of every supplied candidate.
#![forbid(unsafe_code)]
use crate::{
    experiment::{self, CandidateScanner},
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
fn envelope(quad: Quad, angle: f64) -> Quad {
    let (sin_angle, cos_angle) = angle.sin_cos();
    let mut bounds = [
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ];
    for [x, y] in quad {
        let (u, v) = (
            cos_angle * x + sin_angle * y,
            -sin_angle * x + cos_angle * y,
        );
        bounds[0] = bounds[0].min(u);
        bounds[1] = bounds[1].max(u);
        bounds[2] = bounds[2].min(v);
        bounds[3] = bounds[3].max(v);
    }
    let [l, r, t, b] = bounds;
    [[l, t], [r, t], [r, b], [l, b]]
        .map(|[u, v]| [cos_angle * u - sin_angle * v, sin_angle * u + cos_angle * v])
}
/// At most 64*48*48*8 source reads; no image allocation or rescaling.
fn directions(im: ImageView<'_>, quad: Quad, work: &mut OrientationWork) -> Vec<f64> {
    let Ok(m) = scan::transform(quad) else {
        return vec![];
    };
    let mut tensors = [[0.; 4]; 16];
    for y in 0..48 {
        for x in 0..48 {
            let Ok([px, py]) = experiment::point(
                m.0,
                0,
                (crate::numeric::usize_f64(x) + 0.5) / 48.,
                (crate::numeric::usize_f64(y) + 0.5) / 48.,
            ) else {
                continue;
            };
            if px < 2.
                || py < 2.
                || px > crate::numeric::usize_f64(im.width) - 3.
                || py > crate::numeric::usize_f64(im.height) - 3.
            {
                continue;
            }
            // Sobel derivative at original-image scale. A symmetric stencil avoids a
            // one-sided phase offset; the tensor does not depend on light/dark polarity.
            let top_left = im.gray(px - 1., py - 1.);
            let top = im.gray(px, py - 1.);
            let top_right = im.gray(px + 1., py - 1.);
            let left = im.gray(px - 1., py);
            let right = im.gray(px + 1., py);
            let bottom_left = im.gray(px - 1., py + 1.);
            let bottom = im.gray(px, py + 1.);
            let bottom_right = im.gray(px + 1., py + 1.);
            work.pixels += 8;
            let gx = top_right + 2. * right + bottom_right - top_left - 2. * left - bottom_left;
            let gy = bottom_left + 2. * bottom + bottom_right - top_left - 2. * top - top_right;
            if gx * gx + gy * gy < 1600. {
                continue;
            }
            let tensor = &mut tensors[(y / 12) * 4 + x / 12];
            tensor[0] += gx * gx;
            tensor[1] += gx * gy;
            tensor[2] += gy * gy;
            tensor[3] += 1.;
        }
    }
    let mut groups: Vec<(f64, f64, f64)> = vec![];
    for [xx, xy, yy, count] in tensors {
        work.tiles += 1;
        let energy = xx + yy;
        if count < 12. || energy <= 0. {
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
            let top_left = quad[axis];
            let top = quad[axis + 1];
            angle_distance(angle, (top[1] - top_left[1]).atan2(top[0] - top_left[0]))
                < 5f64.to_radians()
        });
        if aligned {
            continue;
        }
        if let Some(bottom) = groups.iter_mut().find(|bottom| {
            angle_distance(angle, 0.5 * bottom.1.atan2(bottom.0)) < 10f64.to_radians()
        }) {
            bottom.0 += energy * (2. * angle).cos();
            bottom.1 += energy * (2. * angle).sin();
            bottom.2 += energy;
        } else {
            groups.push((
                energy * (2. * angle).cos(),
                energy * (2. * angle).sin(),
                energy,
            ));
        }
    }
    groups.sort_by(|top_left, top| top.2.total_cmp(&top_left.2));
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
impl CandidateScanner {
    /// Keeps originals first, then round-robin orientation retries. All supplied
    /// and derived candidates receive a cheap pass before decoder retry allocation.
    /// # Errors
    /// Returns `Parameters` for invalid scan limits and propagates primary frame-scanner errors.
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
        let quad = [[10., 20.], [410., 20.], [410., 220.], [10., 220.]];
        let a = 0.47;
        let r = envelope(quad, a);
        let (sin_angle, cos_angle) = a.sin_cos();
        assert!(angle_distance((r[1][1] - r[0][1]).atan2(r[1][0] - r[0][0]), a) < 1e-12);
        let us: Vec<_> = r
            .iter()
            .map(|p| p[0] * cos_angle + p[1] * sin_angle)
            .collect();
        let vs: Vec<_> = r
            .iter()
            .map(|p| -p[0] * sin_angle + p[1] * cos_angle)
            .collect();
        for p in quad {
            let u = p[0] * cos_angle + p[1] * sin_angle;
            let v = -p[0] * sin_angle + p[1] * cos_angle;
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
                if crate::numeric::f64_i64(
                    ((crate::numeric::usize_f64(x) * a.cos()
                        + crate::numeric::usize_f64(y) * a.sin())
                        / 8.)
                        .floor(),
                ) % 2
                    == 0
                {
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
        let f = CandidateScanner::default()
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
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    assert_eq!(c.work.paths, 6);
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    assert_eq!(c.work.paths, 10);
                }
            }
        }
        assert!(CandidateScanner::default()
            .scan_oriented_frame(im, &[q; 65], Policy::default())
            .is_err());
    }
}

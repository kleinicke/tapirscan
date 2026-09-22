//! Safe reusable EAN-13 first-pass scanner for supplied cyclic quadrilaterals.
//! This is a candidate decoder, not a full-frame detector. A read does not prove
//! a broad region contains only one symbol; callers must retain its coverage.
#![forbid(unsafe_code)]
use crate::profile;
use crate::sampling::{Error, ImageView, Path, Sampler, Transform};
pub type Quad = [[f64; 2]; 4];
pub const MAX_CANDIDATES: usize = 64;
const FRACTIONS: [f64; 5] = [0.2, 0.35, 0.5, 0.65, 0.8];
#[derive(Clone, Copy, Debug)]
pub struct PathEvidence {
    pub fraction: f64,
    pub left: f64,
    pub right: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Read {
    pub digits: [u8; 13],
    pub polygon: Quad,
    pub cost: f32,
    pub gap: f32,
    pub run_width: bool,
    pub contrast_normalized: bool,
    pub axis: usize,
    pub paths: [PathEvidence; 2],
}
#[derive(Clone, Debug)]
pub struct CandidateResult {
    pub candidate_index: usize,
    pub coverage: Quad,
    pub read: Option<Read>,
    pub error: Option<Error>,
    pub paths_attempted: usize,
    pub rank: usize,
}
#[derive(Default)]
pub struct Scanner {
    sampler: Sampler,
    contrast: crate::contrast::Normalizer,
    results: Vec<CandidateResult>,
}
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
pub(crate) fn project(
    matrix: Transform,
    axis: usize,
    sample: f64,
    fraction: f64,
) -> Result<[f64; 2], Error> {
    let u = -0.15 + 1.3 * (sample + 0.5) / 512.;
    let (x, y) = if axis == 0 {
        (u, fraction)
    } else {
        (fraction, u)
    };
    let matrix = matrix.0;
    let denominator = matrix[6] * x + matrix[7] * y + matrix[8];
    if !denominator.is_finite() || denominator.abs() < 1e-9 {
        return Err(Error::Geometry);
    }
    let p = [
        (matrix[0] * x + matrix[1] * y + matrix[2]) / denominator - 0.5,
        (matrix[3] * x + matrix[4] * y + matrix[5]) / denominator - 0.5,
    ];
    if p.iter().any(|v| !v.is_finite()) {
        return Err(Error::Geometry);
    }
    Ok(p)
}
impl Scanner {
    /// Result slice is valid until the next mutable call. Every input has one
    /// output, including invalid or undecoded regions. Scratch and result capacity
    /// are reused; no input pixels or candidate geometry are mutated. Rank is a
    /// stable decoded-first order, not calibrated confidence. No cross-region
    /// text deduplication: equal values may be distinct physical instances.
    /// # Errors
    /// Returns `Parameters` for too many candidates. Individual geometry and sampling failures remain attached to their candidate results.
    pub fn scan_regions(
        &mut self,
        image: ImageView<'_>,
        candidates: &[Quad],
    ) -> Result<&[CandidateResult], Error> {
        self.scan_regions_with_contrast(image, candidates, false)
    }
    /// Optional local contrast is attempted only after all original paths fail.
    /// At most two axes x five paths per phase. Acceptance thresholds unchanged.
    /// # Errors
    /// Returns `Parameters` for too many candidates. Individual geometry and sampling failures remain attached to their candidate results.
    pub fn scan_regions_with_contrast(
        &mut self,
        image: ImageView<'_>,
        candidates: &[Quad],
        local_contrast: bool,
    ) -> Result<&[CandidateResult], Error> {
        self.scan_regions_options(image, candidates, local_contrast, false)
    }
    /// # Errors
    /// Returns `Parameters` for too many candidates. Individual geometry and sampling failures remain attached to their candidate results.
    pub fn scan_regions_with_runs(
        &mut self,
        image: ImageView<'_>,
        candidates: &[Quad],
    ) -> Result<&[CandidateResult], Error> {
        self.scan_regions_options(image, candidates, false, true)
    }
    fn scan_regions_options(
        &mut self,
        image: ImageView<'_>,
        candidates: &[Quad],
        local_contrast: bool,
        run_width: bool,
    ) -> Result<&[CandidateResult], Error> {
        self.results.clear();
        if candidates.len() > MAX_CANDIDATES {
            return Err(Error::Parameters);
        }
        self.results
            .try_reserve(candidates.len())
            .map_err(|_| Error::Allocation)?;
        for (candidate_index, &coverage) in candidates.iter().enumerate() {
            let mut result = CandidateResult {
                candidate_index,
                coverage,
                read: None,
                error: None,
                paths_attempted: 0,
                rank: 0,
            };
            match self
                .scan_one(image, coverage, &mut result.paths_attempted, false)
                .and_then(|raw| {
                    if raw.is_none() && local_contrast {
                        self.scan_one(image, coverage, &mut result.paths_attempted, true)
                    } else if raw.is_none() && run_width {
                        self.scan_runs_one(image, coverage, &mut result.paths_attempted)
                    } else {
                        Ok(raw)
                    }
                }) {
                Ok(read) => result.read = read,
                Err(error) => result.error = Some(error),
            }
            self.results.push(result);
        }
        let decoded = self.results.iter().filter(|r| r.read.is_some()).count();
        let (mut good, mut other) = (0, decoded);
        for r in &mut self.results {
            r.rank = if r.read.is_some() {
                good += 1;
                good
            } else {
                other += 1;
                other
            };
        }
        Ok(&self.results)
    }
    fn scan_runs_one(
        &mut self,
        image: ImageView<'_>,
        quad: Quad,
        attempts: &mut usize,
    ) -> Result<Option<Read>, Error> {
        let matrix = transform(quad)?;
        let mut reads = [None; 10];
        let mut text = None;
        // Gather all paths before accepting: competing values reject the region.
        for axis in 0..2 {
            for (i, fraction) in FRACTIONS.into_iter().enumerate() {
                let Some(p) = self.sampler.sample(
                    image,
                    matrix,
                    Path {
                        axis,
                        fraction,
                        curve: 0.,
                        margin: 0.15,
                    },
                )?
                else {
                    continue;
                };
                *attempts += 1;
                let Some(r) = crate::run_profile::decode(p).map_err(|_| Error::Parameters)? else {
                    continue;
                };
                reads[axis * 5 + i] = Some((
                    r,
                    PathEvidence {
                        fraction,
                        left: r.left,
                        right: r.right,
                    },
                ));
                if text.is_none() {
                    text = Some(r.digits);
                }
            }
        }
        if reads.iter().flatten().any(|(r, _)| Some(r.digits) != text) {
            return Ok(None);
        }
        for axis in 0..2 {
            for i in 0..5 {
                let Some((a, e)) = reads[axis * 5 + i] else {
                    continue;
                };
                for j in i + 1..5 {
                    let Some((b, f)) = reads[axis * 5 + j] else {
                        continue;
                    };
                    if f.fraction - e.fraction < 0.299_999
                        || e.right.min(f.right) - e.left.max(f.left)
                            <= 0.8 * (e.right - e.left).max(f.right - f.left)
                    {
                        continue;
                    }
                    let polygon = [
                        project(matrix, axis, f.left, f.fraction)?,
                        project(matrix, axis, f.right, f.fraction)?,
                        project(matrix, axis, e.right, e.fraction)?,
                        project(matrix, axis, e.left, e.fraction)?,
                    ];
                    transform(polygon)?;
                    return Ok(Some(Read {
                        digits: a.digits,
                        polygon,
                        cost: a.cost.max(b.cost),
                        gap: a.gap.min(b.gap),
                        run_width: true,
                        contrast_normalized: false,
                        axis,
                        paths: [e, f],
                    }));
                }
            }
        }
        Ok(None)
    }
    fn scan_one(
        &mut self,
        image: ImageView<'_>,
        q: Quad,
        attempts: &mut usize,
        local_contrast: bool,
    ) -> Result<Option<Read>, Error> {
        let m = transform(q)?;
        for axis in 0..2 {
            let mut previous: [Option<(profile::Read, PathEvidence)>; 5] = [None; 5];
            for (index, fraction) in FRACTIONS.into_iter().enumerate() {
                let Some(p) = self.sampler.sample(
                    image,
                    m,
                    Path {
                        axis,
                        fraction,
                        curve: 0.,
                        margin: 0.15,
                    },
                )?
                else {
                    continue;
                };
                *attempts += 1;
                let p = if local_contrast {
                    self.contrast.normalize(p).map_err(|_| Error::Parameters)?
                } else {
                    p
                };
                let Some(r) = profile::decode(p).map_err(|_| Error::Parameters)? else {
                    continue;
                };
                let (left, right) = if r.reversed {
                    (511. - f64::from(r.right), 511. - f64::from(r.left))
                } else {
                    (f64::from(r.left), f64::from(r.right))
                };
                let current = PathEvidence {
                    fraction,
                    left,
                    right,
                };
                for &(a, e) in previous[..index].iter().flatten() {
                    if a.digits != r.digits
                        || e.right.min(right) - e.left.max(left)
                            <= 0.8 * (e.right - e.left).max(right - left)
                    {
                        continue;
                    }
                    let polygon = [
                        project(m, axis, left, fraction)?,
                        project(m, axis, right, fraction)?,
                        project(m, axis, e.right, e.fraction)?,
                        project(m, axis, e.left, e.fraction)?,
                    ];
                    // Reject a degenerate consensus band before exposing a read.
                    transform(polygon)?;
                    return Ok(Some(Read {
                        run_width: false,
                        contrast_normalized: local_contrast,
                        digits: r.digits,
                        polygon,
                        cost: r.cost,
                        gap: r.gap,
                        axis,
                        paths: [e, current],
                    }));
                }
                previous[index] = Some((r, current));
            }
        }
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    const BITS:&str="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    fn fixture() -> Vec<u8> {
        let mut image = vec![255; 1000 * 180];
        for x in 0..380 {
            if BITS.as_bytes()[x / 4] == b'1' {
                for y in 20..160 {
                    image[y * 1000 + 30 + x] = 0;
                    image[y * 1000 + 530 + x] = 0;
                }
            }
        }
        image
    }
    #[test]
    fn all_regions_equal_text_geometry_and_owned_results() {
        let pixels = fixture();
        let im = ImageView::new(&pixels, 1000, 180, 1, 1000).unwrap();
        let q = [
            [[30., 20.], [410., 20.], [410., 160.], [30., 160.]],
            [[530., 20.], [910., 20.], [910., 160.], [530., 160.]],
        ];
        let mut scanner = Scanner::default();
        let results = scanner.scan_regions(im, &q).unwrap().to_vec();
        assert_eq!(results.len(), 2);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(
                r.read.unwrap().digits,
                [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]
            );
            assert_eq!(r.coverage, q[i]);
            assert_eq!(r.rank, i + 1);
            let p = r.read.unwrap().polygon;
            assert!(p
                .iter()
                .all(|p| p[0] >= q[i][0][0] - 2. && p[0] <= q[i][1][0] + 2.));
        }
        assert!(scanner.scan_regions(im, &[]).unwrap().is_empty());
        assert!(results[0].read.is_some());
    }
    #[test]
    fn invalid_sibling_does_not_hide_valid_region_and_limits_clear_results() {
        let pixels = fixture();
        let im = ImageView::new(&pixels, 1000, 180, 1, 1000).unwrap();
        let valid = [[30., 20.], [410., 20.], [410., 160.], [30., 160.]];
        let mut s = Scanner::default();
        let results = s.scan_regions(im, &[[[0.; 2]; 4], valid]).unwrap();
        assert_eq!(results[0].error, Some(Error::Geometry));
        assert!(results[1].read.is_some());
        assert_eq!(results[1].rank, 1);
        assert!(s
            .scan_regions(im, &vec![valid; MAX_CANDIDATES + 1])
            .is_err());
        assert!(s.results.is_empty());
        assert!(ImageView::new(&pixels[..100], 1000, 180, 1, 1000).is_err());
    }
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
    #[test]
    fn local_contrast_is_optional_and_raw_reads_are_unchanged() {
        let mut pixels = vec![255; 420 * 180];
        for y in 0..180 {
            for x in 0..420 {
                let dark = (30..410).contains(&x)
                    && (20..160).contains(&y)
                    && BITS.as_bytes()[(x - 30) / 4] == b'1';
                let v =
                    0.95 - 0.6 * crate::numeric::usize_f64(x) / 420. - if dark { 0.3 } else { 0. };
                pixels[y * 420 + x] = crate::numeric::f64_u8(255. * v);
            }
        }
        let im = ImageView::new(&pixels, 420, 180, 1, 420).unwrap();
        let quad = [[[30., 20.], [410., 20.], [410., 160.], [30., 160.]]];
        let mut scanner = Scanner::default();
        assert!(scanner.scan_regions(im, &quad).unwrap()[0].read.is_none());
        let r = scanner.scan_regions_with_contrast(im, &quad, true).unwrap()[0]
            .read
            .unwrap();
        assert!(r.contrast_normalized);
        assert_eq!(r.digits, [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        let p = fixture();
        let im = ImageView::new(&p, 1000, 180, 1, 1000).unwrap();
        let a = scanner.scan_regions(im, &quad).unwrap()[0].clone();
        let b = scanner.scan_regions_with_contrast(im, &quad, true).unwrap()[0].clone();
        assert!(!b.read.unwrap().contrast_normalized);
        assert_eq!(a.read.unwrap().polygon, b.read.unwrap().polygon);
        assert_eq!(a.paths_attempted, b.paths_attempted);
    }

    #[test]
    fn run_paths_require_separation_reject_conflicts_and_keep_raw() {
        let quad = [[30., 20.], [410., 20.], [410., 160.], [30., 160.]];
        let mut s = Scanner::default();
        let mut pixels = vec![255; 440 * 180];
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        for mode in 0..3 {
            pixels.fill(255);
            for y in 20..160 {
                if mode == 1 && !(69..71).contains(&y) {
                    continue;
                }
                let bits = crate::ean::encode(if mode == 2 && y >= 90 { &b } else { &a });
                for x in 0..380 {
                    if bits[x / 4] > 0.5 {
                        pixels[y * 440 + x + 30] = 0;
                    }
                }
            }
            let im = ImageView::new(&pixels, 440, 180, 1, 440).unwrap();
            let mut attempts = 0;
            let r = s.scan_runs_one(im, quad, &mut attempts).unwrap();
            assert!(attempts <= 10);
            if mode == 0 {
                let r = r.unwrap();
                assert_eq!(r.digits, a);
                assert!(r.run_width);
                assert!(r.paths[1].fraction - r.paths[0].fraction >= 0.299_999);
                let raw = s.scan_regions(im, &[quad]).unwrap()[0].clone();
                let got = s.scan_regions_with_runs(im, &[[[0.; 2]; 4], quad]).unwrap();
                assert!(got[0].error.is_some());
                assert_eq!(got[1].read.unwrap().digits, a);
                assert!(!got[1].read.unwrap().run_width);
                assert_eq!(got[1].read.unwrap().polygon, raw.read.unwrap().polygon);
            } else {
                assert!(r.is_none(), "thin or competing evidence must reject");
            }
        }
    }
}

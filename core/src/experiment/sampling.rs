//! Profile sampling and normalization. Scratch storage belongs to Experiment.
use super::{
    distance, point, scan, Error, Experiment, ImageView, Observation, Path, Quad, Timer, Work,
};

pub(super) fn interior_bounds(n: usize, lo: f64, hi: f64) -> (usize, usize) {
    let lower = |upper: bool| {
        let (mut a, mut b) = (0, n);
        while a < b {
            let mid = a + (b - a) / 2;
            let u = lo
                + (hi - lo) * (crate::numeric::usize_f64(mid) + 0.5) / crate::numeric::usize_f64(n);
            let before = if upper { u <= 1. } else { u < 0. };
            if before {
                a = mid + 1;
            } else {
                b = mid;
            }
        }
        a
    };
    (lower(false), lower(true))
}
impl Experiment {
    /// Diagnostic export uses exactly the scanner's original sampling/normalization.
    /// # Errors
    /// Returns `Path` for an invalid axis or sampling interval and `Geometry` for invalid quadrilaterals or projections; propagates sampling errors.
    pub fn diagnostic_profile(
        &mut self,
        im: ImageView<'_>,
        q: Quad,
        axis: usize,
        fraction: f64,
        native: bool,
    ) -> Result<Option<Vec<f32>>, Error> {
        if axis > 1 || !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(Error::Path);
        }
        let m = scan::transform(q)?;
        if native {
            if self.native_sample(im, m.0, axis, fraction, &mut Work::default())? {
                Ok(Some(self.signal.clone()))
            } else {
                Ok(None)
            }
        } else {
            Ok(self
                .fixed
                .sample(
                    im,
                    m,
                    Path {
                        axis,
                        fraction,
                        curve: 0.,
                        margin: 0.15,
                    },
                )?
                .map(|p| p.to_vec()))
        }
    }

    /// Reproduce an explicit straight retry window without changing scanner policy.
    /// Bounds, sample count and normalization are supplied by the diagnostic caller.
    /// # Errors
    /// Returns `Path` for an invalid axis or sampling interval and `Geometry` for invalid quadrilaterals or projections; propagates sampling errors.
    #[expect(
        clippy::too_many_arguments,
        reason = "The diagnostic API exposes independent sampling coordinates and switches used by existing experiment callers."
    )]
    pub fn diagnostic_segment(
        &mut self,
        im: ImageView<'_>,
        q: Quad,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        n: usize,
        interior: bool,
    ) -> Result<Option<Vec<f32>>, Error> {
        let m = scan::transform(q)?;
        let mut w = Work::default();
        let normal = self.sample_segment(im, m.0, axis, fraction, lo, hi, n, interior, &mut w)?;
        let accepted = if interior {
            self.normalize_interior(lo, hi, &mut w)
        } else {
            normal
        };
        Ok(accepted.then(|| self.signal.clone()))
    }

    /// Diagnostic-only independent interpretation of one supplied segment/normalization.
    /// # Errors
    /// Returns `Path` for an invalid axis or sampling interval and `Geometry` for invalid quadrilaterals or projections; propagates sampling errors.
    #[expect(
        clippy::too_many_arguments,
        reason = "The diagnostic API mirrors diagnostic_segment and adds interpretation without changing its positional contract."
    )]
    pub fn diagnostic_segment_reads(
        &mut self,
        im: ImageView<'_>,
        q: Quad,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        n: usize,
        interior: bool,
    ) -> Result<Vec<Observation>, Error> {
        let mut observations = vec![];
        if self
            .diagnostic_segment(im, q, axis, fraction, lo, hi, n, interior)?
            .is_some()
        {
            self.collect_policy(
                axis,
                fraction,
                lo,
                hi,
                &mut Work::default(),
                &mut observations,
                true,
                true,
            );
        }
        Ok(observations)
    }

    /// Diagnostic provider on the same fixed/native source coordinates.
    /// # Errors
    /// Returns `Path` for an invalid axis or sampling interval and `Geometry` for invalid quadrilaterals or projections; propagates sampling errors.
    pub fn diagnostic_interior_profile(
        &mut self,
        im: ImageView<'_>,
        q: Quad,
        axis: usize,
        fraction: f64,
        native: bool,
    ) -> Result<Option<Vec<f32>>, Error> {
        if axis > 1 || !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return Err(Error::Path);
        }
        let m = scan::transform(q)?;
        let n = if native {
            crate::numeric::f64_usize(
                distance(
                    point(m.0, axis, -0.15, fraction)?,
                    point(m.0, axis, 1.15, fraction)?,
                )
                .ceil()
                .clamp(64., 4096.),
            )
        } else {
            512
        };
        let mut w = Work::default();
        self.sample_segment(im, m.0, axis, fraction, -0.15, 1.15, n, true, &mut w)?;
        Ok(self
            .normalize_interior(-0.15, 1.15, &mut w)
            .then(|| self.signal.clone()))
    }

    pub(super) fn native_sample(
        &mut self,
        im: ImageView<'_>,
        matrix: [f64; 9],
        axis: usize,
        fraction: f64,
        work: &mut Work,
    ) -> Result<bool, Error> {
        let a = point(matrix, axis, -0.15, fraction)?;
        let b = point(matrix, axis, 1.15, fraction)?;
        let denominator = |u: f64| {
            if axis == 0 {
                matrix[6] * u + matrix[7] * fraction + matrix[8]
            } else {
                matrix[6] * fraction + matrix[7] * u + matrix[8]
            }
        };
        if denominator(-0.15) * denominator(1.15) <= 0. {
            return Err(Error::Geometry);
        }
        let requested = crate::numeric::f64_usize(distance(a, b).ceil());
        let count = requested.clamp(64, 4096);
        work.capped_paths += usize::from(requested > 4096);
        work.samples += count;
        self.signal.resize(count, 0.);
        for (i, v) in self.signal.iter_mut().enumerate() {
            let [x, y] = point(
                matrix,
                axis,
                -0.15
                    + 1.3 * (crate::numeric::usize_f64(i) + 0.5) / crate::numeric::usize_f64(count),
                fraction,
            )?;
            *v = im.bilinear(x, y);
        }
        self.sorted.clear();
        self.sorted.extend_from_slice(&self.signal);
        let (lo, hi) = crate::sampling::contrast_bounds(&mut self.sorted);
        let (lo, hi) = (f64::from(lo), f64::from(hi));
        if hi - lo < 8. {
            return Ok(false);
        }
        for v in &mut self.signal {
            *v = crate::numeric::f64_f32(((hi - f64::from(*v)) / (hi - lo)).clamp(0., 1.));
        }
        Ok(true)
    }

    /// Fine discovery may contain a small symbol occupying <5% dark pixels.
    /// Only after percentile contrast fails, retain the full observed intensity
    /// range. Structural guards, visual digit evidence and consensus still apply.
    pub(crate) fn normalize_sparse_signal(&mut self) -> bool {
        let lo = self.signal.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = self
            .signal
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        if !lo.is_finite() || !hi.is_finite() || hi - lo < 8. {
            return false;
        }
        for v in &mut self.signal {
            *v = ((hi - *v) / (hi - lo)).clamp(0., 1.);
        }
        true
    }

    /// Additional observed-intensity hypothesis. Exclude extension beyond the
    /// supplied candidate from percentile estimation, but normalize the entire
    /// sampled path so quiet zones still face the same structural checks.
    pub(crate) fn normalize_interior(&mut self, lo: f64, hi: f64, work: &mut Work) -> bool {
        if lo >= 0. && hi <= 1. {
            return false;
        }
        let n = self.raw_signal.len();
        self.sorted.clear();

        {
            let (a, b) = interior_bounds(n, lo, hi);
            self.sorted.extend_from_slice(&self.raw_signal[a..b]);
        }

        work.interior_values += self.sorted.len();
        if self.sorted.len() < 64 {
            return false;
        }
        let (lo, hi) = crate::sampling::contrast_bounds(&mut self.sorted);
        if !lo.is_finite() || !hi.is_finite() || hi - lo < 8. {
            return false;
        }
        self.signal.clear();
        self.signal.extend(
            self.raw_signal
                .iter()
                .map(|v| ((hi - v) / (hi - lo)).clamp(0., 1.)),
        );
        work.interior_paths += 1;
        true
    }

    /// Bounded original-image straight segment sampling. Uses the same bilinear
    /// grayscale and percentile normalization as the frozen fixed sampler.
    #[expect(
        clippy::too_many_arguments,
        reason = "The sampling entry point forwards explicit geometry, output size and shared accounting to the timed implementation."
    )]
    pub(crate) fn sample_segment(
        &mut self,
        im: ImageView<'_>,
        m: [f64; 9],
        axis: usize,
        f: f64,
        lo: f64,
        hi: f64,
        n: usize,
        retain_raw: bool,
        work: &mut Work,
    ) -> Result<bool, Error> {
        let timer = Timer::now();
        let result = self.sample_segment_inner(im, m, axis, f, lo, hi, n, retain_raw, work);
        #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
        {
            work.sampling_ms += timer.ms();
        }
        let _ = timer;
        result
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The sampler needs image, projective geometry, path bounds, normalization policy and mutable work accounting together."
    )]
    pub(super) fn sample_segment_inner(
        &mut self,
        im: ImageView<'_>,
        matrix: [f64; 9],
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        count: usize,
        retain_raw: bool,
        work: &mut Work,
    ) -> Result<bool, Error> {
        if !(64..=4096).contains(&count)
            || axis > 1
            || !lo.is_finite()
            || !hi.is_finite()
            || lo >= hi
            || !fraction.is_finite()
            || !(0.0..=1.0).contains(&fraction)
        {
            return Err(Error::Path);
        }
        let z = |u: f64| {
            if axis == 0 {
                matrix[6] * u + matrix[7] * fraction + matrix[8]
            } else {
                matrix[6] * fraction + matrix[7] * u + matrix[8]
            }
        };
        if !z(lo).is_finite()
            || !z(hi).is_finite()
            || z(lo) * z(hi) <= 0.
            || z(lo).abs() < 1e-9
            || z(hi).abs() < 1e-9
        {
            return Err(Error::Geometry);
        }
        work.samples += count;
        self.signal.resize(count, 0.);
        // `axis` and the cross-path fraction do not vary within one segment. Keep
        // the legacy arithmetic order, but select the coordinate layout once rather
        // than branching through `point` for each source pixel. The finite check is
        // intentionally retained at the exact point where the old helper made it.
        let nearest = (76..=384).contains(&count);
        if axis == 0 {
            for (i, v) in self.signal.iter_mut().enumerate() {
                let u = lo
                    + (hi - lo) * (crate::numeric::usize_f64(i) + 0.5)
                        / crate::numeric::usize_f64(count);
                let z = matrix[6] * u + matrix[7] * fraction + matrix[8];
                let x = (matrix[0] * u + matrix[1] * fraction + matrix[2]) / z - 0.5;
                let y = (matrix[3] * u + matrix[4] * fraction + matrix[5]) / z - 0.5;
                if !x.is_finite() || !y.is_finite() {
                    return Err(Error::Geometry);
                }
                *v = if nearest {
                    crate::numeric::f64_f32(im.gray(x.round(), y.round()))
                } else {
                    im.bilinear(x, y)
                };
            }
        } else {
            for (i, v) in self.signal.iter_mut().enumerate() {
                let u = lo
                    + (hi - lo) * (crate::numeric::usize_f64(i) + 0.5)
                        / crate::numeric::usize_f64(count);
                let z = matrix[6] * fraction + matrix[7] * u + matrix[8];
                let x = (matrix[0] * fraction + matrix[1] * u + matrix[2]) / z - 0.5;
                let y = (matrix[3] * fraction + matrix[4] * u + matrix[5]) / z - 0.5;
                if !x.is_finite() || !y.is_finite() {
                    return Err(Error::Geometry);
                }
                *v = if nearest {
                    crate::numeric::f64_f32(im.gray(x.round(), y.round()))
                } else {
                    im.bilinear(x, y)
                };
            }
        }
        if retain_raw {
            self.raw_signal.clone_from(&self.signal);
        }
        self.sorted.clear();
        self.sorted.extend_from_slice(&self.signal);
        let (lo, hi) = crate::sampling::contrast_bounds(&mut self.sorted);
        let (lo, hi) = (f64::from(lo), f64::from(hi));
        if hi - lo < 8. {
            return Ok(false);
        }
        for v in &mut self.signal {
            *v = crate::numeric::f64_f32(((hi - f64::from(*v)) / (hi - lo)).clamp(0., 1.));
        }
        Ok(true)
    }
}

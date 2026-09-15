//! Isolated supplied-region experiment. No reference decoders or label inputs.
//! One controlled signal path feeds either unchanged profile or run likelihood.
use crate::scanner_clock::Timer;
use crate::{
    ean, profile, run_ean,
    sampling::{Error, ImageView, Path, Sampler},
    scan::{self, Quad},
};
#[derive(Clone, Copy, Debug)]
pub struct Observation {
    pub short_quiet: bool,
    pub ambiguous: bool,
    pub digits: [u8; 13],
    pub axis: usize,
    pub fraction: f64,
    pub left: f64,
    pub right: f64,
    pub cost: f32,
    pub gap: f32,
}
#[derive(Default, Debug, Clone)]
pub struct Work {
    pub invalid_visual_seen: usize,
    pub invalid_veto_intervals: usize,
    pub invalid_veto_reads: usize,
    pub invalid_soft_conflicts: usize,
    pub invalid_veto_capped: usize,
    #[cfg(feature = "experimental-extrema-runs")]
    pub extrema_calls: usize,
    pub extrema_examined: usize,
    pub extrema_capped: usize,
    pub extrema_ambiguous: usize,
    pub extrema_decoder_calls: usize,
    #[cfg(feature = "experimental-redundant-decode")]
    pub redundant_decode_calls_avoided: usize,
    #[cfg(feature = "experimental-structural-retry")]
    pub max_run_count: usize,
    #[cfg(feature = "experimental-structural-retry")]
    pub structural_retry_skipped: usize,
    #[cfg(feature = "experimental-structural-retry")]
    pub(crate) structural_discovery_complete: bool,
    #[cfg(feature = "experimental-gap-density")]
    pub continuity_cache_hits: usize,
    #[cfg(feature = "experimental-forward-blur")]
    pub forward_blur_calls: usize,
    #[cfg(feature = "experimental-forward-blur")]
    pub forward_blur_windows: usize,
    #[cfg(feature = "experimental-forward-blur")]
    pub forward_blur_model_attempts: usize,
    #[cfg(feature = "experimental-forward-blur")]
    pub forward_blur_accepted_windows: usize,
    #[cfg(feature = "experimental-forward-blur")]
    pub forward_blur_conflicts: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub extension_cache_hits: usize,
    pub extension_samples: usize,
    pub extension_claims: usize,
    pub extension_capped: usize,
    pub reuse_claims: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_claims_rejected: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_checks: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_checks_capped: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_paths_changed: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_paths_removed: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_paths_split: usize,
    #[cfg(feature = "experimental-verified-coverage-reuse")]
    pub reuse_short_pieces: usize,
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    pub sampling_ms: f64,
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    pub interpretation_ms: f64,
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    pub support_ms: f64,
    pub paths: usize,
    pub samples: usize,
    pub low_contrast: usize,
    pub windows: usize,
    pub quiet_pass: usize,
    pub guard_pass: usize,
    pub decoder_calls: usize,
    pub accepted_paths: usize,
    pub conflicts: usize,
    pub continuity_samples: usize,
    pub continuity_rejects: usize,
    pub capped_paths: usize,
    pub profile_boundary_pairs: usize,
    pub profile_digit_hypotheses: usize,
    pub truncated_paths: usize,
    pub retry_paths: usize,
    pub retry_paths_pending: usize,
    pub sampling_plan_capped: usize,
    pub discovery_paths: usize,
    pub scale_hint_used: usize,
    pub unresolved_probe_paths: usize,
    pub sparse_normalizations: usize,
    pub association_checks: usize,
    pub association_truncated: usize,
    pub continuity_capped_links: usize,
    pub retained_initial_detections: usize,
    pub cleanup_paths: usize,
    pub cleanup_examined: usize,
    pub cleanup_removed_runs: usize,
    pub cleanup_pixels: usize,
    pub interior_paths: usize,
    pub interior_values: usize,
    pub bias_paths: usize,
    pub bias_model_pass: usize,
    pub bias_guard_pass: usize,
}
/// Shared frame budget covers preprocessing comparisons, track links and source
/// continuity pixels across initial and retry assembly. Zero is a valid budget.
#[derive(Clone, Copy, Debug)]
pub struct AssociationBudget {
    pub checks_left: usize,
    pub pixels_left: usize,
}
impl Default for AssociationBudget {
    fn default() -> Self {
        Self {
            checks_left: 200_000,
            pixels_left: 2_000_000,
        }
    }
}
impl AssociationBudget {
    pub(crate) fn check(&mut self, work: &mut Work) -> bool {
        if self.checks_left == 0 {
            work.association_truncated = 1;
            return false;
        }
        self.checks_left -= 1;
        work.association_checks += 1;
        true
    }
    pub(crate) fn pixels(&mut self, n: usize, work: &mut Work) -> bool {
        if self.pixels_left < n {
            work.association_truncated = 1;
            return false;
        }
        self.pixels_left -= n;
        true
    }
}
#[derive(Clone, Debug)]
pub struct Detection {
    pub digits: [u8; 13],
    pub polygon: Quad,
    pub support: usize,
    pub axis: usize,
}
#[derive(Debug)]
pub struct Candidate {
    pub index: usize,
    pub coverage: Quad,
    pub observations: Vec<Observation>,
    pub detections: Vec<Detection>,
    pub work: Work,
    pub ms: f64,
    pub error: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub name: &'static str,
    native: bool,
    dense: bool,
    decoder: DecoderMode,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoderMode {
    Profile,
    Runs,
    Combined,
    Many,
}
impl Config {
    pub fn new(
        name: &'static str,
        native: bool,
        dense: bool,
        decoder: DecoderMode,
    ) -> Result<Self, Error> {
        if native && matches!(decoder, DecoderMode::Profile | DecoderMode::Combined) {
            return Err(Error::Parameters);
        }
        Ok(Self {
            name,
            native,
            dense,
            decoder,
        })
    }
}
pub const MULTI_FIXED: Config = Config {
    name: "multi_fixed5",
    native: false,
    dense: false,
    decoder: DecoderMode::Many,
};
pub const CONFIGS: [Config; 5] = [
    Config {
        name: "profile_fixed5",
        native: false,
        dense: false,
        decoder: DecoderMode::Profile,
    },
    Config {
        name: "runs_fixed5",
        native: false,
        dense: false,
        decoder: DecoderMode::Runs,
    },
    Config {
        name: "runs_native5",
        native: true,
        dense: false,
        decoder: DecoderMode::Runs,
    },
    Config {
        name: "runs_native21",
        native: true,
        dense: true,
        decoder: DecoderMode::Runs,
    },
    Config {
        name: "combined_fixed5",
        native: false,
        dense: false,
        decoder: DecoderMode::Combined,
    },
];
// Exact contiguous selection using the same coordinate expression as the legacy scan.
#[cfg(any(feature = "experimental-interior-bounds", test))]
fn interior_bounds(n: usize, lo: f64, hi: f64) -> (usize, usize) {
    let lower = |upper: bool| {
        let (mut a, mut b) = (0, n);
        while a < b {
            let mid = a + (b - a) / 2;
            let u = lo + (hi - lo) * (mid as f64 + 0.5) / n as f64;
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
pub(crate) fn point(m: [f64; 9], axis: usize, u: f64, f: f64) -> Result<[f64; 2], Error> {
    let (x, y) = if axis == 0 { (u, f) } else { (f, u) };
    let z = m[6] * x + m[7] * y + m[8];
    if !z.is_finite() || z.abs() < 1e-9 {
        return Err(Error::Geometry);
    }
    let p = [
        (m[0] * x + m[1] * y + m[2]) / z - 0.5,
        (m[3] * x + m[4] * y + m[5]) / z - 0.5,
    ];
    if p.iter().any(|v| !v.is_finite()) {
        return Err(Error::Geometry);
    }
    Ok(p)
}
pub(crate) fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
#[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
fn trace_reads(stage: &str, r: &crate::multi_profile::Reads) {
    eprintln!(
        "{{\"stage\":\"{}\",\"ambiguous\":{},\"truncated\":{}}}",
        stage, r.ambiguous_intervals, r.truncated
    );
    for s in &r.symbols {
        eprintln!(
            "{{\"stage\":\"{}\",\"digits\":{:?},\"left\":{},\"right\":{},\"cost\":{},\"gap\":{}}}",
            stage, s.digits, s.left, s.right, s.cost, s.gap
        );
    }
    for (left, right) in &r.rejected_intervals {
        eprintln!(
            "{{\"stage\":\"{}\",\"rejectedLeft\":{},\"rejectedRight\":{}}}",
            stage, left, right
        );
    }
}
#[derive(Default)]
pub struct Experiment {
    #[cfg(feature = "experimental-forward-blur")]
    blur_rejected_intervals: Vec<(f64, f64)>,
    fixed: Sampler,
    signal: Vec<f32>,
    raw_signal: Vec<f32>,
    sorted: Vec<f32>,
    runs: Vec<(usize, usize, bool)>,
    #[cfg(feature = "experimental-local-contrast")]
    local_scratch: crate::local_signal::Scratch,
}
impl Experiment {
    /// Diagnostic export uses exactly the scanner's original sampling/normalization.
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
            distance(
                point(m.0, axis, -0.15, fraction)?,
                point(m.0, axis, 1.15, fraction)?,
            )
            .ceil()
            .clamp(64., 4096.) as usize
        } else {
            512
        };
        let mut w = Work::default();
        self.sample_segment(im, m.0, axis, fraction, -0.15, 1.15, n, true, &mut w)?;
        Ok(self
            .normalize_interior(-0.15, 1.15, &mut w)
            .then(|| self.signal.clone()))
    }
    fn native_sample(
        &mut self,
        im: ImageView<'_>,
        m: [f64; 9],
        axis: usize,
        f: f64,
        work: &mut Work,
    ) -> Result<bool, Error> {
        let a = point(m, axis, -0.15, f)?;
        let b = point(m, axis, 1.15, f)?;
        let z = |u: f64| {
            if axis == 0 {
                m[6] * u + m[7] * f + m[8]
            } else {
                m[6] * f + m[7] * u + m[8]
            }
        };
        if z(-0.15) * z(1.15) <= 0. {
            return Err(Error::Geometry);
        }
        let requested = distance(a, b).ceil() as usize;
        let n = requested.clamp(64, 4096);
        work.capped_paths += usize::from(requested > 4096);
        work.samples += n;
        self.signal.resize(n, 0.);
        for (i, v) in self.signal.iter_mut().enumerate() {
            let [x, y] = point(m, axis, -0.15 + 1.3 * (i as f64 + 0.5) / n as f64, f)?;
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
            *v = ((hi - f64::from(*v)) / (hi - lo)).clamp(0., 1.) as f32;
        }
        Ok(true)
    }
    fn run_decode(&mut self, work: &mut Work) -> Option<(crate::run_profile::Read, bool)> {
        let p = &self.signal;
        let n = p.len();
        self.runs.clear();
        let (mut start, mut black) = (0, p[0] >= 0.5);
        for i in 1..=n {
            let next = i < n && p[i] >= 0.5;
            if i == n || next != black {
                self.runs.push((start, i, black));
                start = i;
                black = next;
            }
        }
        let mut accepted: Option<(crate::run_profile::Read, bool)> = None;
        for i in 0..self.runs.len().saturating_sub(60) {
            let r = &self.runs[i..i + 61];
            if !r[1].2 {
                continue;
            }
            work.windows += 1;
            let (left, right) = (r[1].0, r[59].1);
            let module = (right - left) as f64 / 95.;
            if module < 0.8
                || ((r[0].1 - r[0].0) as f64) < 7. * module
                || ((r[60].1 - r[60].0) as f64) < 7. * module
            {
                continue;
            }
            work.quiet_pass += 1;
            let mut widths = [0.; 59];
            for j in 0..59 {
                widths[j] = (r[j + 1].1 - r[j + 1].0) as f32;
            }
            if [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58]
                .iter()
                .all(|&j| (widths[j] / module as f32 - 1.).abs() <= 0.65)
            {
                work.guard_pass += 1;
            }
            for reversed in [false, true] {
                if reversed {
                    widths.reverse();
                }
                work.decoder_calls += 1;
                let Some(e) = run_ean::decode_evidence(&widths) else {
                    continue;
                };
                let r = crate::run_profile::Read {
                    digits: e.digits,
                    left: left as f64 - 0.5,
                    right: right as f64 - 0.5,
                    cost: e.cost,
                    gap: e.gap,
                };
                if accepted.is_some_and(|(a, _)| a.digits != r.digits) {
                    work.conflicts += 1;
                    return None;
                }
                if accepted.is_none_or(|(a, _)| r.cost < a.cost) {
                    accepted = Some((r, reversed));
                }
            }
        }
        accepted
    }
    fn profile_decode(&mut self, work: &mut Work) -> Option<crate::run_profile::Read> {
        #[cfg(feature = "experimental-forward-blur")]
        self.blur_rejected_intervals.clear();
        if self.signal.len() != 512
            && !(cfg!(feature = "experimental-native-soft")
                && (76..=384).contains(&self.signal.len()))
        {
            return None;
        }
        // Count actual boundary pairs and digit hypotheses used by unchanged profile.
        // Forward-blur records the pairs in profile::BlurTrace, including the native
        // variable-length path; do not retain the previous fixed-512 duplicate scan.
        #[cfg(not(feature = "experimental-forward-blur"))]
        for rev in [false, true] {
            let p = &self.signal;
            let n = p.len();
            let v = |i: usize| p[if rev { n - 1 - i } else { i }];
            let (mut ns, mut ne) = (0usize, 0usize);
            for i in 1..n {
                if v(i - 1) < 0.5 && v(i) >= 0.5 && (i as f32) < n as f32 * 0.35 {
                    ns = (ns + 1).min(10)
                }
                if v(i - 1) >= 0.5 && v(i) < 0.5 && (i as f32) > n as f32 * 0.65 {
                    ne = (ne + 1).min(10)
                }
            }
            work.profile_boundary_pairs += ns * ne;
            work.profile_digit_hypotheses += (ns * ne).min(4);
        }
        #[cfg(not(feature = "experimental-forward-blur"))]
        let result = profile::decode(&self.signal);
        #[cfg(feature = "experimental-forward-blur")]
        let result = {
            let (result, trace) = {
                #[cfg(feature = "experimental-native-soft")]
                if self.signal.len() != 512 {
                    profile::decode_native_with_blur_trace(&self.signal)
                } else {
                    profile::decode_with_blur_trace(&self.signal)
                }
                #[cfg(not(feature = "experimental-native-soft"))]
                profile::decode_with_blur_trace(&self.signal)
            };
            work.profile_boundary_pairs += trace.boundary_pairs;
            work.profile_digit_hypotheses += trace.digit_hypotheses;
            work.forward_blur_calls += 1;
            work.forward_blur_windows += trace.gated_windows;
            work.forward_blur_model_attempts += trace.model_attempts;
            work.forward_blur_accepted_windows += trace.accepted_windows;
            work.forward_blur_conflicts += trace.conflicts;
            work.conflicts += trace.conflicts;
            self.blur_rejected_intervals.extend(
                trace
                    .rejected_intervals
                    .iter()
                    .map(|&(a, b)| (f64::from(a), f64::from(b))),
            );
            result
        };
        result.ok().flatten().map(|r| {
            let (left, right) = if r.reversed {
                (
                    (self.signal.len() - 1) as f64 - f64::from(r.right),
                    (self.signal.len() - 1) as f64 - f64::from(r.left),
                )
            } else {
                (f64::from(r.left), f64::from(r.right))
            };
            crate::run_profile::Read {
                digits: r.digits,
                left,
                right,
                cost: r.cost,
                gap: r.gap,
            }
        })
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
        #[cfg(feature = "experimental-interior-bounds")]
        {
            let (a, b) = interior_bounds(n, lo, hi);
            self.sorted.extend_from_slice(&self.raw_signal[a..b]);
        }
        #[cfg(not(feature = "experimental-interior-bounds"))]
        for (i, &v) in self.raw_signal.iter().enumerate() {
            let u = lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
            if (0.0..=1.0).contains(&u) {
                self.sorted.push(v)
            }
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
    #[cfg(feature = "experimental-native-sharpen")]
    pub(crate) fn sharpen_native_profile(&mut self, normalized: bool) -> bool {
        let n = self.signal.len();
        if !normalized
            || !(76..=384).contains(&n)
            || self
                .signal
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return false;
        }
        self.sorted.clone_from(&self.signal);
        for i in 1..n - 1 {
            self.signal[i] = (1.5 * self.sorted[i]
                - 0.25 * (self.sorted[i - 1] + self.sorted[i + 1]))
                .clamp(0., 1.);
        }
        true
    }
    pub(crate) fn collect_many(
        &mut self,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        work: &mut Work,
        observations: &mut Vec<Observation>,
    ) {
        self.collect_policy(axis, fraction, lo, hi, work, observations, false, false);
    }
    pub(crate) fn collect_policy(
        &mut self,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        work: &mut Work,
        observations: &mut Vec<Observation>,
        cleanup: bool,
        guard_bias: bool,
    ) {
        let timer = Timer::now();
        self.collect_policy_inner(
            axis,
            fraction,
            lo,
            hi,
            work,
            observations,
            cleanup,
            guard_bias,
        );
        #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
        {
            work.interpretation_ms += timer.ms();
        }
        let _ = timer;
        ();
    }
    fn collect_policy_inner(
        &mut self,
        axis: usize,
        fraction: f64,
        lo: f64,
        hi: f64,
        work: &mut Work,
        observations: &mut Vec<Observation>,
        cleanup: bool,
        guard_bias: bool,
    ) {
        #[cfg(feature = "experimental-invalid-visual-veto")]
        let (start, mut run_visual, mut visual_capped, mut soft_reads) =
            (observations.len(), Vec::new(), false, Vec::new());
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        eprintln!("{{\"collect\":true,\"axis\":{},\"fraction\":{},\"lo\":{},\"hi\":{},\"samples\":{},\"signal\":{:?}}}",axis,fraction,lo,hi,self.signal.len(),self.signal);
        crate::multi_profile::sample_runs(&self.signal, 64, &mut self.runs)
            .expect("validated sampled profile");
        #[cfg(feature = "experimental-structural-retry")]
        if !work.structural_discovery_complete {
            work.max_run_count = work.max_run_count.max(self.runs.len());
        }
        #[cfg(not(feature = "experimental-short-quiet"))]
        let short_accepted = false;
        #[cfg(feature = "experimental-short-quiet")]
        let mut short_accepted;
        #[cfg(feature = "experimental-short-quiet")]
        {
            // Raw observed runs only: no cleanup or synthesized transitions. Keep
            // weaker quiet-zone evidence marked until independent row assembly.
            let short = crate::multi_profile::decode_short_quiet(&self.runs, 64, guard_bias);
            #[cfg(feature = "experimental-invalid-visual-veto")]
            crate::invalid_visual::collect(&mut run_visual, &mut visual_capped, &short);
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("short", &short);
            short_accepted = !short.symbols.is_empty();
            work.bias_model_pass += short.bias_model_pass;
            work.bias_guard_pass += short.bias_guard_pass;
            work.windows += short.windows_examined;
            work.quiet_pass += short.quiet_pass;
            work.guard_pass += short.guard_pass;
            work.decoder_calls += short.decoder_calls;
            work.conflicts += short.ambiguous_intervals;
            work.truncated_paths += usize::from(short.truncated);
            let n = self.signal.len() as f64;
            for (left, right) in short.rejected_intervals {
                observations.push(Observation {
                    short_quiet: true,
                    ambiguous: true,
                    digits: [0; 13],
                    axis,
                    fraction,
                    left: lo + (hi - lo) * (left + 0.5) / n,
                    right: lo + (hi - lo) * (right + 0.5) / n,
                    cost: 0.,
                    gap: 0.,
                });
            }
            for r in short.symbols {
                observations.push(Observation {
                    short_quiet: true,
                    ambiguous: false,
                    digits: r.digits,
                    axis,
                    fraction,
                    left: lo + (hi - lo) * (r.left + 0.5) / n,
                    right: lo + (hi - lo) * (r.right + 0.5) / n,
                    cost: r.cost,
                    gap: r.gap,
                });
            }
        }
        let raw = crate::multi_profile::decode_runs(&self.runs, 64);
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        trace_reads("raw", &raw);
        let mut reads = if cleanup {
            let (clean, cw) =
                crate::transition::decode_clustered_reusing(&self.signal, 64, &raw, &self.runs);
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("cleanup", &clean);
            work.cleanup_paths += 1;
            work.cleanup_examined += cw.examined;
            work.cleanup_removed_runs += cw.removed_runs;
            work.cleanup_pixels += cw.removed_pixels;
            crate::transition::merge_reads(raw, clean, 64)
        } else {
            raw
        };
        if guard_bias {
            work.bias_paths += 1;
            let bias = crate::multi_profile::decode_guard_runs(&self.runs, 64);
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("guard", &bias);
            reads = crate::transition::merge_reads(reads, bias, 64);
        }
        #[cfg(feature = "experimental-local-contrast")]
        if cleanup {
            let local = crate::local_signal::decode_reusing_validated(
                &self.signal,
                64,
                guard_bias,
                &mut self.local_scratch,
            )
            .expect("validated sampled profile");
            #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
            trace_reads("local", &local);
            #[cfg(feature = "experimental-short-quiet")]
            {
                let short =
                    crate::local_signal::prepared_short(&self.local_scratch, 64, guard_bias);
                #[cfg(feature = "experimental-invalid-visual-veto")]
                crate::invalid_visual::collect(&mut run_visual, &mut visual_capped, &short);
                short_accepted |= !short.symbols.is_empty();
                work.bias_model_pass += short.bias_model_pass;
                work.bias_guard_pass += short.bias_guard_pass;
                work.windows += short.windows_examined;
                work.quiet_pass += short.quiet_pass;
                work.guard_pass += short.guard_pass;
                work.decoder_calls += short.decoder_calls;
                work.conflicts += short.ambiguous_intervals;
                work.truncated_paths += usize::from(short.truncated);
                let n = self.signal.len() as f64;
                for (left, right) in short.rejected_intervals {
                    observations.push(Observation {
                        short_quiet: true,
                        ambiguous: true,
                        digits: [0; 13],
                        axis,
                        fraction,
                        left: lo + (hi - lo) * (left + 0.5) / n,
                        right: lo + (hi - lo) * (right + 0.5) / n,
                        cost: 0.,
                        gap: 0.,
                    });
                }
                for r in short.symbols {
                    observations.push(Observation {
                        short_quiet: true,
                        ambiguous: false,
                        digits: r.digits,
                        axis,
                        fraction,
                        left: lo + (hi - lo) * (r.left + 0.5) / n,
                        right: lo + (hi - lo) * (r.right + 0.5) / n,
                        cost: r.cost,
                        gap: r.gap,
                    });
                }
            }
            #[cfg(feature = "experimental-redundant-decode")]
            {
                work.redundant_decode_calls_avoided +=
                    crate::local_signal::reused_calls(&self.local_scratch);
            }
            #[cfg(feature = "experimental-extrema-runs")]
            {
                let e = &self.local_scratch.extrema;
                work.extrema_calls += usize::from(e.attempted);
                work.extrema_examined += e.examined;
                work.extrema_capped += usize::from(e.capped);
                work.extrema_ambiguous += usize::from(e.ambiguous);
                work.extrema_decoder_calls += e.decoder_calls;
            }
            reads = crate::transition::merge_reads(reads, local, 64);
        }
        // Under-resolved source profiles can lose narrow dark/bright elements
        // at the middle threshold. Two fixed photometric hypotheses use observed
        // pixels only; their competing values still veto overlapping evidence.
        if cleanup && (76..=384).contains(&self.signal.len()) && (45..61).contains(&self.runs.len())
        {
            for threshold in [0.35f32, 0.65] {
                let shifted: Vec<_> = self
                    .signal
                    .iter()
                    .map(|v| (v + 0.5 - threshold).clamp(0., 1.))
                    .collect();
                let raw =
                    crate::multi_profile::decode_many(&shifted, 64).expect("valid shifted profile");
                let extra = if guard_bias {
                    crate::transition::merge_reads(
                        raw,
                        crate::multi_profile::decode_guard_bias(&shifted, 64)
                            .expect("valid shifted profile"),
                        64,
                    )
                } else {
                    raw
                };
                reads = crate::transition::merge_reads(reads, extra, 64);
            }
        }
        #[cfg(feature = "experimental-invalid-visual-veto")]
        crate::invalid_visual::collect(&mut run_visual, &mut visual_capped, &reads);
        work.bias_model_pass += reads.bias_model_pass;
        work.bias_guard_pass += reads.bias_guard_pass;
        work.windows += reads.windows_examined;
        work.quiet_pass += reads.quiet_pass;
        work.guard_pass += reads.guard_pass;
        work.decoder_calls += reads.decoder_calls;
        work.conflicts += reads.ambiguous_intervals;
        work.truncated_paths += usize::from(reads.truncated);
        // Complementary global evidence is valid only on fixed512 signals. A run
        // ambiguity must not be resurrected through the single-result fallback.
        if (self.signal.len() == 512
            || (cfg!(feature = "experimental-native-soft")
                && (76..=384).contains(&self.signal.len())))
            && reads.ambiguous_intervals == 0
        {
            if let Some(p) = self.profile_decode(work) {
                #[cfg(feature = "experimental-invalid-visual-veto")]
                soft_reads.push(p);
                let overlaps = |r: &crate::run_profile::Read| r.left < p.right && p.left < r.right;
                if reads
                    .symbols
                    .iter()
                    .any(|r| overlaps(r) && r.digits != p.digits)
                {
                    work.conflicts += 1;
                    reads.rejected_intervals.push((p.left, p.right));
                    reads.symbols.retain(|r| !overlaps(r));
                } else if !reads
                    .symbols
                    .iter()
                    .any(|r| overlaps(r) && r.digits == p.digits)
                {
                    reads.symbols.push(p);
                }
            }
        }
        #[cfg(feature = "experimental-forward-blur")]
        if (self.signal.len() == 512
            || (cfg!(feature = "experimental-native-soft")
                && (76..=384).contains(&self.signal.len())))
            && reads.ambiguous_intervals == 0
        {
            // A same-window legacy/blur contradiction must not be resurrected by
            // Many's retained run reads. Preserve disjoint source intervals.
            for &(left, right) in &self.blur_rejected_intervals {
                reads.rejected_intervals.push((left, right));
                reads.symbols.retain(|r| r.right <= left || r.left >= right);
            }
        }
        #[cfg(all(feature = "diagnostic-retry-trace", not(target_arch = "wasm32")))]
        trace_reads("merged", &reads);
        #[cfg(not(feature = "experimental-invalid-visual-veto"))]
        {
            work.accepted_paths += usize::from(short_accepted || !reads.symbols.is_empty());
        }
        for (left, right) in reads.rejected_intervals {
            let n = self.signal.len() as f64;
            observations.push(Observation {
                short_quiet: false,
                ambiguous: true,
                digits: [0; 13],
                axis,
                fraction,
                left: lo + (hi - lo) * (left + 0.5) / n,
                right: lo + (hi - lo) * (right + 0.5) / n,
                cost: 0.,
                gap: 0.,
            });
        }
        for r in reads.symbols {
            let n = self.signal.len() as f64;
            observations.push(Observation {
                short_quiet: false,
                ambiguous: false,
                digits: r.digits,
                axis,
                fraction,
                left: lo + (hi - lo) * (r.left + 0.5) / n,
                right: lo + (hi - lo) * (r.right + 0.5) / n,
                cost: r.cost,
                gap: r.gap,
            });
        }
        #[cfg(feature = "experimental-invalid-visual-veto")]
        {
            let _ = short_accepted;
            crate::invalid_visual::apply(
                &run_visual,
                visual_capped,
                &soft_reads,
                observations,
                start,
                axis,
                fraction,
                lo,
                hi,
                self.signal.len(),
                work,
            );
            work.accepted_paths += usize::from(observations[start..].iter().any(|o| !o.ambiguous));
        }
    }
    /// Bounded original-image straight segment sampling. Uses the same bilinear
    /// grayscale and percentile normalization as the frozen fixed sampler.
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
    fn sample_segment_inner(
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
        if !(64..=4096).contains(&n)
            || axis > 1
            || !lo.is_finite()
            || !hi.is_finite()
            || lo >= hi
            || !f.is_finite()
            || !(0.0..=1.0).contains(&f)
        {
            return Err(Error::Path);
        }
        let z = |u: f64| {
            if axis == 0 {
                m[6] * u + m[7] * f + m[8]
            } else {
                m[6] * f + m[7] * u + m[8]
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
        work.samples += n;
        self.signal.resize(n, 0.);
        // `axis` and the cross-path fraction do not vary within one segment. Keep
        // the legacy arithmetic order, but select the coordinate layout once rather
        // than branching through `point` for each source pixel. The finite check is
        // intentionally retained at the exact point where the old helper made it.
        let nearest = cfg!(feature = "experimental-nearest-lowres") && (76..=384).contains(&n);
        if axis == 0 {
            for (i, v) in self.signal.iter_mut().enumerate() {
                let u = lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
                let z = m[6] * u + m[7] * f + m[8];
                let x = (m[0] * u + m[1] * f + m[2]) / z - 0.5;
                let y = (m[3] * u + m[4] * f + m[5]) / z - 0.5;
                if !x.is_finite() || !y.is_finite() {
                    return Err(Error::Geometry);
                }
                *v = if nearest {
                    im.gray(x.round(), y.round()) as f32
                } else {
                    im.bilinear(x, y)
                };
            }
        } else {
            for (i, v) in self.signal.iter_mut().enumerate() {
                let u = lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
                let z = m[6] * f + m[7] * u + m[8];
                let x = (m[0] * f + m[1] * u + m[2]) / z - 0.5;
                let y = (m[3] * f + m[4] * u + m[5]) / z - 0.5;
                if !x.is_finite() || !y.is_finite() {
                    return Err(Error::Geometry);
                }
                *v = if nearest {
                    im.gray(x.round(), y.round()) as f32
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
            *v = ((hi - f64::from(*v)) / (hi - lo)).clamp(0., 1.) as f32;
        }
        Ok(true)
    }
    pub fn scan(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        config: Config,
    ) -> Vec<Candidate> {
        self.scan_with_budget(im, candidates, config, &mut AssociationBudget::default())
    }
    pub(crate) fn scan_with_budget(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        config: Config,
        budget: &mut AssociationBudget,
    ) -> Vec<Candidate> {
        candidates
            .iter()
            .enumerate()
            .map(|(index, &coverage)| {
                let start = Timer::now();
                let mut out = Candidate {
                    index,
                    coverage,
                    observations: vec![],
                    detections: vec![],
                    work: Work::default(),
                    ms: 0.,
                    error: false,
                };
                let m = match scan::transform(coverage) {
                    Ok(m) => m,
                    Err(_) => {
                        out.error = true;
                        return out;
                    }
                };
                for axis in 0..2 {
                    let fractions: Vec<f64> = if config.dense {
                        (0..21).map(|i| 0.2 + 0.03 * f64::from(i)).collect()
                    } else {
                        vec![0.2, 0.35, 0.5, 0.65, 0.8]
                    };
                    for fraction in fractions {
                        out.work.paths += 1;
                        let sample_timer = Timer::now();
                        let sampled = if config.native {
                            self.native_sample(im, m.0, axis, fraction, &mut out.work)
                        } else {
                            out.work.samples += 512;
                            self.fixed
                                .sample(
                                    im,
                                    m,
                                    Path {
                                        axis,
                                        fraction,
                                        curve: 0.,
                                        margin: 0.15,
                                    },
                                )
                                .map(|p| {
                                    if let Some(p) = p {
                                        self.signal.clear();
                                        self.signal.extend_from_slice(p);
                                        true
                                    } else {
                                        false
                                    }
                                })
                        };
                        #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
                        {
                            if !config.native {
                                out.work.sampling_ms += sample_timer.ms();
                            }
                        }
                        let _ = sample_timer;
                        match sampled {
                            Ok(true) => {}
                            Ok(false) => {
                                out.work.low_contrast += 1;
                                continue;
                            }
                            Err(_) => {
                                out.error = true;
                                continue;
                            }
                        }
                        if config.decoder == DecoderMode::Many {
                            self.collect_many(
                                axis,
                                fraction,
                                -0.15,
                                1.15,
                                &mut out.work,
                                &mut out.observations,
                            );
                            continue;
                        }
                        let r = if config.decoder == DecoderMode::Profile {
                            self.profile_decode(&mut out.work)
                        } else if config.decoder == DecoderMode::Runs {
                            self.run_decode(&mut out.work).map(|(r, _)| r)
                        } else {
                            let a = self.profile_decode(&mut out.work);
                            let b = self.run_decode(&mut out.work).map(|(r, _)| r);
                            match (a, b) {
                                (Some(a), Some(b)) if a.digits != b.digits => {
                                    out.work.conflicts += 1;
                                    None
                                }
                                (a, b) => a.or(b),
                            }
                        };
                        if let Some(r) = r {
                            out.work.accepted_paths += 1;
                            let n = self.signal.len() as f64;
                            out.observations.push(Observation {
                                short_quiet: false,
                                ambiguous: false,
                                digits: r.digits,
                                axis,
                                fraction,
                                left: -0.15 + 1.3 * (r.left + 0.5) / n,
                                right: -0.15 + 1.3 * (r.right + 0.5) / n,
                                cost: r.cost,
                                gap: r.gap,
                            });
                        }
                    }
                }
                out.detections = if config.decoder == DecoderMode::Many {
                    assemble_many_budget(im, m.0, &out.observations, &mut out.work, false, budget)
                } else {
                    assemble(im, m.0, &out.observations, &mut out.work)
                };
                out.ms = start.ms();
                out
            })
            .collect()
    }
}
// Conservative blank-gap veto, common to all controlled configurations. Every
// <=0.5 source-pixel cross-band step probes32 nearest-neighbor module positions.
// This catches blank separation; it is not a general instance-association proof.
fn connected(
    im: ImageView<'_>,
    m: [f64; 9],
    a: Observation,
    b: Observation,
    work: &mut Work,
) -> bool {
    connected_budget(
        im,
        m,
        a,
        b,
        work,
        None,
        #[cfg(feature = "experimental-gap-density")]
        &mut std::collections::HashMap::new(),
    )
}
fn connected_budget(
    im: ImageView<'_>,
    m: [f64; 9],
    a: Observation,
    b: Observation,
    work: &mut Work,
    budget: Option<&mut AssociationBudget>,
    #[cfg(feature = "experimental-gap-density")] cache: &mut std::collections::HashMap<
        [u64; 5],
        Option<(usize, usize)>,
    >,
) -> bool {
    #[cfg(feature = "experimental-gap-density")]
    {
        return connected_density(im, m, a, b, work, budget, cache);
    }

    #[cfg(not(feature = "experimental-gap-density"))]
    {
        let mut budget = budget;
        let u = (a.left + a.right + b.left + b.right) / 4.;
        let pa = point(m, a.axis, u, a.fraction).unwrap();
        let pb = point(m, a.axis, u, b.fraction).unwrap();
        let steps = (distance(pa, pb) * 2.).ceil() as usize;
        if steps > 4096 {
            work.continuity_capped_links += 1;
            work.association_truncated = 1;
            return false;
        }
        for i in 0..=steps {
            if let Some(budget) = budget.as_deref_mut() {
                if !budget.pixels(32, work) {
                    return false;
                }
            }
            let t = i as f64 / steps.max(1) as f64;
            let f = a.fraction + (b.fraction - a.fraction) * t;
            let left = a.left + (b.left - a.left) * t;
            let right = a.right + (b.right - a.right) * t;
            let (mut lo, mut hi) = (255f64, 0f64);
            for j in 0..32 {
                let p = point(
                    m,
                    a.axis,
                    left + (right - left) * (f64::from(j) + 0.5) / 32.,
                    f,
                )
                .unwrap();
                let v = im.gray(p[0].round(), p[1].round());
                work.continuity_samples += 1;
                lo = lo.min(v);
                hi = hi.max(v);
            }
            if hi - lo < 8. {
                work.continuity_rejects += 1;
                return false;
            }
        }
        true
    }
}
#[cfg(feature = "experimental-gap-density")]
fn connected_density(
    im: ImageView<'_>,
    m: [f64; 9],
    a: Observation,
    b: Observation,
    work: &mut Work,
    mut budget: Option<&mut AssociationBudget>,
    cache: &mut std::collections::HashMap<[u64; 5], Option<(usize, usize)>>,
) -> bool {
    let u = (a.left + a.right + b.left + b.right) / 4.;
    let pa = point(m, a.axis, u, a.fraction).unwrap();
    let pb = point(m, a.axis, u, b.fraction).unwrap();
    let steps = (distance(pa, pb) * 2.).ceil() as usize;
    if steps > 4096 {
        work.continuity_capped_links += 1;
        work.association_truncated = 1;
        return false;
    }
    let mut sample = |t: f64, n: usize| -> Option<(usize, usize)> {
        let f = a.fraction + (b.fraction - a.fraction) * t;
        let left = a.left + (b.left - a.left) * t;
        let right = a.right + (b.right - a.right) * t;
        let key = [
            a.axis as u64,
            f.to_bits(),
            left.to_bits(),
            right.to_bits(),
            n as u64,
        ];
        if let Some(value) = cache.get(&key) {
            work.continuity_cache_hits += 1;
            return *value;
        }
        if let Some(budget) = budget.as_deref_mut() {
            if !budget.pixels(n, work) {
                return None;
            }
        }
        let mut values = [0.; 192];
        let (mut lo, mut hi) = (255f64, 0f64);
        for (j, v) in values[..n].iter_mut().enumerate() {
            let p = point(
                m,
                a.axis,
                left + (right - left) * (j as f64 + 0.5) / n as f64,
                f,
            )
            .unwrap();
            *v = im.gray(p[0].round(), p[1].round());
            work.continuity_samples += 1;
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        if hi - lo < 8. {
            work.continuity_rejects += 1;
            if cache.len() < 4096 {
                cache.insert(key, None);
            }
            return None;
        }
        let result = Some((
            values[..n]
                .iter()
                .filter(|&&v| v < lo + 0.35 * (hi - lo))
                .count(),
            values[..n]
                .iter()
                .filter(|&&v| v > lo + 0.65 * (hi - lo))
                .count(),
        ));
        if cache.len() < 4096 {
            cache.insert(key, result);
        }
        result
    };
    let Some(first) = sample(0., 32) else {
        return false;
    };
    if steps == 0 {
        return true;
    }
    let Some(last) = sample(1., 32) else {
        return false;
    };
    let minimum = (first.0.min(last.0), first.1.min(last.1));
    for i in 1..steps {
        let Some((dark, _light)) = sample(i as f64 / steps as f64, 32) else {
            return false;
        };
        // For ordinary dark bars on a light substrate, require lost DARK occupancy.
        // A bright specular maximum can reduce normalized light occupancy while
        // preserving every black bar; that alone is not a separating gap.
        // Only a large dark-density collapse relative to BOTH endpoints proves
        // a separator. Weak endpoints cannot establish a stronger density promise.
        if minimum.0 >= 6 && minimum.1 >= 6 && dark * 2 < minimum.0 {
            // Sparse samples can alias an ordinary EAN row. Confirm a suspected
            // separator against densely sampled endpoint and intervening profiles.
            let (Some(da), Some(db), Some(dc)) = (
                sample(0., 192),
                sample(1., 192),
                sample(i as f64 / steps as f64, 192),
            ) else {
                return false;
            };
            let dm = (da.0.min(db.0), da.1.min(db.1));
            if dm.0 >= 36 && dm.1 >= 36 && dc.0 * 2 < dm.0 {
                work.continuity_rejects += 1;
                return false;
            }
        }
    }
    true
}

fn assemble(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
) -> Vec<Detection> {
    let mut results = vec![];
    for axis in 0..2 {
        let mut group: Vec<Observation> = vec![];
        let emit = |g: &[Observation], out: &mut Vec<Detection>| {
            if g.len() < 2 {
                return;
            }
            let a = g[0];
            let b = *g.last().unwrap();
            if b.fraction - a.fraction < 0.149_999 {
                return;
            }
            let polygon = [
                point(m, axis, a.left, a.fraction).unwrap(),
                point(m, axis, a.right, a.fraction).unwrap(),
                point(m, axis, b.right, b.fraction).unwrap(),
                point(m, axis, b.left, b.fraction).unwrap(),
            ];
            if scan::transform(polygon).is_ok() {
                out.push(Detection {
                    digits: a.digits,
                    polygon,
                    support: g.len(),
                    axis,
                });
            }
        };
        for &b in observations.iter().filter(|r| r.axis == axis) {
            if let Some(&a) = group.last() {
                let overlap = a.right.min(b.right) - a.left.max(b.left);
                if a.digits != b.digits
                    || overlap <= 0.8 * (a.right - a.left).max(b.right - b.left)
                    || !connected(im, m, a, b, work)
                {
                    emit(&group, &mut results);
                    group.clear();
                }
            }
            group.push(b);
        }
        emit(&group, &mut results);
    }
    results
}
/// Associate multiple intervals on each scanline with distinct spatial tracks.
/// A track receives at most one observation per fraction. Non-unique links are
/// left unassociated, rather than joining neighboring equal-value instances.
#[cfg(test)]
pub(crate) fn assemble_many(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
) -> Vec<Detection> {
    assemble_many_budget(
        im,
        m,
        observations,
        work,
        source_support,
        &mut AssociationBudget::default(),
    )
}
// Quantize only numeric row identity (1e-12 of proposal extent). Raw evidence
// stays unchanged; nearby physically distinct rows retain separate identities.
fn canonical_row(f: f64) -> f64 {
    (f * 1e12).round() / 1e12
}
pub(crate) fn assemble_many_budget(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
    budget: &mut AssociationBudget,
) -> Vec<Detection> {
    assemble_many_budget_options(im, m, observations, work, source_support, budget, false)
}
pub(crate) fn assemble_many_budget_options(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
    budget: &mut AssociationBudget,
    allow_single_row: bool,
) -> Vec<Detection> {
    let timer = Timer::now();
    let result = assemble_many_budget_inner(
        im,
        m,
        observations,
        work,
        source_support,
        budget,
        allow_single_row,
    );
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    {
        work.support_ms += timer.ms();
    }
    let _ = timer;
    result
}
fn assemble_many_budget_inner(
    im: ImageView<'_>,
    m: [f64; 9],
    observations: &[Observation],
    work: &mut Work,
    source_support: bool,
    budget: &mut AssociationBudget,
    allow_single_row: bool,
) -> Vec<Detection> {
    #[cfg(feature = "experimental-gap-density")]
    let mut density_cache = std::collections::HashMap::new();
    let mut results = vec![];
    for axis in 0..2 {
        let mut obs: Vec<_> = observations
            .iter()
            .copied()
            .filter(|o| o.axis == axis)
            .map(|mut o| {
                o.fraction = canonical_row(o.fraction);
                o
            })
            .collect();
        obs.sort_by(|a, b| {
            a.fraction
                .total_cmp(&b.fraction)
                .then(a.left.total_cmp(&b.left))
        });
        // Different sampling windows can observe the same physical interval on one
        // row. Consolidate only heavily overlapping equal evidence; veto conflicts.
        let mut keep: Vec<_> = obs.iter().map(|o| !o.ambiguous).collect();
        for i in 0..obs.len() {
            for j in i + 1..obs.len() {
                if obs[j].fraction != obs[i].fraction {
                    break;
                }
                if !budget.check(work) {
                    return results;
                }
                let overlap = obs[i].right.min(obs[j].right) - obs[i].left.max(obs[j].left);
                if overlap > 0.
                    && (obs[i].ambiguous || obs[j].ambiguous || obs[i].digits != obs[j].digits)
                {
                    keep[i] = false;
                    keep[j] = false;
                    work.conflicts += 1;
                }
            }
        }
        let barriers: Vec<_> = obs
            .iter()
            .zip(&keep)
            .filter_map(|(o, &keep)| (!keep).then_some(*o))
            .collect();
        let mut unique: Vec<Observation> = vec![];
        for (i, o) in obs.into_iter().enumerate() {
            if !keep[i] {
                continue;
            }
            let mut found = false;
            for a in unique
                .iter_mut()
                .rev()
                .take_while(|a| a.fraction == o.fraction)
            {
                if !budget.check(work) {
                    return results;
                }
                if a.digits == o.digits
                    && a.right.min(o.right) - a.left.max(o.left)
                        > 0.8 * (a.right - a.left).max(o.right - o.left)
                {
                    if (!o.short_quiet && a.short_quiet)
                        || (o.short_quiet == a.short_quiet && o.cost < a.cost)
                    {
                        *a = o;
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                unique.push(o);
            }
        }
        let obs = unique;
        let mut tracks: Vec<Vec<Observation>> = vec![];
        let mut active: Vec<bool> = vec![];
        let mut offset = 0;
        'rows: while offset < obs.len() {
            let end = offset
                + obs[offset..]
                    .iter()
                    .take_while(|o| o.fraction == obs[offset].fraction)
                    .count();
            let row = &obs[offset..end];
            // Decoded contradictory evidence between matching rows ends the earlier
            // track. Without this, alternating values in touching stacked symbols can
            // reconnect across a different symbol because contrast alone stays high.
            for (i, t) in tracks.iter().enumerate() {
                if !budget.check(work) {
                    break 'rows;
                }
                if !active[i] {
                    continue;
                }
                let a = *t.last().unwrap();
                for b in &barriers {
                    if !budget.check(work) {
                        break 'rows;
                    }
                    if b.fraction <= a.fraction || b.fraction > row[0].fraction {
                        continue;
                    }
                    if a.right.min(b.right) > a.left.max(b.left) {
                        active[i] = false;
                        break;
                    }
                }
                for b in row {
                    if !budget.check(work) {
                        break 'rows;
                    }
                    if a.digits != b.digits
                        && a.right.min(b.right) - a.left.max(b.left)
                            > 0.8 * (a.right - a.left).max(b.right - b.left)
                    {
                        active[i] = false;
                        break;
                    }
                }
            }
            let mut links = vec![vec![]; row.len()];
            let mut degree = vec![0; tracks.len()];
            for (j, &b) in row.iter().enumerate() {
                for (i, t) in tracks.iter().enumerate() {
                    if !budget.check(work) {
                        break 'rows;
                    }
                    let a = *t.last().unwrap();
                    let overlap = a.right.min(b.right) - a.left.max(b.left);
                    if active[i]
                        && a.digits == b.digits
                        && overlap > 0.8 * (a.right - a.left).max(b.right - b.left)
                        && connected_budget(
                            im,
                            m,
                            a,
                            b,
                            work,
                            Some(budget),
                            #[cfg(feature = "experimental-gap-density")]
                            &mut density_cache,
                        )
                    {
                        links[j].push(i);
                        degree[i] += 1;
                    }
                    if work.association_truncated > 0 {
                        break 'rows;
                    }
                }
            }
            for (j, &b) in row.iter().enumerate() {
                if links[j].len() == 1 && degree[links[j][0]] == 1 {
                    tracks[links[j][0]].push(b);
                } else {
                    tracks.push(vec![b]);
                    active.push(true);
                }
            }
            offset = end;
        }
        'tracks: for t in tracks {
            let a = t[0];
            let b = *t.last().unwrap();
            let supported =
                t.len() >= 2 && (t.iter().filter(|o| !o.short_quiet).count() >= 2 || t.len() >= 4);
            let separation = if source_support {
                let cross = distance(
                    point(m, axis, 0.5, 0.).unwrap(),
                    point(m, axis, 0.5, 1.).unwrap(),
                );
                // Weak visual evidence requires a wider physical baseline. Adjacent
                // interpolated rows can repeat the same edge defect or parity alias.
                let strong = t.iter().any(|o| o.cost <= 0.08 && o.gap >= 0.08);
                let left = point(m, axis, a.left, a.fraction).unwrap();
                let right = point(m, axis, a.right, a.fraction).unwrap();
                let pixels = if strong {
                    1.
                } else {
                    (distance(left, right) / 95.).clamp(2., 12.)
                };
                pixels / cross.max(1.)
            } else {
                0.15
            };
            if !supported || b.fraction - a.fraction < separation - 1e-6 {
                if allow_single_row {
                    // Retain the row-level ambiguity veto and full quiet-zone requirement.
                    // A permissive read claims a one-source-pixel band, not the entire region.
                    if let Some(o) = t
                        .iter()
                        .filter(|o| {
                            !o.short_quiet
                                && (!cfg!(feature = "experimental-strong-single-row")
                                    || (o.cost <= 0.06 && o.gap >= 0.1))
                        })
                        .min_by(|a, b| a.cost.total_cmp(&b.cost))
                    {
                        let cross = distance(
                            point(m, axis, 0.5, 0.).unwrap(),
                            point(m, axis, 0.5, 1.).unwrap(),
                        );
                        if observations.iter().any(|other| {
                            other.axis == axis
                                && (other.ambiguous || other.digits != o.digits)
                                && (other.fraction - o.fraction).abs() * cross <= 2.
                                && other.right.min(o.right) - other.left.max(o.left)
                                    > 0.8 * (other.right - other.left).max(o.right - o.left)
                        }) {
                            work.conflicts += 1;
                            continue 'tracks;
                        }
                        let half = 0.5 / cross.max(1.);
                        let ps = [
                            point(m, axis, o.left, o.fraction - half),
                            point(m, axis, o.right, o.fraction - half),
                            point(m, axis, o.right, o.fraction + half),
                            point(m, axis, o.left, o.fraction + half),
                        ];
                        if let [Ok(p0), Ok(p1), Ok(p2), Ok(p3)] = ps {
                            let polygon = [p0, p1, p2, p3];
                            if scan::transform(polygon).is_ok() {
                                results.push(Detection {
                                    digits: o.digits,
                                    polygon,
                                    support: 1,
                                    axis,
                                });
                            }
                        }
                    }
                }
                continue;
            }
            let points = [
                point(m, axis, a.left, a.fraction),
                point(m, axis, a.right, a.fraction),
                point(m, axis, b.right, b.fraction),
                point(m, axis, b.left, b.fraction),
            ];
            if let [Ok(p0), Ok(p1), Ok(p2), Ok(p3)] = points {
                let polygon = [p0, p1, p2, p3];
                if scan::transform(polygon).is_ok() {
                    results.push(Detection {
                        digits: a.digits,
                        polygon,
                        support: t.len(),
                        axis,
                    });
                }
            }
        }
    }
    results
}
/// Diagnose whether old global module evidence can decode an accepted run's
/// boundaries. Never used in acceptance or search; no expected digits supplied.
pub fn profile_at_run_boundary(p: &[f32], left: f64, right: f64) -> Option<[u8; 13]> {
    if p.len() < 2
        || !left.is_finite()
        || !right.is_finite()
        || left >= right
        || left < -0.5
        || right > p.len() as f64 - 0.5
        || p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return None;
    }
    let mut modules = [0.; 95];
    for (i, v) in modules.iter_mut().enumerate() {
        let x = (left + (i as f64 + 0.5) * (right - left) / 95.).clamp(0., (p.len() - 1) as f64);
        let j = x.floor() as usize;
        let a = p[j];
        *v = a + (p[(j + 1).min(p.len() - 1)] - a) * (x - j as f64) as f32;
    }
    let a = ean::decode(&modules, 0.1, 0.02);
    modules.reverse();
    let b = ean::decode(&modules, 0.1, 0.02);
    a.or(b).map(|r| r.digits)
}
#[cfg(test)]
mod tests {
    use super::*;
    const BITS:&str="10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    fn fixture(gap: usize) -> (Vec<u8>, Vec<Quad>) {
        let mut p = vec![255u8; 1000 * 240];
        for y in 10..230 {
            if gap > 0 && (110..110 + gap).contains(&y) {
                continue;
            }
            for x in 0..380 {
                if BITS.as_bytes()[x / 4] == b'1' {
                    p[y * 1000 + 30 + x] = 0;
                    p[y * 1000 + 530 + x] = 0;
                }
            }
        }
        (
            p,
            vec![
                [[30., 10.], [410., 10.], [410., 230.], [30., 230.]],
                [[530., 10.], [910., 10.], [910., 230.], [530., 230.]],
            ],
        )
    }
    #[cfg(feature = "experimental-short-quiet")]
    #[test]
    fn short_quiet_requires_four_distinct_source_supported_rows_and_keeps_vetoes() {
        let (w, h) = (512, 160);
        let mut pixels = vec![0u8; w * h];
        for y in 0..h {
            for x in 40..460 {
                pixels[y * w + x] = 255;
            }
            for x in 0..380 {
                if BITS.as_bytes()[x / 4] == b'1' {
                    pixels[y * w + 60 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
        let q = [[60., 20.], [440., 20.], [440., 140.], [60., 140.]];
        let m = scan::transform(q).unwrap();
        let c = Experiment::default().scan(im, &[q], MULTI_FIXED);
        let mut obs: Vec<_> = c[0]
            .observations
            .iter()
            .filter(|o| o.short_quiet && !o.ambiguous && o.axis == 0)
            .copied()
            .collect();
        obs.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
        assert!(obs.len() >= 4);
        assert!(assemble_many(im, m.0, &obs[..3], &mut Work::default(), true).is_empty());
        assert_eq!(
            assemble_many(im, m.0, &obs[..4], &mut Work::default(), true).len(),
            1
        );
        let repeated = vec![obs[0]; 4];
        assert!(assemble_many(im, m.0, &repeated, &mut Work::default(), true).is_empty());
        let mut conflict = obs[..4].to_vec();
        conflict.push(Observation {
            ambiguous: true,
            ..obs[2]
        });
        assert!(assemble_many(im, m.0, &conflict, &mut Work::default(), true).is_empty());
    }
    #[cfg(feature = "experimental-short-quiet")]
    #[test]
    fn short_quiet_keeps_separate_equal_and_different_symbols() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        for second in [a, b] {
            let (w, h) = (512, 320);
            let mut pixels = vec![255u8; w * h];
            for (top, d) in [(10, a), (180, second)] {
                let bits = crate::ean::encode(&d);
                for y in top..top + 130 {
                    for x in 0..w {
                        pixels[y * w + x] = if (40..460).contains(&x) { 255 } else { 0 };
                    }
                    for x in 0..380 {
                        if bits[x / 4] > 0.5 {
                            pixels[y * w + 60 + x] = 0;
                        }
                    }
                }
            }
            let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
            let q = [[60., 10.], [440., 10.], [440., 310.], [60., 310.]];
            let c = Experiment::default().scan(
                im,
                &[q],
                Config::new("short_dense", false, true, DecoderMode::Many).unwrap(),
            );
            assert!(
                c[0].observations
                    .iter()
                    .filter(|o| o.short_quiet && !o.ambiguous)
                    .count()
                    >= 8
            );
            assert_eq!(c[0].detections.len(), 2);
            assert_eq!(c[0].detections[0].digits, a);
            assert_eq!(c[0].detections[1].digits, second);
            assert!(c[0].detections[0].polygon[2][1] < c[0].detections[1].polygon[0][1]);
        }
    }
    #[test]
    fn broad_region_associates_parallel_equal_tracks() {
        let (p, _) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let q = [[[0., 10.], [1000., 10.], [1000., 230.], [0., 230.]]];
        let out = Experiment::default().scan(im, &q, MULTI_FIXED);
        assert_eq!(out[0].observations.len(), 10);
        assert_eq!(out[0].detections.len(), 2);
        assert!(out[0].detections.iter().all(|d| d.support == 5));
        assert!(out[0].detections[0].polygon[1][0] < out[0].detections[1].polygon[0][0]);
    }
    #[test]
    fn canonical_rows_deduplicate_and_veto_conflicts_without_merging_nearby_rows() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let a = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [0; 13],
            axis: 0,
            fraction: 0.3,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        let b = Observation {
            fraction: 0.1 + 0.2,
            ..a
        };
        let c = Observation { fraction: 0.7, ..a };
        let ds = assemble_many(im, m.0, &[a, b, c], &mut Work::default(), false);
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].support, 2);
        let conflict = Observation {
            digits: [1; 13],
            ..b
        };
        assert!(assemble_many(im, m.0, &[a, conflict, c], &mut Work::default(), false).is_empty());
        assert_ne!(canonical_row(0.3), canonical_row(0.3 + 1e-8));
    }
    #[test]
    fn invalid_boundary_diagnostics_return_none() {
        for p in [vec![], vec![0.], vec![f32::NAN; 512], vec![1.1; 512]] {
            assert!(profile_at_run_boundary(&p, 0., 95.).is_none());
        }
        for (a, b) in [
            (f64::NAN, 95.),
            (0., f64::INFINITY),
            (5., 5.),
            (5., 4.),
            (-1., 95.),
            (0., 512.),
        ] {
            assert!(profile_at_run_boundary(&[0.; 512], a, b).is_none());
        }
    }
    #[test]
    fn one_fraction_cannot_supply_its_own_consensus() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let a = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [0; 13],
            axis: 0,
            fraction: 0.5,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        assert!(assemble_many(im, m.0, &[a, a], &mut Work::default(), false).is_empty());
    }
    #[test]
    fn incompatible_intervening_read_ends_a_track() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let mut obs = vec![];
        for (fraction, value) in [(0.1, 0), (0.2, 0), (0.3, 1), (0.4, 1), (0.5, 0), (0.6, 0)] {
            obs.push(Observation {
                short_quiet: false,
                ambiguous: false,
                digits: [value; 13],
                axis: 0,
                fraction,
                left: 0.,
                right: 1.,
                cost: 0.,
                gap: 1.,
            });
        }
        let out = assemble_many(im, m.0, &obs, &mut Work::default(), true);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|d| d.support == 2));
    }
    #[test]
    fn permissive_single_row_is_opt_in_and_keeps_ambiguity_veto() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap().0;
        let o = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7],
            axis: 0,
            fraction: 0.5,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        let run = |obs: &[Observation], allow| {
            assemble_many_budget_options(
                im,
                m,
                obs,
                &mut Work::default(),
                true,
                &mut AssociationBudget::default(),
                allow,
            )
        };
        assert!(run(&[o], false).is_empty());
        let ds = run(&[o], true);
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].support, 1);
        assert!((distance(ds[0].polygon[0], ds[0].polygon[3]) - 1.).abs() < 1e-8);
        assert!(run(
            &[Observation {
                ambiguous: true,
                ..o
            }],
            true
        )
        .is_empty());
        assert!(run(
            &[Observation {
                short_quiet: true,
                ..o
            }],
            true
        )
        .is_empty());
        for fraction in [0.5005, 0.505] {
            assert!(run(
                &[
                    o,
                    Observation {
                        fraction,
                        ambiguous: true,
                        ..o
                    }
                ],
                true
            )
            .is_empty());
            assert!(run(
                &[
                    o,
                    Observation {
                        fraction,
                        digits: [4; 13],
                        ..o
                    }
                ],
                true
            )
            .is_empty());
        }
        assert!(run(
            &[
                o,
                Observation {
                    digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
                    ..o
                }
            ],
            true
        )
        .is_empty());
        assert_eq!(run(&[o, o, o], true)[0].support, 1);
    }
    #[test]
    fn ambiguous_row_is_a_spatial_barrier() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let o = |fraction, digits| Observation {
            short_quiet: false,
            ambiguous: false,
            digits,
            axis: 0,
            fraction,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        let obs = [
            o(0.1, [0; 13]),
            o(0.2, [0; 13]),
            o(0.3, [0; 13]),
            o(0.3, [1; 13]),
            o(0.5, [0; 13]),
            o(0.6, [0; 13]),
        ];
        let out = assemble_many(im, m.0, &obs, &mut Work::default(), true);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|d| d.support == 2));
        let mut direct = vec![obs[0], obs[1], obs[2], obs[4], obs[5]];
        direct[2].ambiguous = true;
        assert_eq!(
            assemble_many(im, m.0, &direct, &mut Work::default(), true).len(),
            2
        );
    }
    #[test]
    fn aggregate_link_and_pixel_limits_are_reported() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let a = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [0; 13],
            axis: 0,
            fraction: 0.2,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        let b = Observation { fraction: 0.8, ..a };
        for (checks, pixels) in [(0, 100_000), (100, 31)] {
            let mut work = Work::default();
            let mut budget = AssociationBudget {
                checks_left: checks,
                pixels_left: pixels,
            };
            assert!(
                assemble_many_budget(im, m.0, &[a, b], &mut work, true, &mut budget).is_empty()
            );
            assert_eq!(work.association_truncated, 1);
            assert!(work.association_checks <= checks);
            assert!(work.continuity_samples <= pixels);
        }
        let long = scan::transform([[0., 0.], [1000., 0.], [1000., 10000.], [0., 10000.]]).unwrap();
        let mut work = Work::default();
        assert!(!connected_budget(
            im,
            long.0,
            a,
            b,
            &mut work,
            Some(&mut AssociationBudget::default()),
            #[cfg(feature = "experimental-gap-density")]
            &mut std::collections::HashMap::new()
        ));
        assert_eq!(work.continuity_capped_links, 1);
        assert_eq!(work.association_truncated, 1);
        assert_eq!(work.continuity_samples, 0);
    }
    #[test]
    fn integer_run_decoder_matches_original_at512() {
        let mut ex = Experiment::default();
        for reverse in [false, true] {
            for shift in 40..44 {
                let mut p = vec![0f32; 512];
                for i in 0..380 {
                    p[shift + i] = f32::from(BITS.as_bytes()[i / 4] - b'0');
                }
                if reverse {
                    p.reverse();
                }
                ex.signal = p.clone();
                let old = crate::run_profile::decode(&p).unwrap();
                let new = ex.run_decode(&mut Work::default()).map(|r| r.0);
                assert_eq!(
                    old.map(|r| (r.digits, r.left, r.right)),
                    new.map(|r| (r.digits, r.left, r.right))
                );
            }
        }
    }
    #[test]
    fn public_configuration_rejects_incompatible_native_profiles() {
        for mode in [DecoderMode::Profile, DecoderMode::Combined] {
            assert!(matches!(
                Config::new("invalid", true, false, mode),
                Err(Error::Parameters)
            ));
        }
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        for mode in [DecoderMode::Runs, DecoderMode::Many] {
            let c = Config::new("valid", true, false, mode).unwrap();
            let out = Experiment::default().scan(im, &qs, c);
            assert!(!out.is_empty());
        }
    }
    #[cfg(feature = "experimental-native-sharpen")]
    #[test]
    fn sharpening_requires_successful_normalization() {
        let mut ex = Experiment::default();
        for value in [0., 0.5, 1., 24., 255.] {
            ex.signal = vec![value; 200];
            ex.raw_signal = ex.signal.clone();
            assert!(!ex.normalize_interior(-0.15, 1.15, &mut Work::default()));
            let before = ex.signal.clone();
            assert!(!ex.sharpen_native_profile(false));
            assert_eq!(ex.signal, before);
        }
        for n in [75, 385, 512] {
            ex.signal = vec![0.; n];
            assert!(!ex.sharpen_native_profile(true));
        }
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            ex.signal = vec![0.; 200];
            ex.signal[100] = value;
            assert!(!ex.sharpen_native_profile(true));
        }
    }
    #[cfg(feature = "experimental-native-sharpen")]
    #[test]
    fn sharpening_keeps_quiet_endpoints_and_cannot_multiply_row_support() {
        let mut ex = Experiment::default();
        ex.signal = vec![0.; 240];
        for i in 0..190 {
            ex.signal[25 + i] = (BITS.as_bytes()[i / 2] - b'0') as f32;
        }
        let mut observations = vec![];
        let mut work = Work::default();
        ex.collect_policy(0, 0.5, 0., 1., &mut work, &mut observations, true, true);
        assert!(!observations.is_empty());
        assert!(ex.sharpen_native_profile(true));
        assert_eq!(ex.signal[0], 0.);
        assert_eq!(ex.signal[239], 0.);
        ex.collect_policy(0, 0.5, 0., 1., &mut work, &mut observations, true, true);
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        assert!(assemble_many(im, m.0, &observations, &mut Work::default(), true).is_empty());
    }
    #[cfg(feature = "experimental-native-sharpen")]
    #[test]
    fn filtered_unfiltered_conflict_vetoes_a_physical_row() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let raw = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7],
            axis: 0,
            fraction: 0.3,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        let filtered = Observation {
            digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
            ..raw
        };
        let second = Observation {
            fraction: 0.7,
            ..raw
        };
        assert_eq!(
            assemble_many(im, m.0, &[raw, second], &mut Work::default(), false).len(),
            1
        );
        let mut work = Work::default();
        assert!(assemble_many(im, m.0, &[raw, filtered, second], &mut work, false).is_empty());
        assert!(work.conflicts > 0);
    }
    #[cfg(feature = "experimental-native-sharpen")]
    #[test]
    fn sharpening_damaged_guards_and_low_amplitude_noise_do_not_read() {
        let mut ex = Experiment::default();
        for damaged in [false, true] {
            ex.signal = (0..240)
                .map(|i| if i % 2 == 0 { 0.49 } else { 0.51 })
                .collect();
            if damaged {
                ex.signal.fill(0.);
                for i in 0..190 {
                    ex.signal[25 + i] = (BITS.as_bytes()[i / 2] - b'0') as f32;
                }
                ex.signal[25..31].fill(0.);
                ex.signal[209..215].fill(0.);
            }
            assert!(ex.sharpen_native_profile(true));
            let mut obs = vec![];
            ex.collect_policy(0, 0.5, 0., 1., &mut Work::default(), &mut obs, true, true);
            assert!(obs.iter().all(|o| o.ambiguous));
        }
    }
    #[test]
    fn interior_normalization_ignores_padding_and_preserves_quiet_values() {
        let mut ex = Experiment::default();
        let n = 512;
        ex.raw_signal = (0..n)
            .map(|i| {
                let u = -0.15 + 1.3 * (f64::from(i) + 0.5) / f64::from(n);
                if !(0.0..=1.0).contains(&u) {
                    255.
                } else if i % 8 < 4 {
                    60.
                } else {
                    150.
                }
            })
            .collect();
        let mut w = Work::default();
        assert!(ex.normalize_interior(-0.15, 1.15, &mut w));
        let a = ex.signal.clone();
        for (i, v) in ex.raw_signal.iter_mut().enumerate() {
            let u = -0.15 + 1.3 * (i as f64 + 0.5) / f64::from(n);
            if !(0.0..=1.0).contains(&u) {
                *v = 180.;
            }
        }
        assert!(ex.normalize_interior(-0.15, 1.15, &mut w));
        assert_eq!(a, ex.signal);
        assert_eq!(a[0], 0.);
        assert_eq!(w.interior_paths, 2);
        assert!(!ex.normalize_interior(0., 1., &mut w));
        ex.raw_signal.fill(150.);
        assert!(!ex.normalize_interior(-0.15, 1.15, &mut w));
        ex.raw_signal = vec![0.; 64];
        assert!(!ex.normalize_interior(-0.15, 1.15, &mut w));
    }
    #[test]
    fn copied_fixed_sampler_is_identical_and_native_changes_only_length() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        let mut a = Sampler::default();
        let old = a.sample(im, m, Path::default()).unwrap().unwrap().to_vec();
        let mut ex = Experiment::default();
        let mut work = Work::default();
        assert!(ex.native_sample(im, m.0, 0, 0.5, &mut work).unwrap());
        assert_eq!(ex.signal.len(), 494);
        let reference = a.sample(im, m, Path::default()).unwrap().unwrap();
        assert_eq!(old, reference);
    }
    #[test]
    fn blank_noise_invalid_and_checksum_are_not_reads() {
        let mut ex = Experiment::default();
        let q = [[[20., 10.], [400., 10.], [400., 230.], [20., 230.]]];
        let mut state = 1u32;
        for mode in 0..8 {
            let mut p = vec![255u8; 420 * 240];
            for v in &mut p {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *v = match mode {
                    0 => 0,
                    1 => 128,
                    2 => 255,
                    _ => (state >> 24) as u8,
                };
            }
            let im = ImageView::new(&p, 420, 240, 1, 420).unwrap();
            for config in CONFIGS {
                let out = ex.scan(im, &q, config);
                assert_eq!(out.len(), 1);
                assert!(out[0].detections.is_empty());
                assert_eq!(out[0].work.paths, if config.dense { 42 } else { 10 });
            }
            let r = ex.scan(im, &[[[0.; 2]; 4]], CONFIGS[0]);
            assert!(r[0].error);
        }
        let mut bad = BITS.as_bytes().to_vec();
        bad[85..92].copy_from_slice(b"1001000");
        ex.signal = vec![0.; 512];
        for i in 0..380 {
            ex.signal[50 + i] = f32::from(bad[i / 4] - b'0');
        }
        assert!(ex.run_decode(&mut Work::default()).is_none());
        assert!(ex.profile_decode(&mut Work::default()).is_none());
    }
    #[test]
    fn equal_text_instances_and_known_white_gaps_survive() {
        let mut ex = Experiment::default();
        for gap in [0, 1, 2, 3, 10] {
            let (p, qs) = fixture(gap);
            let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
            for config in CONFIGS {
                let results = ex.scan(im, &qs, config);
                assert_eq!(results.len(), 2);
                assert_eq!(results[0].coverage, qs[0]);
                assert_eq!(results[1].coverage, qs[1]);
                for r in results {
                    if gap == 0 {
                        assert_eq!(r.detections.len(), 1, "{}", config.name);
                    } else {
                        assert!(
                            r.detections.len() >= 2,
                            "gap {gap} {} {:?}",
                            config.name,
                            r.detections
                        );
                    }
                    for d in r.detections {
                        assert_eq!(d.digits, [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
                        let min = d.polygon.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
                        let max = d
                            .polygon
                            .iter()
                            .map(|p| p[1])
                            .fold(f64::NEG_INFINITY, f64::max);
                        assert!(
                            gap == 0 || max < 110. || min >= 110. + gap as f64 - 1.,
                            "merged gap {gap} {min} {max}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn both_predicted_axes_and_directions() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let mut ex = Experiment::default();
        for turn in 0..4 {
            let mut q = qs[0];
            q.rotate_left(turn);
            for c in CONFIGS {
                let out = ex.scan(im, &[q], c);
                assert_eq!(out[0].detections.len(), 1, "{} turn {turn}", c.name);
                assert_eq!(
                    out[0].detections[0].digits,
                    [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]
                );
            }
        }
    }
    #[test]
    fn unused_pixel_budget_does_not_silently_stop_disjoint_tracks() {
        let pixels = vec![255; 600 * 200];
        let im = ImageView::new(&pixels, 600, 200, 1, 600).unwrap();
        let m = scan::transform([[0., 0.], [600., 0.], [600., 200.], [0., 200.]]).unwrap();
        let a = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [1; 13],
            axis: 0,
            fraction: 0.2,
            left: 0.1,
            right: 0.4,
            cost: 0.,
            gap: 1.,
        };
        let b = Observation {
            digits: [2; 13],
            fraction: 0.5,
            left: 0.6,
            right: 0.9,
            ..a
        };
        for pixels_left in [0, 31] {
            let mut w = Work::default();
            let mut budget = AssociationBudget {
                checks_left: 100,
                pixels_left,
            };
            let r = assemble_many_budget(im, m.0, &[a, b], &mut w, true, &mut budget);
            assert!(r.is_empty());
            assert_eq!(w.association_truncated, 0);
            assert_eq!(w.continuity_samples, 0);
            assert!(w.association_checks > 0);
        }
    }
}

#[cfg(test)]
mod diagnostic_validation_tests {
    use super::*;
    #[test]
    fn both_profile_exports_reject_invalid_paths() {
        let pixels = vec![255; 64 * 64];
        let im = ImageView::new(&pixels, 64, 64, 1, 64).unwrap();
        let q = [[0., 0.], [64., 0.], [64., 64.], [0., 64.]];
        let mut e = Experiment::default();
        for native in [false, true] {
            for (axis, f) in [(2, 0.5), (0, f64::NAN), (0, -0.01), (1, 1.01)] {
                assert!(e.diagnostic_profile(im, q, axis, f, native).is_err());
                assert!(e
                    .diagnostic_interior_profile(im, q, axis, f, native)
                    .is_err());
            }
        }
    }
}

#[cfg(test)]
mod segment_diagnostic_tests {
    use super::*;
    #[test]
    fn explicit_window_matches_native_and_interior_for_both_axes() {
        let pixels: Vec<u8> = (0..128 * 96)
            .map(|i| ((i * 37 + i / 128 * 13) % 256) as u8)
            .collect();
        let im = ImageView::new(&pixels, 128, 96, 1, 128).unwrap();
        let q = [[15., 12.], [110., 17.], [105., 80.], [20., 85.]];
        let m = scan::transform(q).unwrap();
        let mut e = Experiment::default();
        for axis in 0..2 {
            let n = distance(
                point(m.0, axis, -0.15, 0.5).unwrap(),
                point(m.0, axis, 1.15, 0.5).unwrap(),
            )
            .ceil()
            .clamp(64., 4096.) as usize;
            if !cfg!(feature = "experimental-nearest-lowres") {
                assert_eq!(
                    e.diagnostic_profile(im, q, axis, 0.5, true).unwrap(),
                    e.diagnostic_segment(im, q, axis, 0.5, -0.15, 1.15, n, false)
                        .unwrap()
                );
                assert_eq!(
                    e.diagnostic_interior_profile(im, q, axis, 0.5, true)
                        .unwrap(),
                    e.diagnostic_segment(im, q, axis, 0.5, -0.15, 1.15, n, true)
                        .unwrap()
                );
            }
        }
        for (axis, f, lo, hi, n) in [
            (2, 0.5, 0., 1., 64),
            (0, f64::NAN, 0., 1., 64),
            (0, 0.5, 1., 0., 64),
            (0, 0.5, 0., 1., 63),
            (0, 0.5, 0., 1., 4097),
            (0, 0.5, f64::NAN, 1., 64),
        ] {
            assert!(e
                .diagnostic_segment(im, q, axis, f, lo, hi, n, false)
                .is_err());
        }
    }
}

#[cfg(test)]
mod interior_bounds_tests {
    use super::*;
    #[test]
    fn exact_bounds_match_every_legacy_selected_index() {
        for n in [0, 64, 95, 512, 1905, 4096] {
            for (lo, hi) in [
                (-0.15, 1.15),
                (-1., 2.),
                (-2., -1.),
                (2., 3.),
                (0., 1.),
                (-0.000_000_01, 1.000_000_01),
                (-0.5, 0.5),
                (0.5, 1.5),
            ] {
                let selected: Vec<_> = (0..n)
                    .filter(|&i| {
                        let u = lo + (hi - lo) * (i as f64 + 0.5) / n as f64;
                        (0.0..=1.0).contains(&u)
                    })
                    .collect();
                let (a, b) = interior_bounds(n, lo, hi);
                assert_eq!((a..b).collect::<Vec<_>>(), selected);
            }
        }
    }
}

#[cfg(test)]
mod low_contrast_signal_tests {
    use super::*;
    #[test]
    fn dim_symbols_and_noisy_non_symbols_keep_structure_checks() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let bits = crate::ean::encode(&digits);
        let (w, h) = (512, 80);
        let q = [[40., 10.], [420., 10.], [420., 70.], [40., 70.]];
        for contrast in [12u8, 24, 48] {
            let mut data = vec![32 + contrast; w * h];
            for y in 10..70 {
                for x in 0..380 {
                    if bits[x / 4] > 0.5 {
                        data[y * w + 40 + x] = 32;
                    }
                }
            }
            let im = ImageView::new(&data, w, h, 1, w).unwrap();
            let f = Experiment::default()
                .scan_frame(
                    im,
                    &[q],
                    crate::multi_scan::Policy {
                        transition_cleanup: true,
                        source_identity: true,
                        interior_normalization: true,
                        guard_bias: true,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_eq!(f.barcodes.len(), 1);
            assert_eq!(f.barcodes[0].detection.digits, digits);
        }
        let mut rng = 1_234_567_u32;
        for mode in 0..8 {
            let mut data = vec![0; w * h];
            for y in 0..h {
                for x in 0..w {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    data[y * w + x] = 32
                        + if mode == 0 {
                            0
                        } else if mode == 1 {
                            ((x / 3) % 2 * 24) as u8
                        } else {
                            (rng % 32) as u8
                        };
                }
            }
            let im = ImageView::new(&data, w, h, 1, w).unwrap();
            let f = Experiment::default()
                .scan_frame(
                    im,
                    &[q],
                    crate::multi_scan::Policy {
                        transition_cleanup: true,
                        source_identity: true,
                        interior_normalization: true,
                        guard_bias: true,
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(f.barcodes.is_empty());
        }
    }
}

#[cfg(test)]
mod evidence_span_tests {
    use super::*;
    #[test]
    fn weak_repeated_edges_need_wider_source_support() {
        let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let (w, h) = (360, 100);
        let mut pixels = vec![255; w * h];
        let bits = crate::ean::encode(&d);
        for y in 0..h {
            for x in 0..285 {
                if bits[x / 3] > 0.5 {
                    pixels[y * w + 30 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
        let q = [[0., 0.], [360., 0.], [360., 100.], [0., 100.]];
        let m = scan::transform(q).unwrap().0;
        let obs = |f, cost, gap| Observation {
            short_quiet: false,
            ambiguous: false,
            digits: d,
            axis: 0,
            fraction: f,
            left: 30. / 360.,
            right: 315. / 360.,
            cost,
            gap,
        };
        for strong in [false, true] {
            let cost = if strong { 0.04 } else { 0.11 };
            let pair = [obs(0.5, cost, 0.11), obs(0.52, cost, 0.11)];
            let ds = assemble_many(im, m, &pair, &mut Work::default(), true);
            assert_eq!(ds.len(), usize::from(strong));
        }
        let pair = [obs(0.5, 0.11, 0.11), obs(0.54, 0.11, 0.11)];
        assert_eq!(
            assemble_many(im, m, &pair, &mut Work::default(), true).len(),
            1
        );
    }
}
#[cfg(test)]
mod lowres_threshold_tests {
    use super::*;
    fn shallow(d: [u8; 13]) -> Vec<f32> {
        let bits = crate::ean::encode(&d);
        let mut p = vec![0.; 36];
        let mut at = 0;
        let mut changed = 0;
        while at < bits.len() {
            let mut end = at + 1;
            while end < bits.len() && bits[end] == bits[at] {
                end += 1;
            }
            let mut value = bits[at];
            if at > 3 && at < 45 && value > 0.5 && end - at == 1 && changed < 2 {
                value = 0.4;
                changed += 1;
            }
            for _ in at..end {
                p.extend([value; 3]);
            }
            at = end;
        }
        assert_eq!(changed, 2);
        p.extend([0.; 36]);
        p
    }
    #[test]
    fn threshold_reads_existing_shallow_elements_without_checksum_repair() {
        let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let mut bad = good;
        bad[12] = 8;
        for d in [good, bad] {
            for reverse in [false, true] {
                let mut p = shallow(d);
                if reverse {
                    p.reverse();
                }
                let original = p.clone();
                let mut ex = Experiment::default();
                ex.signal = p;
                let mut obs = vec![];
                ex.collect_policy(0, 0.5, 0., 1., &mut Work::default(), &mut obs, true, true);
                assert_eq!(ex.signal, original);
                assert_eq!(obs.iter().any(|o| !o.ambiguous && o.digits == d), d == good);
            }
        }
    }
}
#[cfg(all(test, feature = "experimental-nearest-lowres"))]
mod nearest_lowres_tests {
    use super::*;
    #[test]
    fn thin_black_pixel_is_retained_by_nearest_sample() {
        let im = ImageView::new(&[255u8, 0, 255], 3, 1, 1, 3).unwrap();
        let bilinear = im.bilinear(1.4, 0.);
        let nearest = im.gray(1.4f64.round(), 0f64.round()) as f32;
        assert_eq!(bilinear, 102.);
        assert_eq!(nearest, 0.);
        assert!(nearest < bilinear);
    }
}
#[cfg(all(test, feature = "experimental-forward-blur"))]
mod forward_blur_region_tests {
    use super::*;
    fn physical_row(d: &[u8; 13]) -> [u8; 512] {
        let bits = ean::encode(d);
        std::array::from_fn(|i| {
            let (mut sum, mut norm) = (0., 0.);
            for n in -640..=640 {
                let dx = f64::from(n) / 32.;
                let weight = (-dx * dx / (2. * 2.6 * 2.6)).exp();
                let module = ((i as f64 + dx - 64.) / 4.).floor() as i32;
                if (0..95).contains(&module) {
                    sum += weight * f64::from(bits[module as usize]);
                }
                norm += weight;
            }
            (255. * (1. - sum / norm)).round() as u8
        })
    }
    #[test]
    fn scan_scaled_many_reaches_blur_and_retains_native_discovery() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let row = physical_row(&digits);
        let mut pixels = vec![];
        for _ in 0..160 {
            pixels.extend_from_slice(&row);
        }
        let im = ImageView::new(&pixels, 512, 160, 1, 512).unwrap();
        let left = 512. * 0.15 / 1.3;
        let right = 512. * 1.15 / 1.3;
        let q = [[left, 0.], [right, 0.], [right, 160.], [left, 160.]];
        let result = Experiment::default()
            .scan_scaled(im, &[q], crate::multi_scan::Policy::default())
            .unwrap();
        assert_eq!(result.len(), 1);
        let c = &result[0];
        assert_eq!(c.coverage, q);
        assert_eq!(c.work.discovery_paths, 10);
        assert!(c.work.forward_blur_calls > 0);
        assert!(c.work.forward_blur_windows > 0);
        assert!(c.work.forward_blur_accepted_windows > 0);
        assert!(c
            .observations
            .iter()
            .any(|o| !o.ambiguous && o.digits == digits));
        assert!(c.detections.iter().all(|d| d.digits == digits));
        assert!(c.work.forward_blur_model_attempts <= c.work.forward_blur_calls * 24);
    }
}

#[cfg(all(test, feature = "experimental-gap-density"))]
mod dark_gap_replay_tests {
    use super::*;
    // Fixed grayscale samples from an ordinary continuous barcode under glare.
    // Endpoint A, endpoint B, interior row; no payload enters this fixture/runtime.
    const DENSE: [[u8; 192]; 3] = [
        [
            31, 95, 129, 146, 16, 70, 127, 134, 135, 143, 141, 140, 57, 28, 23, 38, 136, 125, 143,
            32, 170, 124, 27, 30, 123, 127, 128, 127, 31, 17, 16, 17, 16, 54, 129, 131, 130, 127,
            125, 124, 36, 16, 18, 40, 130, 137, 62, 20, 119, 130, 128, 133, 80, 14, 121, 128, 127,
            118, 60, 26, 17, 15, 110, 124, 127, 124, 34, 13, 61, 134, 21, 12, 14, 13, 16, 11, 123,
            119, 70, 11, 109, 121, 124, 124, 60, 18, 16, 14, 13, 23, 91, 111, 61, 10, 100, 116, 69,
            4, 84, 122, 76, 18, 13, 14, 14, 16, 69, 118, 124, 123, 114, 5, 75, 121, 106, 14, 14,
            13, 13, 15, 49, 115, 119, 118, 102, 11, 28, 124, 51, 26, 64, 112, 121, 118, 129, 4, 38,
            112, 121, 120, 120, 121, 123, 30, 15, 11, 29, 96, 117, 119, 125, 26, 13, 11, 15, 92,
            116, 45, 13, 11, 12, 13, 19, 81, 108, 44, 9, 87, 123, 119, 119, 67, 9, 65, 113, 116,
            124, 79, 22, 17, 15, 15, 17, 50, 116, 132, 6, 27, 131, 128, 3, 7,
        ],
        [
            80, 124, 113, 37, 60, 116, 135, 152, 151, 140, 130, 144, 31, 33, 9, 123, 139, 56, 27,
            120, 141, 47, 22, 123, 129, 131, 131, 23, 19, 20, 18, 20, 21, 100, 127, 154, 136, 130,
            124, 59, 19, 20, 9, 121, 120, 74, 25, 116, 125, 126, 133, 88, 11, 110, 122, 124, 123,
            101, 19, 14, 10, 82, 121, 124, 131, 58, 8, 94, 122, 97, 21, 14, 14, 17, 13, 52, 123,
            92, 5, 60, 119, 123, 120, 88, 18, 16, 14, 12, 13, 37, 135, 120, 6, 10, 120, 120, 18,
            35, 120, 128, 31, 12, 13, 12, 14, 22, 120, 121, 119, 115, 16, 10, 124, 128, 19, 14, 12,
            11, 14, 18, 87, 115, 118, 115, 53, 12, 79, 118, 67, 7, 93, 117, 113, 115, 70, 17, 84,
            120, 116, 119, 117, 115, 100, 13, 11, 11, 26, 114, 115, 118, 99, 27, 11, 11, 5, 122,
            133, 14, 14, 10, 10, 11, 15, 107, 133, 24, 6, 95, 114, 115, 119, 16, 13, 91, 116, 112,
            114, 57, 15, 12, 10, 12, 13, 56, 124, 104, 6, 18, 108, 121, 21, 7,
        ],
        [
            36, 94, 124, 76, 23, 83, 124, 130, 195, 156, 134, 124, 33, 37, 21, 100, 144, 169, 16,
            99, 139, 110, 33, 81, 123, 130, 134, 107, 26, 17, 17, 15, 15, 66, 126, 130, 133, 131,
            120, 101, 29, 16, 13, 88, 125, 121, 11, 38, 127, 127, 130, 124, 29, 36, 127, 125, 126,
            131, 37, 17, 18, 31, 120, 128, 124, 122, 27, 33, 123, 132, 26, 16, 15, 15, 13, 20, 127,
            130, 8, 9, 128, 123, 123, 124, 52, 15, 14, 13, 12, 17, 105, 111, 31, 7, 102, 130, 55,
            7, 122, 132, 75, 14, 12, 11, 13, 23, 83, 122, 120, 121, 82, 10, 96, 116, 74, 12, 10,
            11, 11, 16, 71, 120, 119, 114, 82, 10, 42, 129, 108, 4, 88, 118, 121, 116, 112, 4, 70,
            129, 123, 119, 116, 116, 112, 18, 13, 14, 14, 127, 122, 119, 113, 15, 13, 11, 23, 86,
            121, 29, 11, 11, 11, 11, 12, 98, 125, 47, 3, 98, 114, 115, 116, 76, 3, 69, 113, 115,
            111, 83, 16, 12, 15, 16, 14, 33, 124, 100, 3, 8, 120, 120, 6, 11,
        ],
    ];
    const SPARSE: [[u8; 32]; 3] = [
        [
            132, 141, 24, 124, 122, 22, 124, 125, 129, 124, 127, 124, 20, 126, 15, 19, 128, 14, 51,
            14, 120, 18, 27, 121, 46, 12, 9, 126, 139, 126, 8, 111,
        ],
        [
            126, 155, 106, 95, 77, 74, 111, 116, 121, 128, 122, 127, 27, 121, 13, 7, 111, 15, 5,
            12, 115, 18, 11, 116, 70, 14, 11, 99, 133, 130, 9, 111,
        ],
        [
            117, 199, 24, 130, 130, 26, 127, 128, 136, 122, 125, 120, 17, 116, 13, 10, 112, 15, 16,
            11, 118, 5, 30, 121, 70, 9, 10, 114, 127, 121, 22, 116,
        ],
    ];
    fn counts(v: &[u8]) -> (usize, usize) {
        let lo = f64::from(*v.iter().min().unwrap());
        let hi = f64::from(*v.iter().max().unwrap());
        (
            v.iter()
                .filter(|&&x| f64::from(x) < lo + 0.35 * (hi - lo))
                .count(),
            v.iter()
                .filter(|&&x| f64::from(x) > lo + 0.65 * (hi - lo))
                .count(),
        )
    }
    #[test]
    fn specular_light_density_loss_preserves_dark_bar_continuity() {
        for profiles in [
            SPARSE.iter().map(<[u8; 32]>::as_slice).collect::<Vec<_>>(),
            DENSE.iter().map(<[u8; 192]>::as_slice).collect::<Vec<_>>(),
        ] {
            let a = counts(profiles[0]);
            let b = counts(profiles[1]);
            let mid = counts(profiles[2]);
            assert!(
                mid.1 * 2 < a.1.min(b.1),
                "reproduces previous light-density rejection"
            );
            assert!(mid.0 * 2 >= a.0.min(b.0), "black occupancy remains intact");
        }
        let mut pixels = vec![255u8; 384 * 3];
        // The384px fixture places dense samples on odd x and sparse samples on
        // distinct even x. Preserve exactly both recorded sampling phases.
        for (y, which) in [(0, 0), (1, 2), (2, 1)] {
            for j in 0..192 {
                pixels[y * 384 + 2 * j] = DENSE[which][j];
                pixels[y * 384 + 2 * j + 1] = DENSE[which][j];
            }
            for j in 0..32 {
                pixels[y * 384 + 12 * j + 6] = SPARSE[which][j];
            }
        }
        let q = [[0., 0.], [384., 0.], [384., 4.], [0., 4.]];
        let m = scan::transform(q).unwrap().0;
        let a = Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [0; 13],
            axis: 0,
            fraction: 0.125,
            left: 0.,
            right: 1.,
            cost: 0.,
            gap: 1.,
        };
        let b = Observation {
            fraction: 0.625,
            ..a
        };
        let mut w = Work::default();
        let mut budget = AssociationBudget::default();
        assert!(connected_density(
            ImageView::new(&pixels, 384, 3, 1, 384).unwrap(),
            m,
            a,
            b,
            &mut w,
            Some(&mut budget),
            &mut std::collections::HashMap::new()
        ));
        assert_eq!(w.continuity_rejects, 0);
        // A real white separating source row still rejects the connection.
        pixels[384..768].fill(255);
        let mut w = Work::default();
        assert!(!connected_density(
            ImageView::new(&pixels, 384, 3, 1, 384).unwrap(),
            m,
            a,
            b,
            &mut w,
            None,
            &mut std::collections::HashMap::new()
        ));
        assert!(w.continuity_rejects > 0);
    }
}

#[cfg(all(test, feature = "experimental-structural-retry"))]
mod structural_run_count_tests {
    use super::*;
    #[test]
    fn structural_max_uses_raw_runs_and_freezes_after_discovery() {
        let mut ex = Experiment::default();
        let mut work = Work::default();
        let mut observations = vec![];
        for period in [16, 8, 32] {
            ex.signal = (0..512)
                .map(|i| if (i / period) % 2 == 0 { 0. } else { 1. })
                .collect();
            let mut raw = Vec::new();
            crate::multi_profile::sample_runs(&ex.signal, 64, &mut raw).unwrap();
            let expected = work.max_run_count.max(raw.len());
            ex.collect_policy(0, 0.5, 0., 1., &mut work, &mut observations, true, true);
            assert_eq!(work.max_run_count, expected);
        }
        let frozen = work.max_run_count;
        work.structural_discovery_complete = true;
        ex.signal = (0..512).map(|i| if i % 2 == 0 { 0. } else { 1. }).collect();
        ex.collect_policy(0, 0.5, 0., 1., &mut work, &mut observations, true, true);
        assert_eq!(work.max_run_count, frozen);
    }
}

#[cfg(all(
    test,
    feature = "experimental-redundant-decode",
    feature = "experimental-local-contrast"
))]
mod redundant_collection_test {
    use super::*;
    #[test]
    fn collect_exercises_reuse_without_altering_input_or_observation_shape() {
        let mut ex = Experiment::default();
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let mut signal = vec![0.; 40];
        for bit in crate::ean::encode(&digits) {
            signal.extend([bit; 4]);
        }
        signal.extend([0.; 40]);
        ex.signal = signal.clone();
        let mut work = Work::default();
        let mut observations = vec![];
        ex.collect_policy(
            0,
            0.35,
            -0.15,
            1.15,
            &mut work,
            &mut observations,
            true,
            true,
        );
        assert!(work.redundant_decode_calls_avoided >= 2);
        assert!(work.accepted_paths > 0);
        assert_eq!(ex.signal, signal);
        assert!(observations
            .iter()
            .any(|o| !o.ambiguous && o.digits == digits));
        assert!(observations
            .iter()
            .all(|o| o.axis == 0 && o.fraction == 0.35));
    }
}

#[cfg(all(test, feature = "experimental-asymmetric-quiet"))]
mod folded_boundary_confirmation_tests {
    use super::*;
    const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    #[test]
    fn narrow_trailing_quiet_needs_four_rows_and_preserves_instances_after_rotation() {
        for second in [A, B] {
            for rotated in [false, true] {
                let (w, h) = if rotated { (320, 512) } else { (512, 320) };
                let mut pixels = vec![255u8; w * h];
                let put = |pixels: &mut Vec<u8>, x: usize, y: usize, value| {
                    let (a, b) = if rotated { (319 - y, x) } else { (x, y) };
                    pixels[b * w + a] = value;
                };
                for (top, d) in [(10, A), (180, second)] {
                    let bits = crate::ean::encode(&d);
                    for y in top..top + 130 {
                        for x in 448..512 {
                            put(&mut pixels, x, y, 0);
                        }
                        for x in 0..380 {
                            if bits[x / 4] > 0.5 {
                                put(&mut pixels, 60 + x, y, 0);
                            }
                        }
                    }
                }
                let im = ImageView::new(&pixels, w, h, 1, w).unwrap();
                let mut q = [[60., 10.], [440., 10.], [440., 310.], [60., 310.]];
                if rotated {
                    for p in &mut q {
                        *p = [319. - p[1], p[0]];
                    }
                }
                let c = Experiment::default().scan(
                    im,
                    &[q],
                    Config::new("folded_boundary", false, true, DecoderMode::Many).unwrap(),
                );
                assert_eq!(
                    c[0].detections.len(),
                    2,
                    "distinct symbols, rotated={rotated}"
                );
                assert_eq!(
                    c[0].detections.iter().filter(|d| d.digits == A).count(),
                    if second == A { 2 } else { 1 }
                );
                let mut obs: Vec<_> = c[0]
                    .observations
                    .iter()
                    .filter(|o| o.short_quiet && !o.ambiguous && o.axis == 0 && o.fraction < 0.45)
                    .copied()
                    .collect();
                obs.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
                obs.dedup_by(|a, b| a.fraction == b.fraction);
                assert!(obs.len() >= 4);
                let m = scan::transform(q).unwrap();
                assert!(assemble_many(im, m.0, &obs[..3], &mut Work::default(), true).is_empty());
                assert_eq!(
                    assemble_many(im, m.0, &obs[..4], &mut Work::default(), true).len(),
                    1
                );
                let mut conflict = obs[..4].to_vec();
                conflict.push(Observation {
                    ambiguous: true,
                    ..obs[1]
                });
                assert!(assemble_many(im, m.0, &conflict, &mut Work::default(), true).is_empty());
            }
        }
    }
}

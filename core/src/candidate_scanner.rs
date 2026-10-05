//! Candidate scanning, reusable scratch and evidence collection.
//! One controlled signal path feeds either unchanged profile or run likelihood.
mod sampling;
#[cfg(test)]
use sampling::interior_bounds;
mod association;
mod decoding;
use crate::scanner_clock::Timer;
use crate::{
    ean, profile, run_ean,
    sampling::{Error, ImageView, Path, Sampler},
    scan::{self, Quad},
};
pub(crate) use association::*;
#[derive(Clone, Copy, Debug)]
pub struct Observation {
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    pub invalid_checksum: bool,
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
    #[cfg(not(feature = "mode-low"))]
    /// Additional long-profile threshold attempts, capped at eight per candidate.
    pub medium_threshold_profiles: usize,
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub selected_retry_limit: usize,
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    pub invalid_consensus_observations: usize,
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    pub invalid_consensus_blocks: usize,
    pub invalid_visual_seen: usize,
    pub invalid_veto_intervals: usize,
    pub invalid_veto_reads: usize,
    pub invalid_soft_conflicts: usize,
    pub invalid_veto_capped: usize,

    pub extrema_calls: usize,
    pub extrema_examined: usize,
    pub extrema_capped: usize,
    pub extrema_ambiguous: usize,
    pub extrema_decoder_calls: usize,

    pub redundant_decode_calls_avoided: usize,
    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
    pub max_run_count: usize,
    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
    pub structural_retry_skipped: usize,
    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
    pub(crate) structural_discovery_complete: bool,

    pub continuity_cache_hits: usize,

    pub forward_blur_calls: usize,

    pub forward_blur_windows: usize,

    pub forward_blur_model_attempts: usize,

    pub forward_blur_accepted_windows: usize,

    pub forward_blur_conflicts: usize,

    pub extension_cache_hits: usize,
    pub extension_samples: usize,
    pub extension_claims: usize,
    pub extension_capped: usize,
    pub reuse_claims: usize,

    pub reuse_claims_rejected: usize,

    pub reuse_checks: usize,

    pub reuse_checks_capped: usize,

    pub reuse_paths_changed: usize,

    pub reuse_paths_removed: usize,

    pub reuse_paths_split: usize,

    pub reuse_short_pieces: usize,
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    pub sampling_ms: f64,
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    pub interpretation_ms: f64,
    #[cfg(all(feature = "native-timing", not(target_arch = "wasm32")))]
    pub support_ms: f64,
    #[cfg(feature = "mode-very-high")]
    pub phase_rescue_paths: usize,
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
    #[cfg(feature = "mode-very-high")]
    pub discovery_requests: usize,
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
    fixed3: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecoderMode {
    Profile,
    Runs,
    Combined,
    Many,
}
impl Config {
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub(crate) fn with_three_rows(mut self) -> Self {
        self.fixed3 = true;
        self.name = "multi_fixed3";
        self
    }
    /// # Errors
    /// Rejects native-length sampling with a fixed-length profile decoder.
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
            fixed3: false,
        })
    }
}
pub const MULTI_FIXED: Config = Config {
    name: "multi_fixed5",
    native: false,
    dense: false,
    decoder: DecoderMode::Many,
    fixed3: false,
};
pub const CONFIGS: [Config; 5] = [
    Config {
        name: "profile_fixed5",
        native: false,
        dense: false,
        decoder: DecoderMode::Profile,
        fixed3: false,
    },
    Config {
        name: "runs_fixed5",
        native: false,
        dense: false,
        decoder: DecoderMode::Runs,
        fixed3: false,
    },
    Config {
        name: "runs_native5",
        native: true,
        dense: false,
        decoder: DecoderMode::Runs,
        fixed3: false,
    },
    Config {
        name: "runs_native21",
        native: true,
        dense: true,
        decoder: DecoderMode::Runs,
        fixed3: false,
    },
    Config {
        name: "combined_fixed5",
        native: false,
        dense: false,
        decoder: DecoderMode::Combined,
        fixed3: false,
    },
];
// Exact contiguous selection using the same coordinate expression as the legacy scan.

pub(crate) fn point(
    matrix: [f64; 9],
    axis: usize,
    u: f64,
    fraction: f64,
) -> Result<[f64; 2], Error> {
    let (x, y) = if axis == 0 {
        (u, fraction)
    } else {
        (fraction, u)
    };
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
pub(crate) use crate::geometry::distance;

#[cfg(not(feature = "mode-low"))]
struct CachedProfile {
    signal: Vec<f32>,
    result: Result<Option<profile::Read>, profile::Error>,
    trace: profile::BlurTrace,
}

#[derive(Default)]
pub struct CandidateScanner {
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    pub(crate) threshold_recovery: bool,
    #[cfg(not(feature = "mode-low"))]
    profile_cache: [Option<CachedProfile>; 2],
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub(crate) retail: crate::retail_pipeline::Collector,

    blur_rejected_intervals: Vec<(f64, f64)>,
    fixed: Sampler,
    signal: Vec<f32>,
    raw_signal: Vec<f32>,
    sorted: Vec<f32>,
    runs: Vec<(usize, usize, bool)>,

    local_scratch: crate::local_signal::Scratch,
}
impl CandidateScanner {
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
        self.scan_with_budget_axes(im, candidates, config, budget, false)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "This evidence pass keeps ordered hypotheses, contradiction vetoes and work accounting together within one scan transaction."
    )]
    pub(crate) fn scan_with_budget_axes(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        config: Config,
        budget: &mut AssociationBudget,
        module_axis: bool,
    ) -> Vec<Candidate> {
        candidates
            .iter()
            .enumerate()
            .map(|(index, &coverage)| {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                self.retail.coverage(coverage);
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
                let Ok(m) = scan::transform(coverage) else {
                    out.error = true;
                    return out;
                };
                let full = !module_axis
                    || coverage
                        .iter()
                        .zip([
                            [0., 0.],
                            [crate::numeric::usize_f64(im.width - 1), 0.],
                            [
                                crate::numeric::usize_f64(im.width - 1),
                                crate::numeric::usize_f64(im.height - 1),
                            ],
                            [0., crate::numeric::usize_f64(im.height - 1)],
                        ])
                        .all(|(a, b)| (a[0] - b[0]).abs() <= 1. && (a[1] - b[1]).abs() <= 1.);
                for axis in 0..2 {
                    if module_axis && axis == 1 && !full {
                        continue;
                    }
                    let dense = std::array::from_fn::<_, 21, _>(|i| {
                        0.2 + 0.03 * f64::from(u32::try_from(i).unwrap())
                    });
                    let fractions: &[f64] = if config.dense {
                        &dense
                    } else if config.fixed3 {
                        &[0.2, 0.5, 0.8]
                    } else {
                        &[0.2, 0.35, 0.5, 0.65, 0.8]
                    };
                    for &fraction in fractions {
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
                            let before = out.observations.len();
                            self.collect_many(
                                axis,
                                fraction,
                                -0.15,
                                1.15,
                                &mut out.work,
                                &mut out.observations,
                            );
                            if out.observations.len() == before {
                                self.extend_partial(
                                    im,
                                    m.0,
                                    axis,
                                    fraction,
                                    -0.15,
                                    1.15,
                                    &mut out.work,
                                    &mut out.observations,
                                    false,
                                    false,
                                );
                            }
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
                            let n = crate::numeric::usize_f64(self.signal.len());
                            out.observations.push(Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
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

// Quantize only numeric row identity (1e-12 of proposal extent). Raw evidence
// stays unchanged; nearby physically distinct rows retain separate identities.

// Invalid vetoes require three physically separated paths, and full interval
// containment so a broad unsupported row cannot borrow a narrow track's support.

/// Diagnose whether old global module evidence can decode an accepted run's
/// boundaries. Never used in acceptance or search; no expected digits supplied.
#[must_use]
pub fn profile_at_run_boundary(p: &[f32], left: f64, right: f64) -> Option<[u8; 13]> {
    if p.len() < 2
        || !left.is_finite()
        || !right.is_finite()
        || left >= right
        || left < -0.5
        || right > crate::numeric::usize_f64(p.len()) - 0.5
        || p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return None;
    }
    let mut modules = [0.; 95];
    for (i, v) in modules.iter_mut().enumerate() {
        let x = (left + (crate::numeric::usize_f64(i) + 0.5) * (right - left) / 95.)
            .clamp(0., crate::numeric::usize_f64(p.len() - 1));
        let j = crate::numeric::f64_usize(x.floor());
        let a = p[j];
        *v = a
            + (p[(j + 1).min(p.len() - 1)] - a)
                * crate::numeric::f64_f32(x - crate::numeric::usize_f64(j));
    }
    let a = ean::decode(&modules, 0.1, 0.02);
    modules.reverse();
    let b = ean::decode(&modules, 0.1, 0.02);
    a.or(b).map(|r| r.digits)
}
#[cfg(test)]
mod dark_gap_replay_tests;
#[cfg(test)]
mod diagnostic_validation_tests;
#[cfg(test)]
mod evidence_span_tests;
#[cfg(test)]
mod forward_blur_region_tests;
#[cfg(test)]
mod interior_bounds_tests;
#[cfg(test)]
mod low_contrast_signal_tests;
#[cfg(test)]
mod lowres_threshold_tests;
#[cfg(test)]
mod nearest_lowres_tests;
#[cfg(test)]
mod segment_diagnostic_tests;
#[cfg(test)]
mod tests;
#[cfg(all(test, any(feature = "mode-low", feature = "mode-very-high")))]
mod structural_run_count_tests {
    use super::*;
    #[test]
    fn structural_max_uses_raw_runs_and_freezes_after_discovery() {
        let mut ex = CandidateScanner::default();
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
#[cfg(test)]
mod folded_boundary_confirmation_tests;
#[cfg(test)]
mod redundant_collection_test;
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
#[cfg(all(test, any(feature = "mode-high", feature = "mode-very-high")))]
mod invalid_consensus_tests {
    use super::*;
    #[test]
    fn invalid_values_never_escape_and_only_three_distinct_rows_block() {
        let pixels: Vec<u8> = (0..600 * 100)
            .map(|i| if i % 600 % 4 < 2 { 0 } else { 255 })
            .collect();
        let im = ImageView::new(&pixels, 600, 100, 1, 600).unwrap();
        let q = [[0., 0.], [599., 0.], [599., 99.], [0., 99.]];
        let m = scan::transform(q).unwrap().0;
        let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let mut bad = good;
        bad[12] = 8;
        let o = |fraction, invalid, left, right| Observation {
            invalid_checksum: invalid,
            short_quiet: false,
            ambiguous: invalid,
            digits: if invalid { bad } else { good },
            axis: 0,
            fraction,
            left,
            right,
            cost: 0.01,
            gap: 0.2,
        };
        let run = |rows: &[Observation]| {
            let mut w = Work::default();
            let out = assemble_many_budget_inner(
                im,
                m,
                rows,
                &mut w,
                true,
                &mut AssociationBudget::default(),
                false,
            );
            (out, w)
        };
        let invalid: Vec<_> = [0.2, 0.5, 0.8]
            .into_iter()
            .map(|f| o(f, true, 0.05, 0.45))
            .collect();
        let (out, w) = run(&invalid);
        assert!(out.is_empty());
        assert!(w.invalid_consensus_blocks >= 3);
        let valid: Vec<_> = [0.2, 0.5, 0.8]
            .into_iter()
            .map(|f| o(f, false, 0.05, 0.45))
            .collect();
        let mut all = valid.clone();
        all.extend(&invalid);
        assert!(run(&all).0.is_empty());
        let mut only_two = valid.clone();
        only_two.extend(&invalid[..2]);
        assert!(run(&only_two).0.iter().any(|d| d.digits == good));
        let mut duplicate = valid.clone();
        duplicate.extend([invalid[0]; 8]);
        assert!(run(&duplicate).0.iter().any(|d| d.digits == good));
        let mut separated = invalid;
        separated.extend([0.2, 0.5, 0.8].into_iter().map(|f| o(f, false, 0.55, 0.95)));
        let result = run(&separated).0;
        assert!(result.iter().any(|d| d.digits == good));
        assert!(result.iter().all(|d| d.digits != bad));
    }
}
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
#[cfg(all(test, any(feature = "mode-high", feature = "mode-very-high")))]
mod invalid_spacing_tests {
    use super::*;
    #[test]
    fn fractional_repeats_do_not_count_as_distinct_source_rows() {
        let q = [[0., 0.], [199., 0.], [199., 99.], [0., 99.]];
        let m = scan::transform(q).unwrap().0;
        let d = Detection {
            digits: [1; 13],
            polygon: q,
            support: 3,
            axis: 0,
        };
        let obs = |fraction| Observation {
            invalid_checksum: true,
            short_quiet: false,
            ambiguous: true,
            digits: [1; 13],
            axis: 0,
            fraction,
            left: 0.1,
            right: 0.9,
            cost: 0.01,
            gap: 0.2,
        };
        let spaced = |v: &[Observation]| {
            invalid_track_spaced(
                m,
                &d,
                v,
                &mut AssociationBudget::default(),
                &mut Work::default(),
            )
        };
        assert!(!spaced(&[obs(0.3), obs(0.3001), obs(0.3002)]));
        assert!(!spaced(&[obs(0.3), obs(0.3001), obs(0.4)]));
        assert!(spaced(&[obs(0.3), obs(0.32), obs(0.4)]));
        assert!(!spaced(&[obs(0.3), obs(0.3), obs(0.4)]));
    }
}
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
#[cfg(all(test, any(feature = "mode-high", feature = "mode-very-high")))]
mod invalid_bounded_tests {
    use super::*;
    #[test]
    fn wide_unsupported_interval_cannot_borrow_narrow_track_support() {
        let matrix = scan::transform([[0., 0.], [100., 0.], [100., 100.], [0., 100.]])
            .unwrap()
            .0;
        let d = Detection {
            digits: [1; 13],
            polygon: [
                point(matrix, 0, 0.3, 0.2).unwrap(),
                point(matrix, 0, 0.5, 0.2).unwrap(),
                point(matrix, 0, 0.5, 0.8).unwrap(),
                point(matrix, 0, 0.3, 0.8).unwrap(),
            ],
            support: 3,
            axis: 0,
        };
        let o = Observation {
            invalid_checksum: true,
            short_quiet: false,
            ambiguous: true,
            digits: [1; 13],
            axis: 0,
            fraction: 0.5,
            left: 0.,
            right: 0.8,
            cost: 0.01,
            gap: 0.2,
        };
        assert!(invalid_contains(&d, [40., 50.]));
        assert!(invalid_interval(matrix, &d, &o).is_none());
        let o = Observation {
            left: 0.3,
            right: 0.5,
            ..o
        };
        assert!(invalid_interval(matrix, &d, &o).is_some());
        let mut work = Work::default();
        let mut b = AssociationBudget {
            checks_left: 0,
            pixels_left: 1000,
        };
        assert!(!invalid_track_spaced(matrix, &d, &[o], &mut b, &mut work));
        assert_eq!(work.association_truncated, 1);
        assert_eq!(work.association_checks, 0);
    }
}

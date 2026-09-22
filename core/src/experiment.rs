//! Isolated supplied-region experiment. No reference decoders or label inputs.
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
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    pub selected_retry_limit: usize,
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    pub invalid_consensus_observations: usize,
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
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
pub(crate) fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[derive(Default)]
pub struct Experiment {
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
impl Experiment {
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
                    let fractions: Vec<f64> = if config.dense {
                        (0..21).map(|i| 0.2 + 0.03 * f64::from(i)).collect()
                    } else if config.fixed3 {
                        vec![0.2, 0.5, 0.8]
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

    #[test]
    fn short_quiet_requires_four_distinct_source_supported_rows_and_keeps_vetoes() {
        let (width, h) = (512, 160);
        let mut pixels = vec![0u8; width * h];
        for y in 0..h {
            for x in 40..460 {
                pixels[y * width + x] = 255;
            }
            for x in 0..380 {
                if BITS.as_bytes()[x / 4] == b'1' {
                    pixels[y * width + 60 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, width, h, 1, width).unwrap();
        let quad = [[60., 20.], [440., 20.], [440., 140.], [60., 140.]];
        let m = scan::transform(quad).unwrap();
        let c = Experiment::default().scan(im, &[quad], MULTI_FIXED);
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
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                conflict.push(Observation {
                    ambiguous: true,
                    ..obs[2]
                });
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                conflict.push(Observation {
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    invalid_checksum: false,
                    ambiguous: true,
                    ..obs[2]
                });
            }
        }

        assert!(assemble_many(im, m.0, &conflict, &mut Work::default(), true).is_empty());
    }

    #[test]
    fn short_quiet_keeps_separate_equal_and_different_symbols() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        for second in [a, b] {
            let (w, height) = (512, 320);
            let mut pixels = vec![255u8; w * height];
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
            let im = ImageView::new(&pixels, w, height, 1, w).unwrap();
            let quad = [[60., 10.], [440., 10.], [440., 310.], [60., 310.]];
            let c = Experiment::default().scan(
                im,
                &[quad],
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
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn canonical_rows_deduplicate_and_veto_conflicts_without_merging_nearby_rows() {
        let (pixels, qs) = fixture(0);
        let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let a = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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

        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let b = Observation {
            fraction: 0.1 + 0.2,
            ..a
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let b = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            fraction: 0.1 + 0.2,
            ..a
        };

        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let c = Observation { fraction: 0.7, ..a };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let c = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            fraction: 0.7,
            ..a
        };

        let ds = assemble_many(im, m.0, &[a, b, c], &mut Work::default(), false);
        assert_eq!(ds.len(), 1);
        assert_eq!(ds[0].support, 2);
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let conflict = Observation {
            digits: [1; 13],
            ..b
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let conflict = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let a = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
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
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    obs.push(Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
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
            }
        }
        let out = assemble_many(im, m.0, &obs, &mut Work::default(), true);
        assert_eq!(out.len(), 3);
        assert!(out.iter().all(|d| d.support == 2));
    }
    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Mode-specific acceptance controls share the same adversarial observation fixture."
    )]
    fn permissive_single_row_is_opt_in_and_keeps_ambiguity_veto() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap().0;
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let o = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                assert!(run(
                    &[Observation {
                        ambiguous: true,
                        ..o
                    }],
                    true
                )
                .is_empty());
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                assert!(run(
                    &[Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        ambiguous: true,
                        ..o
                    }],
                    true
                )
                .is_empty());
            }
        }

        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                assert!(run(
                    &[Observation {
                        short_quiet: true,
                        ..o
                    }],
                    true
                )
                .is_empty());
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                assert!(run(
                    &[Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: true,
                        ..o
                    }],
                    true
                )
                .is_empty());
            }
        }

        for fraction in [0.5005, 0.505] {
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
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
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    assert!(run(
                        &[
                            o,
                            Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
                                fraction,
                                ambiguous: true,
                                ..o
                            }
                        ],
                        true
                    )
                    .is_empty());
                }
            }

            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
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
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    assert!(run(
                        &[
                            o,
                            Observation {
                                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                invalid_checksum: false,
                                fraction,
                                digits: [4; 13],
                                ..o
                            }
                        ],
                        true
                    )
                    .is_empty());
                }
            }
        }
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
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
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                assert!(run(
                    &[
                        o,
                        Observation {
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            invalid_checksum: false,
                            digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
                            ..o
                        }
                    ],
                    true
                )
                .is_empty());
            }
        }

        assert_eq!(run(&[o, o, o], true)[0].support, 1);
    }
    #[test]
    fn ambiguous_row_is_a_spatial_barrier() {
        let (p, qs) = fixture(0);
        let im = ImageView::new(&p, 1000, 240, 1, 1000).unwrap();
        let m = scan::transform(qs[0]).unwrap();
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let o = |fraction, digits| Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let a = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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

        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let b = Observation { fraction: 0.8, ..a };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let b = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            fraction: 0.8,
            ..a
        };

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

    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
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
            let u = -0.15 + 1.3 * (crate::numeric::usize_f64(i) + 0.5) / f64::from(n);
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
                            gap == 0
                                || max < 110.
                                || min >= 110. + crate::numeric::usize_f64(gap) - 1.,
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
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let a = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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

        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let b = Observation {
            digits: [2; 13],
            fraction: 0.5,
            left: 0.6,
            right: 0.9,
            ..a
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let b = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            digits: [2; 13],
            fraction: 0.5,
            left: 0.6,
            right: 0.9,
            ..a
        };

        for pixels_left in [0, 31] {
            let mut width = Work::default();
            let mut budget = AssociationBudget {
                checks_left: 100,
                pixels_left,
            };
            let r = assemble_many_budget(im, m.0, &[a, b], &mut width, true, &mut budget);
            assert!(r.is_empty());
            assert_eq!(width.association_truncated, 0);
            assert_eq!(width.continuity_samples, 0);
            assert!(width.association_checks > 0);
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
        let pixels: Vec<u8> = (0usize..128 * 96)
            .map(|i| ((i * 37 + i / 128 * 13) % 256).to_le_bytes()[0])
            .collect();
        let im = ImageView::new(&pixels, 128, 96, 1, 128).unwrap();
        let q = [[15., 12.], [110., 17.], [105., 80.], [20., 85.]];
        let mut e = Experiment::default();
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
                        let u = lo
                            + (hi - lo) * (crate::numeric::usize_f64(i) + 0.5)
                                / crate::numeric::usize_f64(n);
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
                            ((x / 3) % 2 * 24).to_le_bytes()[0]
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
        let (width, h) = (360, 100);
        let mut pixels = vec![255; width * h];
        let bits = crate::ean::encode(&d);
        for y in 0..h {
            for x in 0..285 {
                if bits[x / 3] > 0.5 {
                    pixels[y * width + 30 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, width, h, 1, width).unwrap();
        let quad = [[0., 0.], [360., 0.], [360., 100.], [0., 100.]];
        let m = scan::transform(quad).unwrap().0;
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let obs = |f, cost, gap| Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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
    #[expect(
        clippy::float_cmp,
        reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
    )]
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
                let mut ex = Experiment {
                    signal: p,
                    ..Experiment::default()
                };
                let mut obs = vec![];
                ex.collect_policy(0, 0.5, 0., 1., &mut Work::default(), &mut obs, true, true);
                assert_eq!(ex.signal, original);
                assert_eq!(obs.iter().any(|o| !o.ambiguous && o.digits == d), d == good);
            }
        }
    }
}
#[cfg(test)]
mod nearest_lowres_tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn thin_black_pixel_is_retained_by_nearest_sample() {
        let im = ImageView::new(&[255u8, 0, 255], 3, 1, 1, 3).unwrap();
        let bilinear = im.bilinear(1.4, 0.);
        let nearest = crate::numeric::f64_f32(im.gray(1.4f64.round(), 0f64.round()));
        assert_eq!(bilinear, 102.);
        assert_eq!(nearest, 0.);
        assert!(nearest < bilinear);
    }
}
#[cfg(test)]
mod forward_blur_region_tests {
    use super::*;
    fn physical_row(d: &[u8; 13]) -> [u8; 512] {
        let bits = ean::encode(d);
        std::array::from_fn(|i| {
            let (mut sum, mut norm) = (0., 0.);
            for n in -640..=640 {
                let dx = f64::from(n) / 32.;
                let weight = (-dx * dx / (2. * 2.6 * 2.6)).exp();
                let module = crate::numeric::f64_i32(
                    ((crate::numeric::usize_f64(i) + dx - 64.) / 4.).floor(),
                );
                if (0..95).contains(&module) {
                    sum += weight * f64::from(bits[crate::numeric::i32_usize(module)]);
                }
                norm += weight;
            }
            crate::numeric::f64_u8((255. * (1. - sum / norm)).round())
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
        #[cfg(feature = "mode-low")]
        {
            assert!(
                (5..=10).contains(&c.work.discovery_paths),
                "both normalized axes and at least five native module-axis rows"
            );
        }
        #[cfg(any(feature = "mode-medium", feature = "mode-high"))]
        {
            assert_eq!(c.work.discovery_paths, 10);
        }
        #[cfg(feature = "mode-very-high")]
        {
            assert_eq!(c.work.discovery_requests, 10);
        }
        #[cfg(feature = "mode-very-high")]
        {
            assert!(c.work.discovery_paths >= 4);
        }
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
#[cfg(test)]
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
        let quad = [[0., 0.], [384., 0.], [384., 4.], [0., 4.]];
        let m = scan::transform(quad).unwrap().0;
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
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
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let a = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
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

        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let b = Observation {
            fraction: 0.625,
            ..a
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let b = Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            fraction: 0.625,
            ..a
        };

        let mut width = Work::default();
        let mut budget = AssociationBudget::default();
        assert!(connected_density(
            ImageView::new(&pixels, 384, 3, 1, 384).unwrap(),
            m,
            a,
            b,
            &mut width,
            Some(&mut budget),
            &mut std::collections::HashMap::new()
        ));
        assert_eq!(width.continuity_rejects, 0);
        // A real white separating source row still rejects the connection.
        pixels[384..768].fill(255);
        let mut width = Work::default();
        assert!(!connected_density(
            ImageView::new(&pixels, 384, 3, 1, 384).unwrap(),
            m,
            a,
            b,
            &mut width,
            None,
            &mut std::collections::HashMap::new()
        ));
        assert!(width.continuity_rejects > 0);
    }
}
#[cfg(all(test, any(feature = "mode-low", feature = "mode-very-high")))]
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
#[cfg(test)]
mod redundant_collection_test {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
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
#[cfg(test)]
mod folded_boundary_confirmation_tests {
    use super::*;
    const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn narrow_trailing_quiet_needs_four_rows_and_preserves_instances_after_rotation() {
        for second in [A, B] {
            for rotated in [false, true] {
                let (width, height) = if rotated { (320, 512) } else { (512, 320) };
                let mut pixels = vec![255u8; width * height];
                let put = |pixels: &mut Vec<u8>, x: usize, y: usize, value| {
                    let (a, b) = if rotated { (319 - y, x) } else { (x, y) };
                    pixels[b * width + a] = value;
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
                let im = ImageView::new(&pixels, width, height, 1, width).unwrap();
                let mut quad = [[60., 10.], [440., 10.], [440., 310.], [60., 310.]];
                if rotated {
                    for p in &mut quad {
                        *p = [319. - p[1], p[0]];
                    }
                }
                let c = Experiment::default().scan(
                    im,
                    &[quad],
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
                let m = scan::transform(quad).unwrap();
                assert!(assemble_many(im, m.0, &obs[..3], &mut Work::default(), true).is_empty());
                assert_eq!(
                    assemble_many(im, m.0, &obs[..4], &mut Work::default(), true).len(),
                    1
                );
                let mut conflict = obs[..4].to_vec();
                {
                    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                    {
                        conflict.push(Observation {
                            ambiguous: true,
                            ..obs[1]
                        });
                    }
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    {
                        conflict.push(Observation {
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            invalid_checksum: false,
                            ambiguous: true,
                            ..obs[1]
                        });
                    }
                }

                assert!(assemble_many(im, m.0, &conflict, &mut Work::default(), true).is_empty());
            }
        }
    }
}
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

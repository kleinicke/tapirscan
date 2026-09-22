//! Ordered retry execution over one explicit frame budget.
#[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
use super::defer_structurally_weak_retries;
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
use super::multiple_initial;
use super::plan::{
    scaled_plan_density, supported_scale_width, supported_scale_width_checked, unresolved_plan,
};
use super::{
    experiment, scan, Candidate, Error, Experiment, ImageView, Policy, Quad, Segment, Timer, Work,
};
use super::{reuse_plan, verified_claims, ReuseBudget};

type PreparedPlan = Option<([f64; 9], Vec<Segment>)>;
pub(super) struct Execution {
    policy: Policy,
    association: experiment::AssociationBudget,
    reuse: ReuseBudget,
    used: usize,
}
impl Execution {
    pub fn new(policy: Policy) -> Self {
        Self {
            policy,
            association: experiment::AssociationBudget {
                checks_left: if policy.complete {
                    usize::MAX
                } else {
                    policy.max_association_checks
                },
                pixels_left: if policy.complete {
                    usize::MAX
                } else {
                    policy.max_association_pixels
                },
            },
            reuse: ReuseBudget {
                remaining: policy.max_association_checks,
                pixels_remaining: 262_144,
                ..Default::default()
            },
            used: 0,
        }
    }
}
fn claims(
    im: ImageView<'_>,
    outputs: &mut [Candidate],
    reuse_budget: &mut ReuseBudget,
) -> Vec<Quad> {
    let mut w = Work::default();
    let claims = verified_claims(outputs, reuse_budget, &mut w, im);
    if let Some(c) = outputs.first_mut() {
        c.work.extension_cache_hits += w.extension_cache_hits;
        c.work.extension_samples += w.extension_samples;
        c.work.extension_claims += w.extension_claims;
        c.work.extension_capped += w.extension_capped;
        c.work.reuse_claims += w.reuse_claims;
        c.work.reuse_claims_rejected += w.reuse_claims_rejected;
        c.work.reuse_checks += w.reuse_checks;
        c.work.reuse_checks_capped += w.reuse_checks_capped;
    }
    claims
}
impl Experiment {
    pub(super) fn execute_policy(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
        scaled: bool,
    ) -> Result<Vec<Candidate>, Error> {
        if candidates.len() > 64
            || policy.max_retry_paths_per_candidate > 4096
            || policy.max_retry_paths_per_frame > 65536
            || policy.max_association_checks > 2_000_000
            || policy.max_association_pixels > 16_000_000
            || !(1..=4096).contains(&policy.max_results)
        {
            return Err(Error::Parameters);
        }

        let mut run = Execution::new(policy);
        let mut outputs = self.initial_pass(im, candidates, &mut run.association);
        self.discover(im, &mut outputs, &mut run, scaled);
        #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
        for c in &mut outputs {
            c.work.structural_discovery_complete = true;
        }
        let verified = claims(im, &mut outputs, &mut run.reuse);
        let refresh = select_effort(im, &mut outputs, &mut run, scaled);
        let plans = prepare_retries(&mut outputs, &mut run, scaled, &verified);
        self.execute_retries(im, &mut outputs, &mut run, &plans, refresh);
        let verified = claims(im, &mut outputs, &mut run.reuse);
        let confirmation = prepare_confirmation(&mut outputs, &mut run, &plans, &verified);
        self.execute_confirmation(im, &mut outputs, &mut run, &plans, &confirmation);
        assemble(im, &mut outputs, &mut run, &plans);
        #[cfg(feature = "mode-very-high")]
        self.rescue_phases(im, &mut outputs, &mut run, &plans);
        Ok(outputs)
    }
    fn initial_pass(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        budget: &mut experiment::AssociationBudget,
    ) -> Vec<Candidate> {
        #[cfg(feature = "mode-low")]
        let outputs = self.scan_with_budget_axes(
            im,
            candidates,
            experiment::MULTI_FIXED.with_three_rows(),
            budget,
            true,
        );
        #[cfg(feature = "mode-medium")]
        let outputs = self.scan_with_budget(
            im,
            candidates,
            experiment::MULTI_FIXED.with_three_rows(),
            budget,
        );
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let outputs = self.scan_with_budget(im, candidates, experiment::MULTI_FIXED, budget);

        outputs
    }
    fn discover(
        &mut self,
        im: ImageView<'_>,
        outputs: &mut [Candidate],
        run: &mut Execution,
        scaled: bool,
    ) {
        #[cfg(feature = "mode-very-high")]
        let early_claims = if run.policy.max_association_checks < 10_000
            || run.policy.max_association_pixels < 10_000
        {
            Vec::new()
        } else {
            claims(im, outputs, &mut run.reuse)
        };
        #[cfg(not(feature = "mode-very-high"))]
        let early_claims = Vec::new();
        // Fixed-pass coverage accounting also belongs to unscaled scans.
        if !scaled {
            return;
        }
        // Round-robin discovery completes before selecting effort.
        for step in 0..10 {
            for candidate in outputs.iter_mut() {
                self.discover_candidate(im, candidate, run, step, &early_claims);
            }
        }
    }
    fn discover_candidate(
        &mut self,
        im: ImageView<'_>,
        c: &mut Candidate,
        run: &mut Execution,
        i: usize,
        early_claims: &[Quad],
    ) {
        let policy = run.policy;
        let used = &mut run.used;
        #[cfg(feature = "mode-very-high")]
        let reuse_budget = &mut run.reuse;
        #[cfg(not(feature = "mode-very-high"))]
        let _ = early_claims;
        #[cfg(feature = "mode-low")]
        let full = c
            .coverage
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

        // Localizer quads identify the module axis. Fixed discovery still covers
        // both axes; retain both native axes for full frames or contrary evidence.
        #[cfg(feature = "mode-low")]
        {
            if i % 2 == 1 && !full && !c.observations.iter().any(|o| o.axis == 1 && !o.ambiguous) {
                return;
            }
        }
        #[cfg(feature = "mode-very-high")]
        {
            c.work.discovery_requests += 1;
        }
        if (!policy.complete && *used >= policy.max_retry_paths_per_frame)
            || c.work.retry_paths >= policy.max_retry_paths_per_candidate
        {
            c.work.retry_paths_pending += 1;
            return;
        }
        let Ok(m) = scan::transform(c.coverage) else {
            return;
        };
        let Some(segment) = discovery_segment(m.0, i) else {
            c.error = true;
            return;
        };
        #[cfg(any(feature = "mode-medium", feature = "mode-high"))]
        let full = false;
        #[cfg(not(feature = "mode-very-high"))]
        self.retry_segment(
            im,
            m.0,
            segment,
            c,
            policy.transition_cleanup && !full,
            policy.interior_normalization && !full,
            policy.guard_bias,
        );
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        {
            *used += 1;
        }
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        {
            c.work.discovery_paths += 1;
        }
        #[cfg(feature = "mode-very-high")]
        {
            c.work.retry_paths_pending += 1;
        }
        #[cfg(feature = "mode-very-high")]
        let segments = if i == 2 || i == 3 || i == 6 || i == 7 {
            vec![segment]
        } else {
            reuse_plan(m.0, vec![segment], early_claims, reuse_budget, &mut c.work)
        };

        #[cfg(feature = "mode-very-high")]
        {
            for segment in segments {
                if (!policy.complete && *used >= policy.max_retry_paths_per_frame)
                    || c.work.retry_paths >= policy.max_retry_paths_per_candidate
                {
                    continue;
                }
                c.work.retry_paths_pending = c.work.retry_paths_pending.saturating_sub(1);
                self.retry_segment(
                    im,
                    m.0,
                    segment,
                    c,
                    policy.transition_cleanup,
                    policy.interior_normalization,
                    policy.guard_bias,
                );
                *used += 1;
                c.work.discovery_paths += 1;
            }
        }
    }
    fn execute_retries(
        &mut self,
        im: ImageView<'_>,
        outputs: &mut [Candidate],
        run: &mut Execution,
        plans: &[PreparedPlan],
        refresh: bool,
    ) {
        let policy = run.policy;
        let used = &mut run.used;
        #[cfg(feature = "mode-medium")]
        let budget = &mut run.association;
        #[cfg(not(feature = "mode-medium"))]
        let _ = refresh;
        for step in 0..policy.max_retry_paths_per_candidate {
            if !policy.complete && *used >= policy.max_retry_paths_per_frame {
                break;
            }
            for (c, p) in outputs.iter_mut().zip(plans) {
                if !policy.complete && *used >= policy.max_retry_paths_per_frame {
                    break;
                }
                let Some((m, paths)) = p else { continue };
                let Some(s) = paths.get(step) else { continue };
                {
                    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                    {
                        if c.work.retry_paths >= c.work.selected_retry_limit {
                            continue;
                        }
                    }
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    {
                        if c.work.retry_paths >= policy.max_retry_paths_per_candidate {
                            continue;
                        }
                    }
                }

                *used += 1;
                c.work.retry_paths_pending -= 1;
                self.retry_segment(
                    im,
                    *m,
                    *s,
                    c,
                    policy.transition_cleanup,
                    policy.interior_normalization,
                    policy.guard_bias,
                );
            }

            // Refresh effort only after every candidate has received the same
            // retry prefix. Strict assembly selects effort; final assembly still
            // determines returned reads. Association work shares the frame budget.
            #[cfg(feature = "mode-medium")]
            {
                if refresh && matches!(step, 31 | 95) {
                    for (candidate, plan) in outputs.iter_mut().zip(plans) {
                        if candidate.work.selected_retry_limit <= 128
                            || candidate.observations.len() < 4
                            || budget.checks_left < 2_000
                            || budget.pixels_left < 131_072
                        {
                            continue;
                        }
                        let Some((transform, _)) = plan else { continue };
                        let mut probe = experiment::AssociationBudget {
                            checks_left: 2_000,
                            pixels_left: 131_072,
                        };
                        let mut work = Work::default();
                        let reads = experiment::assemble_many_budget_options(
                            im,
                            *transform,
                            &candidate.observations,
                            &mut work,
                            true,
                            &mut probe,
                            false,
                        );
                        let checks_used = 2_000 - probe.checks_left;
                        let pixels_used = 131_072 - probe.pixels_left;
                        budget.checks_left -= checks_used;
                        budget.pixels_left -= pixels_used;
                        candidate.work.association_checks += checks_used;
                        candidate.work.continuity_samples += work.continuity_samples;
                        if work.association_truncated == 0
                            && reads.iter().any(|read| read.support >= 4)
                            && !multiple_initial(reads.iter())
                        {
                            candidate.work.selected_retry_limit = 128;
                        }
                    }
                }
            }
        }
    }
    fn execute_confirmation(
        &mut self,
        im: ImageView<'_>,
        outputs: &mut [Candidate],
        run: &mut Execution,
        plans: &[PreparedPlan],
        confirmation: &[Vec<Segment>],
    ) {
        let policy = run.policy;
        let used = &mut run.used;
        for step in 0..policy.max_retry_paths_per_candidate {
            if !policy.complete && *used >= policy.max_retry_paths_per_frame {
                break;
            }
            for ((c, p), extra) in outputs.iter_mut().zip(plans).zip(confirmation) {
                if !policy.complete && *used >= policy.max_retry_paths_per_frame {
                    break;
                }
                {
                    #[cfg(any(
                        feature = "mode-low",
                        feature = "mode-high",
                        feature = "mode-very-high"
                    ))]
                    {
                        if c.work.retry_paths >= policy.max_retry_paths_per_candidate {
                            continue;
                        }
                    }
                    #[cfg(feature = "mode-medium")]
                    {
                        if c.work.retry_paths >= c.work.selected_retry_limit {
                            continue;
                        }
                    }
                }

                let (Some((m, _)), Some(s)) = (p, extra.get(step)) else {
                    continue;
                };
                *used += 1;
                c.work.retry_paths_pending -= 1;
                self.retry_segment(
                    im,
                    *m,
                    *s,
                    c,
                    policy.transition_cleanup,
                    policy.interior_normalization,
                    policy.guard_bias,
                );
            }
        }
    }
    #[cfg(feature = "mode-very-high")]
    fn rescue_phases(
        &mut self,
        im: ImageView<'_>,
        outputs: &mut [Candidate],
        run: &mut Execution,
        plans: &[PreparedPlan],
    ) {
        let policy = run.policy;
        let used = &mut run.used;
        let budget = &mut run.association;
        // Different sampling phases of a source row are not independent
        // row votes. Existing canonical-row grouping and source checks apply.
        #[cfg(feature = "mode-very-high")]
        {
            for (c, p) in outputs.iter_mut().zip(plans) {
                if c.error
                    || c.work.association_truncated > 0
                    || !c.detections.is_empty()
                    || c.work.max_run_count < 30
                {
                    continue;
                }
                let Some((m, _)) = p else { continue };
                let before = c.work.phase_rescue_paths;
                for axis in 0..2 {
                    let (Ok(a), Ok(b)) = (
                        experiment::point(*m, axis, 0., 0.5),
                        experiment::point(*m, axis, 1., 0.5),
                    ) else {
                        continue;
                    };
                    let width = experiment::distance(a, b);
                    if !(76. ..=384.).contains(&width) {
                        continue;
                    }
                    c.work.retry_paths_pending += 10;
                    for fraction in [0.1, 0.3, 0.5, 0.7, 0.9] {
                        for phase in [-0.5, 0.5] {
                            if (!policy.complete && *used >= policy.max_retry_paths_per_frame)
                                || c.work.retry_paths >= policy.max_retry_paths_per_candidate
                            {
                                continue;
                            }
                            let delta = phase / width;
                            let lo = -0.15 + delta;
                            let hi = 1.15 + delta;
                            let (Ok(a), Ok(b)) = (
                                experiment::point(*m, axis, lo, fraction),
                                experiment::point(*m, axis, hi, fraction),
                            ) else {
                                c.error = true;
                                continue;
                            };
                            let length = experiment::distance(a, b);
                            let seg = Segment {
                                axis,
                                fraction,
                                lo,
                                hi,
                                samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
                                sample_cap: length > 4096.,
                                unresolved: false,
                            };
                            *used += 1;
                            c.work.retry_paths_pending -= 1;
                            c.work.phase_rescue_paths += 1;
                            self.retry_segment(
                                im,
                                *m,
                                seg,
                                c,
                                policy.transition_cleanup,
                                policy.interior_normalization,
                                policy.guard_bias,
                            );
                        }
                    }
                }
                if c.work.phase_rescue_paths > before {
                    c.detections = experiment::assemble_many_budget_options(
                        im,
                        *m,
                        &c.observations,
                        &mut c.work,
                        true,
                        budget,
                        policy.allow_single_row,
                    );
                }
            }
        }
    }
}
fn select_effort(
    im: ImageView<'_>,
    outputs: &mut [Candidate],
    run: &mut Execution,
    scaled: bool,
) -> bool {
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let policy = run.policy;
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let budget = &mut run.association;
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    let _ = (im, outputs, run, scaled);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let discovery_reads = probe_discovery(im, outputs, policy, budget, scaled);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let strong = outputs
        .iter()
        .any(|c| c.detections.iter().any(|d| d.support >= 2))
        || discovery_reads.iter().any(|d| d.support >= 2);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let multiple = multiple_initial(
        outputs
            .iter()
            .flat_map(|c| c.detections.iter())
            .chain(discovery_reads.iter()),
    );
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let confident = outputs
        .iter()
        .any(|c| c.detections.iter().any(|d| d.support >= 4))
        || discovery_reads.iter().any(|d| d.support >= 4);
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let cap = if multiple {
        192
    } else if confident {
        10
    } else if strong {
        16
    } else {
        {
            #[cfg(feature = "mode-low")]
            {
                64
            }
            #[cfg(feature = "mode-medium")]
            {
                512
            }
        }
    };
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    let policy = Policy {
        max_retry_paths_per_candidate: policy.max_retry_paths_per_candidate.min(cap),
        ..policy
    };
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    {
        for c in outputs.iter_mut() {
            #[cfg(feature = "mode-low")]
            {
                c.work.selected_retry_limit = if confident && !multiple {
                    c.work
                        .discovery_paths
                        .min(policy.max_retry_paths_per_candidate)
                } else {
                    policy.max_retry_paths_per_candidate
                };
            }
            #[cfg(feature = "mode-medium")]
            let dimensions = u32::try_from(im.width.saturating_sub(1))
                .ok()
                .zip(u32::try_from(im.height.saturating_sub(1)).ok());
            #[cfg(feature = "mode-medium")]
            let broad = dimensions.is_some_and(|(width, height)| {
                let (right, bottom) = (f64::from(width), f64::from(height));
                let full = [[0., 0.], [right, 0.], [right, bottom], [0., bottom]];
                c.coverage
                    .iter()
                    .flatten()
                    .zip(full.iter().flatten())
                    .all(|(a, b)| (a - b).abs() <= f64::EPSILON)
            });
            #[cfg(feature = "mode-medium")]
            {
                c.work.selected_retry_limit = if !multiple && !strong && !broad {
                    let evidence_cap = if c.work.guard_pass > 0 { 512 } else { 64 };
                    policy.max_retry_paths_per_candidate.min(evidence_cap)
                } else {
                    policy.max_retry_paths_per_candidate
                };
            }
        }
    }
    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
    {
        run.policy = policy;
        !multiple && !strong
    }
    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
    {
        false
    }
}
fn prepare_retries(
    outputs: &mut [Candidate],
    run: &mut Execution,
    scaled: bool,
    claims: &[Quad],
) -> Vec<PreparedPlan> {
    let policy = run.policy;
    let reuse_budget = &mut run.reuse;
    let mut plans = Vec::with_capacity(outputs.len());
    for c in outputs.iter_mut() {
        #[cfg(feature = "mode-medium")]
        let policy = Policy {
            max_retry_paths_per_candidate: c.work.selected_retry_limit,
            ..policy
        };
        let start = Timer::now();
        let p = match scan::transform(c.coverage) {
            Ok(m) => {
                if let Ok(p) = (|| -> Result<Vec<Segment>, Error> {
                    let width = if scaled {
                        supported_scale_width(m.0, c)
                    } else {
                        None
                    };
                    c.work.scale_hint_used = usize::from(width.is_some());
                    #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                    let reserve = confirmation_reserve(policy.max_retry_paths_per_candidate);
                    #[cfg(feature = "mode-low")]
                    let remaining = Policy {
                        max_retry_paths_per_candidate: c
                            .work
                            .selected_retry_limit
                            .saturating_sub(c.work.retry_paths)
                            .saturating_sub(reserve),
                        ..policy
                    };
                    #[cfg(feature = "mode-medium")]
                    let remaining = Policy {
                        max_retry_paths_per_candidate: policy
                            .max_retry_paths_per_candidate
                            .saturating_sub(c.work.retry_paths)
                            .saturating_sub(reserve),
                        ..policy
                    };
                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                    let remaining = Policy {
                        max_retry_paths_per_candidate: policy
                            .max_retry_paths_per_candidate
                            .saturating_sub(c.work.retry_paths),
                        ..policy
                    };

                    // A visual width may guide tile size and retain its existing work allowance,
                    // but one physical row must not reduce independent-row coverage.
                    let dense = scaled
                        && width.is_some()
                        && supported_scale_width_checked(m.0, c, true).is_none();
                    let mut p =
                        scaled_plan_density(m.0, remaining, &mut c.work, width, scaled, dense)?;
                    if scaled && width.is_some_and(|w| w > 288.) {
                        let mut fine = unresolved_plan(
                            m.0,
                            &c.detections,
                            remaining.max_retry_paths_per_candidate,
                            &mut c.work,
                        )?;
                        fine.extend(
                            p.into_iter().take(
                                remaining
                                    .max_retry_paths_per_candidate
                                    .saturating_sub(fine.len()),
                            ),
                        );
                        p = fine;
                    }

                    let p = reuse_plan(m.0, p, claims, reuse_budget, &mut c.work);
                    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
                    let p = defer_structurally_weak_retries(c, scaled, p);
                    #[cfg(any(
                        feature = "mode-medium",
                        feature = "mode-high",
                        feature = "mode-very-high"
                    ))]
                    let p = if policy.candidate_retry_mask & (1_u64 << c.index) == 0
                        && c.work.accepted_paths == 0
                    {
                        Vec::new()
                    } else {
                        p
                    };
                    Ok(p)
                })() {
                    Some((m.0, p))
                } else {
                    c.error = true;
                    None
                }
            }
            Err(_) => None,
        };
        plans.push(p);
        c.ms += start.ms();
    }
    plans
}
fn prepare_confirmation(
    outputs: &mut [Candidate],
    run: &mut Execution,
    plans: &[PreparedPlan],
    claims: &[Quad],
) -> Vec<Vec<Segment>> {
    let budget = &run.association;
    let reuse_budget = &mut run.reuse;
    let mut confirmation = Vec::with_capacity(outputs.len());
    let mut confirmation_allowance = budget.checks_left;
    for (c, p) in outputs.iter_mut().zip(plans) {
        let mut extra: Vec<Segment> = Vec::new();
        // At tiny association budgets extra evidence would exhaust the
        // existing assembly rather than improve it. Use a conservative planning
        // allowance for the extra probes; actual assembly retains hard caps.
        if let Some((m, _)) = p
            .as_ref()
            .filter(|_| confirmation_allowance >= (c.observations.len() + 64).saturating_pow(2))
        {
            confirmation_allowance = confirmation_allowance
                .saturating_sub((c.observations.len() + 64).saturating_pow(2));
            let mut anchors: Vec<_> = c
                .observations
                .iter()
                .copied()
                .filter(|o| !o.ambiguous)
                .collect();
            anchors.sort_by(|a, b| a.cost.total_cmp(&b.cost));
            for o in anchors {
                let (Ok(a), Ok(b), Ok(top), Ok(bottom)) = (
                    experiment::point(*m, o.axis, o.left, o.fraction),
                    experiment::point(*m, o.axis, o.right, o.fraction),
                    experiment::point(*m, o.axis, 0.5, 0.),
                    experiment::point(*m, o.axis, 0.5, 1.),
                ) else {
                    continue;
                };
                let cross = experiment::distance(top, bottom);
                if cross < 1. {
                    continue;
                }
                let width = o.right - o.left;
                if width <= 0. {
                    continue;
                }
                let lo = o.left - 0.15 * width;
                let hi = o.right + 0.15 * width;
                let length = experiment::distance(a, b) * 1.3;
                for delta in [-1., 1., -2., 2.] {
                    let fraction = o.fraction + delta / cross;
                    if !(0.0..=1.0).contains(&fraction) {
                        continue;
                    }
                    if extra.iter().any(|s| {
                        s.axis == o.axis
                            && (s.fraction - fraction).abs() * cross < 0.5
                            && (s.lo - lo).abs() < 0.1 * width
                            && (s.hi - hi).abs() < 0.1 * width
                    }) {
                        continue;
                    }
                    if extra.len() >= 64 {
                        c.work.sampling_plan_capped += 1;
                        break;
                    }
                    extra.push(Segment {
                        axis: o.axis,
                        fraction,
                        lo,
                        hi,
                        samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
                        sample_cap: length > 4096.,
                        unresolved: false,
                    });
                }
                if extra.len() >= 64 {
                    break;
                }
            }
        }
        c.work.retry_paths_pending += extra.len();

        let extra = if let Some((m, _)) = p {
            reuse_plan(*m, extra, claims, reuse_budget, &mut c.work)
        } else {
            extra
        };
        confirmation.push(extra);
    }
    confirmation
}
fn assemble(
    im: ImageView<'_>,
    outputs: &mut [Candidate],
    run: &mut Execution,
    plans: &[PreparedPlan],
) {
    let policy = run.policy;
    let budget = &mut run.association;
    for (c, p) in outputs.iter_mut().zip(plans) {
        if let Some((m, _)) = p {
            if c.work.retry_paths > 0 {
                let start = Timer::now();
                let refined = experiment::assemble_many_budget_options(
                    im,
                    *m,
                    &c.observations,
                    &mut c.work,
                    true,
                    budget,
                    policy.allow_single_row,
                );
                if c.work.association_truncated > 0 && !c.detections.is_empty() {
                    c.work.retained_initial_detections = c.detections.len();
                } else {
                    c.detections = refined;
                }
                // Frame reconciliation withholds every exhausted candidate,
                // including these raw partial detections when initial was empty.
                c.ms += start.ms();
            }
        }
    }
}

fn discovery_segment(transform: [f64; 9], step: usize) -> Option<Segment> {
    let axis = step % 2;
    let fraction = 0.1 + 0.2 * crate::numeric::usize_f64(step / 2);
    let a = experiment::point(transform, axis, -0.15, fraction).ok()?;
    let b = experiment::point(transform, axis, 1.15, fraction).ok()?;
    let length = experiment::distance(a, b);
    Some(Segment {
        axis,
        fraction,
        lo: -0.15,
        hi: 1.15,
        samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
        sample_cap: length > 4096.,
        unresolved: false,
    })
}

#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
fn probe_discovery(
    im: ImageView<'_>,
    outputs: &mut [Candidate],
    policy: Policy,
    budget: &mut experiment::AssociationBudget,
    scaled: bool,
) -> Vec<experiment::Detection> {
    // Decide effort only after every candidate has received fixed and
    // native discovery. Never return early on a successful read. Retain
    // all candidates and explicit pending work on lower-effort frames.
    // Native discovery adds observations after the fixed-pass detections
    // were assembled. Use bounded, strict assembly only to choose effort;
    // final returned detections still go through the ordinary final assembly.
    // These probes spend the same frame association budget, not a hidden one.
    let mut discovery_reads = Vec::new();
    let mut probe_checks = budget.checks_left.min(policy.max_association_checks / 10);
    let mut probe_pixels = budget.pixels_left.min(policy.max_association_pixels / 10);
    {
        if scaled && budget.checks_left >= 10_000 && budget.pixels_left >= 10_000 {
            for c in outputs.iter_mut() {
                if c.observations.len() < 2 || probe_checks == 0 || probe_pixels == 0 {
                    continue;
                }
                let Ok(m) = scan::transform(c.coverage) else {
                    continue;
                };
                #[cfg(feature = "mode-low")]
                let checks = probe_checks.min(2000);
                #[cfg(feature = "mode-medium")]
                let checks = probe_checks.min(2_000);

                let pixels = probe_pixels.min(32768);
                let mut small = experiment::AssociationBudget {
                    checks_left: checks,
                    pixels_left: pixels,
                };
                let mut work = Work::default();
                let reads = experiment::assemble_many_budget_options(
                    im,
                    m.0,
                    &c.observations,
                    &mut work,
                    true,
                    &mut small,
                    false,
                );
                let used_checks = checks - small.checks_left;
                let used_pixels = pixels - small.pixels_left;
                probe_checks -= used_checks;
                probe_pixels -= used_pixels;
                budget.checks_left -= used_checks;
                budget.pixels_left -= used_pixels;
                c.work.association_checks += used_checks;
                c.work.continuity_samples += work.continuity_samples;
                if work.association_truncated == 0 {
                    discovery_reads.extend(reads);
                }
            }
        }
    }
    discovery_reads
}

#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
fn confirmation_reserve(limit: usize) -> usize {
    match limit {
        129..=256 => 16,
        24..=128 => 8,
        16..24 => 4,
        _ => 0,
    }
}

#[cfg(all(test, feature = "mode-very-high"))]
mod tests {
    use super::*;

    #[test]
    fn unscaled_paths_keep_fixed_pass_coverage_accounting() {
        let pixels = vec![255; 64 * 64];
        let image = ImageView::new(&pixels, 64, 64, 1, 64).unwrap();
        let quad = [[4., 4.], [60., 4.], [60., 60.], [4., 60.]];
        for (checks, expected_claims) in [(0, 0), (100_000, 1)] {
            let mut candidates = [Candidate {
                index: 0,
                coverage: quad,
                observations: Vec::new(),
                detections: vec![experiment::Detection {
                    digits: [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1],
                    polygon: quad,
                    support: 2,
                    axis: 0,
                }],
                work: Work::default(),
                ms: 0.,
                error: false,
            }];
            let mut run = Execution::new(Policy {
                max_association_checks: checks,
                ..Policy::default()
            });
            Experiment::default().discover(image, &mut candidates, &mut run, false);
            assert_eq!(candidates[0].work.discovery_paths, 0);
            assert_eq!(candidates[0].work.reuse_claims, expected_claims);
        }
    }
}

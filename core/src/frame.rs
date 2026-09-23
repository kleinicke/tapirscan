//! Frame-level reconciliation; original candidate evidence remains untouched.
#![forbid(unsafe_code)]
mod identity;
pub(crate) use identity::*;
mod conflict;
use crate::{
    experiment::{AssociationBudget, Candidate, CandidateScanner, Detection, Work},
    multi_scan::Policy,
    sampling::{Error, ImageView},
    scan::Quad,
};
use conflict::same_text_identity;
#[cfg(feature = "mode-very-high")]
use conflict::source_conflict_winner;
#[cfg(feature = "mode-very-high")]
use conflict::source_pair_winner;
#[derive(Debug)]
pub struct Barcode {
    pub detection: Detection,
    pub candidate_indices: Vec<usize>,
}
#[derive(Debug, Default)]
pub struct ReconciliationWork {
    pub comparisons: usize,
    pub merged: usize,
    pub ambiguous: usize,
    pub conflicting: usize,
    pub pending_observations: usize,
    pub truncated: bool,
    pub source_pairs: usize,
    pub source_matches: usize,
    pub source_pixels: usize,
    pub source_capped: usize,
    #[cfg(feature = "mode-very-high")]
    pub source_cache_hits: usize,
    pub optional_identity_deferred: usize,
    pub pending_coverage_checks: usize,
    pub pending_quarantined: usize,
}
#[derive(Debug)]
pub struct Frame {
    pub candidates: Vec<Candidate>,
    pub barcodes: Vec<Barcode>,
    pub reconciliation: ReconciliationWork,
    pub unfinished: bool,
}

// Equal reads on crossing paths may have narrow, poorly overlapping polygons.
// Require aligned scale and the same interior module position at intersection.

// Conservative, constant-size envelope check for mandatory pending quarantine.
// At most64pending envelopes per group member; separate from optional matching.

struct Group {
    members: Vec<(usize, usize)>,
    conflicted: bool,
}
impl CandidateScanner {
    /// Primary frame surface reconciles physical reads while retaining all raw
    /// candidate results. Resource exhaustion returns a flagged partial frame.
    /// # Errors
    /// Returns `Parameters` for invalid scan policy or too many candidates; propagates scanner setup errors.
    pub fn scan_frame(
        &mut self,
        im: ImageView<'_>,
        candidates: &[Quad],
        policy: Policy,
    ) -> Result<Frame, Error> {
        let candidates = self.scan_scaled(im, candidates, policy)?;
        Ok(reconcile_image(Some(im), candidates, policy))
    }
}
#[cfg(test)]
fn reconcile(candidates: Vec<Candidate>, policy: Policy) -> Frame {
    reconcile_image(None, candidates, policy)
}

// Called only inside existing equal-text guards. An incomplete optional
// merge proof cannot invalidate already accepted bands.

// Called only inside existing equal-text guards. An incomplete optional
// merge proof cannot invalidate already accepted bands.

// Only resolve an existing broad-versus-thin conflict through fresh source pixels.
// A support count alone never selects a value. Inconclusive probes retain the veto.

// Every fresh row in both decoded bands must prefer the same hypothesis.
// No new text is emitted; inconclusive/budget-limited comparisons retain evidence.

fn remaining_budget(candidates: &[Candidate], policy: Policy) -> AssociationBudget {
    let used_checks: usize = candidates.iter().map(|c| c.work.association_checks).sum();
    AssociationBudget {
        checks_left: if policy.complete {
            usize::MAX
        } else {
            policy.max_association_checks.saturating_sub(used_checks)
        },
        pixels_left: if policy.complete {
            usize::MAX
        } else {
            policy
                .max_association_pixels
                .saturating_sub(candidates.iter().map(|c| c.work.continuity_samples).sum())
        },
    }
}

fn ordered_entries(candidates: &[Candidate]) -> Vec<(usize, usize)> {
    let mut entries: Vec<_> = candidates
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| {
            (0..if c.work.association_truncated > 0 || c.work.retained_initial_detections > 0 {
                0
            } else {
                c.detections.len()
            })
                .map(move |di| (ci, di))
        })
        .collect();

    // Small observations precede broad ones, so a later broad fit cannot fuse
    // two already-distinct equal-value instances through transitive grouping.
    entries.sort_by(|&(a, b), &(c, d)| {
        signed_area(&candidates[a].detections[b].polygon)
            .abs()
            .total_cmp(&signed_area(&candidates[c].detections[d].polygon).abs())
            .then(a.cmp(&c))
            .then(b.cmp(&d))
    });

    entries
}

#[cfg(feature = "mode-very-high")]
fn reject_source_aliases(
    im: Option<ImageView<'_>>,
    candidates: &[Candidate],
    mut entries: Vec<(usize, usize)>,
    budget: &mut AssociationBudget,
    counter: &mut Work,
) -> Vec<(usize, usize)> {
    // Finite source-validated rejection; preserve all raw candidate evidence.
    if let Some(im) = im.filter(|_| {
        entries.first().is_some_and(|&(a, b)| {
            entries.iter().any(|&(c, d)| {
                candidates[c].detections[d].digits != candidates[a].detections[b].digits
            })
        })
    }) {
        let mut rejected = vec![false; entries.len()];
        let mut probes = 0;
        'outer: for i in 0..entries.len() {
            for j in 0..entries.len() {
                if i == j || rejected[i] || rejected[j] {
                    continue;
                }
                if budget.checks_left == 0 || probes >= 32 {
                    break 'outer;
                }
                budget.checks_left -= 1;
                counter.association_checks += 1;
                let (a, b) = entries[i];
                let (c, d) = entries[j];
                let thin = &candidates[a].detections[b];
                let broad = &candidates[c].detections[d];
                if thin.digits == broad.digits {
                    continue;
                }
                let overlap = same_space(thin.polygon, broad.polygon);
                if !overlap && crate::identity::gap_edges(thin.polygon, broad.polygon).is_none() {
                    continue;
                }
                probes += 1;
                if (overlap && source_conflict_winner(im, broad, thin, budget, counter))
                    || (probes <= 4 && source_pair_winner(im, broad, thin, budget, counter))
                {
                    rejected[i] = true;
                }
            }
        }
        entries = entries
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| (!rejected[i]).then_some(e))
            .collect();
    }
    entries
}

#[expect(
    clippy::too_many_lines,
    reason = "Frame reconciliation retains raw candidates while applying identity checks and ranking in their established order."
)]
fn reconcile_image(im: Option<ImageView<'_>>, candidates: Vec<Candidate>, policy: Policy) -> Frame {
    let mut budget = remaining_budget(&candidates, policy);
    let mut counter = Work::default();
    let mut work = ReconciliationWork::default();
    #[cfg(feature = "mode-very-high")]
    let mut identity_cache = crate::identity::IdentityCache::default();
    work.pending_observations = candidates
        .iter()
        .map(|c| {
            if c.work.association_truncated > 0 {
                c.detections.len().max(c.work.retained_initial_detections)
            } else {
                c.work.retained_initial_detections
            }
        })
        .sum();
    let candidate_pending = work.pending_observations;
    let pending_envelopes: Vec<_> = candidates
        .iter()
        .filter(|c| c.work.association_truncated > 0 || c.work.retained_initial_detections > 0)
        .map(|c| envelope(c.coverage))
        .collect();
    let entries = ordered_entries(&candidates);

    #[cfg(feature = "mode-very-high")]
    let entries = reject_source_aliases(im, &candidates, entries, &mut budget, &mut counter);
    let mut groups: Vec<Group> = vec![];
    'observations: for (position, &(ci, di)) in entries.iter().enumerate() {
        let d = &candidates[ci].detections[di];
        let mut matches = vec![];
        let mut complete = true;
        let mut conflict = false;
        for (gi, g) in groups.iter_mut().enumerate() {
            let mut any = false;
            let mut all = true;
            let mut differs = false;
            for &(mi, mj) in &g.members {
                if !budget.check(&mut counter) {
                    work.pending_observations += entries.len() - position;
                    break 'observations;
                }
                let member = &candidates[mi].detections[mj];
                let mut overlap = same_space(d.polygon, member.polygon);
                if !overlap
                    && policy.source_identity
                    && d.digits == member.digits
                    && crossing_read_paths(d.polygon, member.polygon)
                {
                    if let Some(im) = im {
                        let ends = |q: Quad| {
                            [
                                [0.5 * (q[0][0] + q[3][0]), 0.5 * (q[0][1] + q[3][1])],
                                [0.5 * (q[1][0] + q[2][0]), 0.5 * (q[1][1] + q[2][1])],
                            ]
                        };
                        let a = ends(d.polygon);
                        let mut b = ends(member.polygon);
                        if (a[1][0] - a[0][0]) * (b[1][0] - b[0][0])
                            + (a[1][1] - a[0][1]) * (b[1][1] - b[0][1])
                            < 0.
                        {
                            b.reverse();
                        }
                        work.source_pairs += 1;
                        {
                            #[cfg(any(
                                feature = "mode-low",
                                feature = "mode-medium",
                                feature = "mode-high"
                            ))]
                            {
                                if same_text_identity(
                                    im,
                                    a,
                                    b,
                                    &mut budget,
                                    &mut counter,
                                    &mut work,
                                ) {
                                    overlap = true;
                                    work.source_matches += 1;
                                }
                            }
                            #[cfg(feature = "mode-very-high")]
                            {
                                if same_text_identity(
                                    im,
                                    a,
                                    b,
                                    &mut budget,
                                    &mut counter,
                                    &mut work,
                                    &mut identity_cache,
                                ) {
                                    overlap = true;
                                    work.source_matches += 1;
                                }
                            }
                        }
                    }
                }
                if !overlap
                    && policy.source_identity
                    && (ci != mi || d.axis == member.axis)
                    && d.digits == member.digits
                    && !g.conflicted
                {
                    // Across proposals, require overlapping bands by default. The optional
                    // shared-coverage path still requires full source continuity.
                    let partial_overlap = intersection(d.polygon, member.polygon) > 0.;
                    let shared_coverage = ci != mi
                        && intersection(candidates[ci].coverage, candidates[mi].coverage) > 0.;
                    if ci == mi || partial_overlap || shared_coverage {
                        if let (Some(im), Some((a, b))) =
                            (im, crate::identity::gap_edges(d.polygon, member.polygon))
                        {
                            let mut barrier = false;
                            let mut contradiction = false;
                            for source in [Some(ci), if ci == mi { None } else { Some(mi) }]
                                .into_iter()
                                .flatten()
                            {
                                let Ok(m) = crate::scan::transform(candidates[source].coverage)
                                else {
                                    contradiction = true;
                                    break;
                                };
                                let source_axis = if source == ci { d.axis } else { member.axis };
                                for o in candidates[source].observations.iter().filter(|o| {
                                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                    {
                                        #[cfg(any(
                                            feature = "mode-high",
                                            feature = "mode-very-high"
                                        ))]
                                        if o.invalid_checksum {
                                            return false;
                                        }
                                    }
                                    (o.ambiguous && o.axis == source_axis)
                                        || (!o.ambiguous && o.digits != d.digits)
                                }) {
                                    if !budget.check(&mut counter) {
                                        break;
                                    }
                                    if let Ok(p) = crate::experiment::point(
                                        m.0,
                                        o.axis,
                                        (o.left + o.right) * 0.5,
                                        o.fraction,
                                    ) {
                                        if crate::identity::barrier_between(a, b, p) {
                                            if o.ambiguous {
                                                barrier = true;
                                            } else {
                                                contradiction = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                                if contradiction || counter.association_truncated > 0 {
                                    break;
                                }
                            }
                            if (barrier || partial_overlap || shared_coverage)
                                && !contradiction
                                && counter.association_truncated == 0
                            {
                                work.source_pairs += 1;
                                {
                                    #[cfg(any(
                                        feature = "mode-low",
                                        feature = "mode-medium",
                                        feature = "mode-high"
                                    ))]
                                    {
                                        if same_text_identity(
                                            im,
                                            a,
                                            b,
                                            &mut budget,
                                            &mut counter,
                                            &mut work,
                                        ) {
                                            overlap = true;
                                            work.source_matches += 1;
                                        }
                                    }
                                    #[cfg(feature = "mode-very-high")]
                                    {
                                        if same_text_identity(
                                            im,
                                            a,
                                            b,
                                            &mut budget,
                                            &mut counter,
                                            &mut work,
                                            &mut identity_cache,
                                        ) {
                                            overlap = true;
                                            work.source_matches += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                any |= overlap;
                all &= overlap;
                differs |= overlap && d.digits != member.digits;
            }
            if !any {
                continue;
            }
            if g.conflicted || differs {
                g.conflicted = true;
                conflict = true;
                continue;
            }
            matches.push(gi);
            complete &= all;
        }
        if conflict {
            work.conflicting += 1;
            for &gi in &matches {
                groups[gi].conflicted = true;
            }
            groups.push(Group {
                members: vec![(ci, di)],
                conflicted: true,
            });
            continue;
        }
        if matches.len() > 1 || !complete {
            work.ambiguous += 1;
            continue;
        }
        if let Some(&gi) = matches.first() {
            groups[gi].members.push((ci, di));
            work.merged += 1;
        } else {
            groups.push(Group {
                members: vec![(ci, di)],
                conflicted: false,
            });
        }
    }
    #[cfg(feature = "mode-very-high")]
    {
        work.source_cache_hits = identity_cache.hits;
    }
    work.comparisons = counter.association_checks;
    work.source_pixels = counter.continuity_samples;
    work.source_capped = counter.continuity_capped_links;
    work.truncated = counter.association_truncated > 0;

    // An unvisited observation could contradict any accumulated group. Until
    // all conflict checks finish, retain raw evidence but publish no groups.
    if work.truncated {
        groups.clear();
        work.pending_observations = entries.len() + candidate_pending;
    }
    let mut barcodes = Vec::new();
    for g in groups.into_iter().filter(|g| !g.conflicted) {
        let mut quarantined = false;
        for &(ci, di) in &g.members {
            let bounds = envelope(candidates[ci].detections[di].polygon);
            for &pending in &pending_envelopes {
                work.pending_coverage_checks += 1;
                if envelopes_overlap(bounds, pending) {
                    quarantined = true;
                    break;
                }
            }
            if quarantined {
                break;
            }
        }
        if quarantined {
            work.pending_quarantined += g.members.len();
            work.pending_observations += g.members.len();
            continue;
        }

        let &(ci, di) = g
            .members
            .iter()
            .max_by_key(|&&(ci, di)| candidates[ci].detections[di].support)
            .unwrap();
        let mut ids: Vec<_> = g
            .members
            .iter()
            .map(|&(ci, _)| candidates[ci].index)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        barcodes.push(Barcode {
            detection: candidates[ci].detections[di].clone(),
            candidate_indices: ids,
        });
    }

    // Result limits retain strongest support first; stable ties preserve spatial order.
    if barcodes.len() > policy.max_results {
        work.truncated = true;
        barcodes.sort_by(|a, b| b.detection.support.cmp(&a.detection.support));
        barcodes.truncate(policy.max_results);
    }
    let unfinished = work.optional_identity_deferred > 0
        || work.truncated
        || work.source_matches > 0
        || work.ambiguous > 0
        || work.conflicting > 0
        || candidates.iter().any(|c| {
            c.error
                || c.work.invalid_veto_intervals > 0
                || c.work.association_truncated > 0
                || c.work.retry_paths_pending > 0
                || c.work.sampling_plan_capped > 0
                || c.work.capped_paths > 0
                || c.work.truncated_paths > 0
        });
    Frame {
        candidates,
        barcodes,
        reconciliation: work,
        unfinished,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn q(x: f64, y: f64, w: f64, h: f64) -> Quad {
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    }
    fn c(index: usize, polygon: Quad, digits: [u8; 13]) -> Candidate {
        Candidate {
            index,
            coverage: polygon,
            observations: vec![],
            detections: vec![Detection {
                digits,
                polygon,
                support: 2,
                axis: 0,
            }],
            work: Work::default(),
            ms: 0.,
            error: false,
        }
    }

    #[test]
    fn rotated_product389_duplicate_keeps_raw_gap_contradiction() {
        // Frozen008 diagnosis:048 supported-scale policy explores additional rows.
        // Actual competing evidence must veto merging even for equal accepted text.
        let mut candidate = c(
            3,
            [
                [158.314_777_938_640_42, 440.082_728_862_731_4],
                [330.719_876_596_870_73, 290.376_169_662_891_8],
                [376.346_119_958_412_94, 342.920_273_476_344_05],
                [203.941_021_300_182_63, 492.626_832_676_183_65],
            ],
            [8, 8, 5, 8, 1, 2, 1, 0, 0, 0, 4, 8, 3],
        );
        candidate.detections[0].polygon = [
            [191.536_982_678_976_22, 430.049_602_013_270_4],
            [336.426_924_177_610_2, 304.235_604_786_738_5],
            [347.534_340_789_899_66, 316.534_208_287_619_1],
            [202.055_710_992_441_93, 442.859_388_189_976_7],
        ];
        let mut second = candidate.detections[0].clone();
        second.polygon = [
            [205.988_942_253_052_2, 448.221_454_147_727_1],
            [352.048_801_572_055_9, 321.391_568_337_869_05],
            [358.811_109_312_205_9, 329.563_501_960_328_6],
            [213.572_476_108_807_42, 455.680_282_780_548_17],
        ];
        candidate.detections.push(second);
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let observation = crate::experiment::Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [7, 8, 5, 8, 1, 7, 7, 0, 0, 0, 4, 8, 3],
            axis: 0,
            fraction: 0.5,
            left: 0.136_914_062_499_999_96,
            right: 0.979_882_812_499_999_8,
            cost: 0.065_870_23,
            gap: 0.028_946_504,
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let observation = crate::experiment::Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            short_quiet: false,
            ambiguous: false,
            digits: [7, 8, 5, 8, 1, 7, 7, 0, 0, 0, 4, 8, 3],
            axis: 0,
            fraction: 0.5,
            left: 0.136_914_062_499_999_96,
            right: 0.979_882_812_499_999_8,
            cost: 0.065_870_23,
            gap: 0.028_946_504,
        };

        let (a, b) = crate::identity::gap_edges(
            candidate.detections[0].polygon,
            candidate.detections[1].polygon,
        )
        .expect("eligible geometry");
        let m = crate::scan::transform(candidate.coverage).unwrap();
        let pt = crate::experiment::point(
            m.0,
            observation.axis,
            (observation.left + observation.right) * 0.5,
            observation.fraction,
        )
        .unwrap();
        assert!(crate::identity::barrier_between(a, b, pt));
        candidate.observations.push(observation);
        let pixels = vec![255; 1000 * 1000];
        let f = reconcile_image(
            Some(ImageView::new(&pixels, 1000, 1000, 1, 1000).unwrap()),
            vec![candidate],
            Policy {
                source_identity: true,
                ..Policy::default()
            },
        );
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(f.reconciliation.source_pairs, 0);
        assert_eq!(f.reconciliation.source_pixels, 0);
        assert_eq!(f.candidates[0].observations[0].digits, observation.digits);
    }
    #[test]
    fn rotated_real_duplicate_retains_conflicting_gap_observation() {
        // Frozen whole-scene +40degree development case5021047102439_2.
        // Two accepted bands share a candidate, but a different raw read falls
        // between them. The geometry is eligible; contradiction must veto merging.
        let mut candidate = c(
            0,
            [
                [499.341_561_335_391_13, 1_631.622_487_722_129_3],
                [1_365.190_883_379_574_6, 880.648_752_325_499],
                [1_782.499_267_912_159_7, 1_361.551_777_691_788_9],
                [916.649_945_867_976, 2_112.525_513_088_419_3],
            ],
            [5, 0, 2, 1, 0, 4, 7, 1, 0, 2, 4, 3, 9],
        );
        candidate.detections[0].polygon = [
            [524.758_141_214_695_1, 1_624.252_610_767_705],
            [1_294.550_488_135_453_4, 956.591_599_170_446_1],
            [1_560.887_328_873_230_7, 1_287.487_692_090_278_2],
            [814.950_649_192_182_7, 1_934.458_060_314_289_5],
        ];
        let mut second = candidate.detections[0].clone();
        second.polygon = [
            [821.680_978_167_715_7, 1_941.107_270_009_087_9],
            [1_566.263_134_316_768_9, 1_295.311_715_428_289_7],
            [1_573.980_958_430_841_2, 1_307.347_744_707_973_6],
            [831.384_260_323_076_9, 1_951.421_259_663_886_5],
        ];
        candidate.detections.push(second);
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let observation = crate::experiment::Observation {
            short_quiet: false,
            ambiguous: false,
            digits: [7, 0, 2, 5, 6, 4, 7, 1, 0, 2, 4, 3, 9],
            axis: 0,
            fraction: 0.6875,
            left: 0.034_294_564_269_340_74,
            right: 0.896_728_315_959_380_7,
            cost: 0.061_383_46,
            gap: 0.031_900_98,
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let observation = crate::experiment::Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            short_quiet: false,
            ambiguous: false,
            digits: [7, 0, 2, 5, 6, 4, 7, 1, 0, 2, 4, 3, 9],
            axis: 0,
            fraction: 0.6875,
            left: 0.034_294_564_269_340_74,
            right: 0.896_728_315_959_380_7,
            cost: 0.061_383_46,
            gap: 0.031_900_98,
        };

        let (a, b) = crate::identity::gap_edges(
            candidate.detections[0].polygon,
            candidate.detections[1].polygon,
        )
        .expect("geometry eligible");
        let m = crate::scan::transform(candidate.coverage).unwrap();
        let pt = crate::experiment::point(
            m.0,
            observation.axis,
            (observation.left + observation.right) * 0.5,
            observation.fraction,
        )
        .unwrap();
        assert!(crate::identity::barrier_between(a, b, pt));
        candidate.observations.push(observation);
        let pixels = vec![255; 2200 * 2200];
        let f = reconcile_image(
            Some(ImageView::new(&pixels, 2200, 2200, 1, 2200).unwrap()),
            vec![candidate],
            Policy {
                source_identity: true,
                ..Policy::default()
            },
        );
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(f.reconciliation.source_pairs, 0);
        assert_eq!(f.reconciliation.source_pixels, 0);
        assert_eq!(f.candidates[0].observations[0].digits, observation.digits);
    }

    #[test]
    fn frozen_derived_duplicate_retains_competing_gap_read() {
        // Iteration039 derived_2_sameTrue_turn0_proposals candidate1. Actual
        // geometry/raw competing observation; blank pixels cannot affect the veto.
        let mut candidate = c(
            1,
            [[34.0, 572.0], [735.0, 531.0], [755.0, 871.0], [54.0, 912.0]],
            [9, 7, 8, 5, 0, 9, 0, 3, 5, 9, 3, 0, 6],
        );
        candidate.detections[0].polygon = [
            [67.765_297_553_845_09, 649.102_172_325_552_9],
            [652.674_169_404_885_6, 614.892_095_655_235_4],
            [657.672_307_411_457_4, 682.833_716_684_808_2],
            [68.770_883_534_129_87, 717.277_309_236_834_5],
        ];
        let mut second = candidate.detections[0].clone();
        second.polygon = [
            [68.443_136_181_094_64, 762.785_779_481_676],
            [660.338_974_078_137_3, 728.167_050_018_368_2],
            [664.003_778_751_369, 773.442_004_381_161],
            [69.113_526_834_611_15, 808.235_870_755_750_3],
        ];
        candidate.detections.push(second);
        #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
        let observation = crate::experiment::Observation {
            short_quiet: false,
            digits: [3, 6, 4, 1, 2, 0, 0, 3, 5, 9, 3, 0, 6],
            axis: 0,
            fraction: 0.5,
            left: 0.053_730_147_864_183_98,
            right: 0.877_139_495_481_927_5,
            cost: 0.099_777_33,
            gap: 0.020_716_548,
            ambiguous: false,
        };
        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
        let observation = crate::experiment::Observation {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            invalid_checksum: false,
            short_quiet: false,
            digits: [3, 6, 4, 1, 2, 0, 0, 3, 5, 9, 3, 0, 6],
            axis: 0,
            fraction: 0.5,
            left: 0.053_730_147_864_183_98,
            right: 0.877_139_495_481_927_5,
            cost: 0.099_777_33,
            gap: 0.020_716_548,
            ambiguous: false,
        };

        let (a, b) = crate::identity::gap_edges(
            candidate.detections[0].polygon,
            candidate.detections[1].polygon,
        )
        .expect("geometry is eligible");
        let m = crate::scan::transform(candidate.coverage).unwrap();
        let p = crate::experiment::point(
            m.0,
            observation.axis,
            (observation.left + observation.right) * 0.5,
            observation.fraction,
        )
        .unwrap();
        assert!(crate::identity::barrier_between(a, b, p));
        candidate.observations.push(observation);
        let pixels = vec![255; 800 * 1000];
        let frame = reconcile_image(
            Some(ImageView::new(&pixels, 800, 1000, 1, 800).unwrap()),
            vec![candidate],
            Policy {
                source_identity: true,
                ..Policy::default()
            },
        );
        assert_eq!(frame.barcodes.len(), 2);
        assert_eq!(frame.reconciliation.source_pairs, 0);
        assert_eq!(frame.reconciliation.source_pixels, 0);
        assert!(!frame.reconciliation.truncated);
        assert_eq!(
            frame.candidates[0].observations[0].digits,
            observation.digits
        );
    }
    #[test]
    fn identical_shifted_nested_and_distinct_equal_values() {
        let d = [0; 13];
        let frame = reconcile(
            vec![
                c(0, q(0., 0., 100., 50.), d),
                c(1, q(1., 1., 100., 50.), d),
                c(2, q(10., 10., 80., 30.), d),
                c(3, q(200., 0., 100., 50.), d),
            ],
            Policy::default(),
        );
        assert_eq!(frame.barcodes.len(), 2);
        assert_eq!(frame.candidates.len(), 4);
        assert_eq!(frame.barcodes[0].candidate_indices, vec![0, 1, 2]);
        assert_eq!(frame.barcodes[1].candidate_indices, vec![3]);
        assert!(!frame.unfinished);
    }
    #[test]
    fn a_broad_equal_read_cannot_bridge_two_instances() {
        let d = [0; 13];
        let frame = reconcile(
            vec![
                c(0, q(0., 0., 100., 50.), d),
                c(1, q(200., 0., 100., 50.), d),
                c(2, q(0., 0., 300., 50.), d),
            ],
            Policy::default(),
        );
        assert_eq!(frame.barcodes.len(), 2);
        assert_eq!(frame.reconciliation.ambiguous, 1);
        assert!(frame.unfinished);
    }
    #[test]
    fn comparison_and_result_caps_keep_original_evidence() {
        for policy in [
            Policy {
                max_association_checks: 0,
                ..Policy::default()
            },
            Policy {
                max_results: 1,
                ..Policy::default()
            },
        ] {
            let f = reconcile(
                vec![
                    c(0, q(0., 0., 10., 10.), [0; 13]),
                    c(1, q(30., 0., 10., 10.), [0; 13]),
                ],
                policy,
            );
            assert_eq!(
                f.barcodes.len(),
                usize::from(policy.max_association_checks != 0)
            );
            assert_eq!(f.candidates.len(), 2);
            assert!(f.unfinished);
            assert!(f.reconciliation.truncated);
        }
    }
    #[test]
    fn exhausted_conflict_checks_publish_only_pending_evidence() {
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            for limit in 0..3 {
                let cs = order
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| c(i, q(0., 0., 100., 50.), [if v == 2 { 2 } else { 1 }; 13]))
                    .collect();
                let f = reconcile(
                    cs,
                    Policy {
                        max_association_checks: limit,
                        ..Policy::default()
                    },
                );
                assert!(f.barcodes.is_empty());
                assert_eq!(f.candidates.len(), 3);
                if f.reconciliation.truncated {
                    assert_eq!(f.reconciliation.pending_observations, 3);
                }
            }
        }
        for order in [[1, 2], [2, 1]] {
            let f = reconcile(
                order
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| c(i, q(0., 0., 100., 50.), [v; 13]))
                    .collect(),
                Policy {
                    max_association_checks: 0,
                    ..Policy::default()
                },
            );
            assert!(f.barcodes.is_empty());
            assert_eq!(f.reconciliation.pending_observations, 2);
        }
    }
    #[test]
    fn exhausted_candidate_partial_and_later_conflict_are_pending() {
        for reverse in [false, true] {
            let mut candidate = c(0, q(0., 0., 100., 50.), [1; 13]);
            let mut later = candidate.detections[0].clone();
            later.digits = [2; 13];
            candidate.detections.push(later);
            if reverse {
                candidate.detections.reverse();
            }
            candidate.work.association_truncated = 1;
            let f = reconcile(vec![candidate], Policy::default());
            assert!(f.barcodes.is_empty());
            assert_eq!(f.reconciliation.pending_observations, 2);
            assert_eq!(f.candidates[0].detections.len(), 2);
        }
    }
    #[test]
    fn overlapping_tile_bands_require_source_agreement_and_no_contradiction() {
        for reverse in [false, true] {
            for white_gap in [false, true] {
                for contradictory in [false, true] {
                    let mut pixels = vec![255; 500 * 200];
                    for y in 10..150 {
                        for x in 50..450 {
                            if (x / 4) % 3 == 0 && !(white_gap && (65..70).contains(&y)) {
                                pixels[y * 500 + x] = 0;
                            }
                        }
                    }
                    let mut a = c(0, q(0., 0., 500., 200.), [1; 13]);
                    a.detections[0].polygon = q(50., 20., 400., 60.);
                    let mut b = c(1, q(0., 0., 500., 200.), [1; 13]);
                    b.detections[0].polygon = q(50., 55., 400., 60.);
                    if contradictory {
                        {
                            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                            {
                                b.observations.push(crate::experiment::Observation {
                                    short_quiet: false,
                                    ambiguous: false,
                                    digits: [2; 13],
                                    axis: 0,
                                    fraction: 0.35,
                                    left: 0.1,
                                    right: 0.9,
                                    cost: 0.,
                                    gap: 1.,
                                });
                            }
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            {
                                b.observations.push(crate::experiment::Observation {
                                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                    invalid_checksum: false,
                                    short_quiet: false,
                                    ambiguous: false,
                                    digits: [2; 13],
                                    axis: 0,
                                    fraction: 0.35,
                                    left: 0.1,
                                    right: 0.9,
                                    cost: 0.,
                                    gap: 1.,
                                });
                            }
                        }
                    }
                    let mut cs = vec![a, b];
                    if reverse {
                        cs.reverse();
                    }
                    let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
                    let f = reconcile_image(
                        Some(im),
                        cs,
                        Policy {
                            source_identity: true,
                            ..Policy::default()
                        },
                    );
                    assert_eq!(
                        f.barcodes.len(),
                        if white_gap || contradictory { 2 } else { 1 }
                    );
                    assert_eq!(f.candidates.len(), 2);
                }
            }
        }
    }

    #[test]
    fn disjoint_tile_bands_require_source_agreement_and_no_contradiction() {
        for reverse in [false, true] {
            for white_gap in [false, true] {
                for contradictory in [false, true] {
                    let mut pixels = vec![255; 500 * 200];
                    for y in 10..150 {
                        for x in 50..450 {
                            if (x / 4) % 3 == 0 && !(white_gap && (65..70).contains(&y)) {
                                pixels[y * 500 + x] = 0;
                            }
                        }
                    }
                    let mut a = c(0, q(0., 0., 500., 200.), [1; 13]);
                    a.detections[0].polygon = q(50., 20., 400., 40.);
                    let mut b = c(1, q(0., 0., 500., 200.), [1; 13]);
                    b.detections[0].polygon = q(50., 80., 400., 40.);
                    if contradictory {
                        {
                            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                            {
                                b.observations.push(crate::experiment::Observation {
                                    short_quiet: false,
                                    ambiguous: false,
                                    digits: [2; 13],
                                    axis: 0,
                                    fraction: 0.35,
                                    left: 0.1,
                                    right: 0.9,
                                    cost: 0.,
                                    gap: 1.,
                                });
                            }
                            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                            {
                                b.observations.push(crate::experiment::Observation {
                                    #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                                    invalid_checksum: false,
                                    short_quiet: false,
                                    ambiguous: false,
                                    digits: [2; 13],
                                    axis: 0,
                                    fraction: 0.35,
                                    left: 0.1,
                                    right: 0.9,
                                    cost: 0.,
                                    gap: 1.,
                                });
                            }
                        }
                    }
                    let mut cs = vec![a, b];
                    if reverse {
                        cs.reverse();
                    }
                    let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
                    let f = reconcile_image(
                        Some(im),
                        cs,
                        Policy {
                            source_identity: true,
                            ..Policy::default()
                        },
                    );
                    assert_eq!(
                        f.barcodes.len(),
                        if white_gap || contradictory { 2 } else { 1 }
                    );
                    assert_eq!(f.candidates.len(), 2);
                }
            }
        }
    }
    #[test]
    fn pending_candidates_quarantine_overlap_but_not_independent_reads() {
        for swapped in [false, true] {
            for different in [false, true] {
                let done = c(0, q(0., 0., 100., 50.), [1; 13]);
                let mut pending = c(1, q(0., 0., 100., 50.), [if different { 2 } else { 1 }; 13]);
                pending.work.association_truncated = 1;
                let independent = c(2, q(200., 0., 100., 50.), [3; 13]);
                let mut cs = vec![done, pending, independent];
                if swapped {
                    cs.reverse();
                }
                let f = reconcile(cs, Policy::default());
                assert_eq!(f.barcodes.len(), 1);
                assert_eq!(f.barcodes[0].detection.digits, [3; 13]);
                assert_eq!(f.reconciliation.pending_quarantined, 1);
                assert_eq!(f.reconciliation.pending_observations, 2);
                assert!(f.unfinished);
            }
        }
    }
    #[test]
    fn image_direction_identity_survives_cyclic_proposal_axes() {
        let mut pixels = vec![255; 500 * 200];
        for y in 10..150 {
            for x in 50..450 {
                if (x / 4) % 3 == 0 {
                    pixels[y * 500 + x] = 0;
                }
            }
        }
        for shift in 0..4 {
            for reverse in [false, true] {
                let mut a = c(0, q(0., 0., 500., 200.), [1; 13]);
                a.detections[0].polygon = q(50., 20., 400., 60.);
                let mut b = c(1, q(0., 0., 500., 200.), [1; 13]);
                b.coverage.rotate_left(shift);
                b.detections[0].axis = shift % 2;
                b.detections[0].polygon = q(50., 55., 400., 60.);
                let mut cs = vec![a, b];
                if reverse {
                    cs.reverse();
                }
                let f = reconcile_image(
                    Some(ImageView::new(&pixels, 500, 200, 1, 500).unwrap()),
                    cs,
                    Policy {
                        source_identity: true,
                        ..Policy::default()
                    },
                );
                assert_eq!(f.barcodes.len(), 1);
            }
        }
    }
    #[test]
    fn result_cap_keeps_highest_support_and_raw_candidates() {
        let a = c(0, q(0., 0., 10., 10.), [0; 13]);
        let mut b = c(1, q(30., 0., 10., 10.), [1; 13]);
        b.detections[0].support = 99;
        let f = reconcile(
            vec![a, b],
            Policy {
                max_results: 1,
                ..Policy::default()
            },
        );
        assert_eq!(f.barcodes[0].detection.support, 99);
        assert_eq!(f.candidates.len(), 2);
        assert!(f.unfinished);
    }
    #[test]
    fn source_association_budget_is_shared_across_candidates() {
        let bits=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
        let mut pixels = vec![255; 1000 * 240];
        for y in 10..230 {
            for x in 0..380 {
                if bits[x / 4] == b'1' {
                    pixels[y * 1000 + 30 + x] = 0;
                    pixels[y * 1000 + 530 + x] = 0;
                }
            }
        }
        let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
        let policy = Policy {
            max_retry_paths_per_candidate: 0,
            max_retry_paths_per_frame: 0,
            max_association_checks: 1000,
            max_association_pixels: 4096,
            ..Policy::default()
        };
        let frame = CandidateScanner::default()
            .scan_frame(
                im,
                &[q(30., 10., 380., 220.), q(530., 10., 380., 220.)],
                policy,
            )
            .unwrap();
        assert!(frame.unfinished);
        {
            #[cfg(feature = "mode-low")]
            {
                assert!(frame
                    .candidates
                    .iter()
                    .all(|c| matches!(c.work.paths, 3 | 6)));
            }
            #[cfg(feature = "mode-medium")]
            {
                assert!(frame.candidates.iter().all(|c| c.work.paths == 6));
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                assert!(frame.candidates.iter().all(|c| c.work.paths == 10));
            }
        }

        assert!(
            frame
                .candidates
                .iter()
                .map(|c| c.work.continuity_samples)
                .sum::<usize>()
                <= 4096
        );
        assert!(
            frame
                .candidates
                .iter()
                .map(|c| c.work.association_checks)
                .sum::<usize>()
                + frame.reconciliation.comparisons
                <= 1000
        );
        assert!(frame
            .candidates
            .iter()
            .any(|c| c.work.association_truncated > 0));
    }
    #[test]
    fn geometry_and_conflicting_text() {
        assert!(!same_space(q(0., 0., 10., 10.), q(10., 0., 10., 10.)));
        let mut b = q(0., 0., 10., 10.);
        b.reverse();
        assert!(same_space(q(0., 0., 10., 10.), b));
        let f = reconcile(vec![c(0, b, [0; 13]), c(1, b, [1; 13])], Policy::default());
        assert!(f.barcodes.is_empty());
        assert_eq!(f.reconciliation.conflicting, 1);
        assert_eq!(f.candidates.len(), 2);
    }
    #[test]
    fn conflicts_with_nonrepresentative_members_survive_permutations() {
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let mut candidates = vec![];
            for i in order {
                let mut d = match i {
                    0 => c(i, q(0., 0., 100., 50.), [1; 13]),
                    1 => c(i, q(15., 0., 100., 50.), [1; 13]),
                    _ => c(i, q(100., 0., 15., 50.), [2; 13]),
                };
                d.detections[0].support = if i == 1 { 9 } else { 2 };
                candidates.push(d);
            }
            let f = reconcile(candidates, Policy::default());
            assert!(f.unfinished);
            assert!(f.reconciliation.conflicting > 0);
            assert!(f.barcodes.is_empty());
        }
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "Keep the paired identity and separating-gap controls in one regression."
    )]
    fn source_identity_preserves_ambiguity_and_neighbor_gaps() {
        let mut pixels = vec![255; 500 * 200];
        for y in 0..200 {
            for x in 50..450 {
                if (x / 4) % 3 == 0 {
                    pixels[y * 500 + x] = 0;
                }
            }
        }
        let make = || {
            let mut c = c(0, q(0., 0., 500., 200.), [1; 13]);
            c.detections = vec![
                Detection {
                    digits: [1; 13],
                    polygon: q(50., 20., 400., 60.),
                    support: 3,
                    axis: 0,
                },
                Detection {
                    digits: [1; 13],
                    polygon: q(50., 120., 400., 60.),
                    support: 4,
                    axis: 0,
                },
            ];
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    c.observations.push(crate::experiment::Observation {
                        short_quiet: false,
                        ambiguous: true,
                        digits: [0; 13],
                        axis: 0,
                        fraction: 0.5,
                        left: 0.1,
                        right: 0.9,
                        cost: 0.,
                        gap: 0.,
                    });
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    c.observations.push(crate::experiment::Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous: true,
                        digits: [0; 13],
                        axis: 0,
                        fraction: 0.5,
                        left: 0.1,
                        right: 0.9,
                        cost: 0.,
                        gap: 0.,
                    });
                }
            }

            c
        };
        let policy = Policy {
            source_identity: true,
            ..Policy::default()
        };
        let f = reconcile_image(
            Some(ImageView::new(&pixels, 500, 200, 1, 500).unwrap()),
            vec![make()],
            policy,
        );
        assert_eq!(f.barcodes.len(), 1);
        assert_eq!(f.candidates[0].detections.len(), 2);
        assert!(f.candidates[0].observations[0].ambiguous);
        assert!(f.unfinished);
        assert_eq!(f.barcodes[0].detection.polygon, q(50., 120., 400., 60.));
        assert_eq!(f.barcodes[0].detection.support, 4);
        let mut contradictory = make();
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
            {
                contradictory
                    .observations
                    .push(crate::experiment::Observation {
                        short_quiet: false,
                        ambiguous: false,
                        digits: [2; 13],
                        ..contradictory.observations[0]
                    });
            }
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                contradictory
                    .observations
                    .push(crate::experiment::Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous: false,
                        digits: [2; 13],
                        ..contradictory.observations[0]
                    });
            }
        }

        let f = reconcile_image(
            Some(ImageView::new(&pixels, 500, 200, 1, 500).unwrap()),
            vec![contradictory],
            policy,
        );
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(f.reconciliation.source_pairs, 0);
        // A single original-pixel blank gap must preserve two identical instances.
        pixels[100 * 500..101 * 500].fill(255);
        let im = ImageView::new(&pixels, 500, 200, 1, 500).unwrap();
        let f = reconcile_image(Some(im), vec![make()], policy);
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(f.reconciliation.source_matches, 0);
        assert!(f.reconciliation.source_pixels > 0);
        let mut a = make();
        a.observations.clear();
        let f = reconcile_image(Some(im), vec![a], policy);
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(f.reconciliation.source_pairs, 0);
    }
}
#[cfg(test)]
mod crossing_identity_tests {
    use super::*;
    #[test]
    fn requires_shared_interior_module_position() {
        let a = [[20., 50.], [400., 50.], [400., 52.], [20., 52.]];
        let b = [[20., 30.], [400., 70.], [400., 72.], [20., 32.]];
        assert!(crossing_read_paths(a, b));
        assert!(crossing_read_paths(a, [b[2], b[3], b[0], b[1]]));
        assert!(!crossing_read_paths(a, a.map(|p| [p[0], p[1] + 30.])));
        assert!(!crossing_read_paths(a, b.map(|p| [p[0] + 40., p[1]])));
        let rot = |p: [f64; 2]| [-p[1], p[0]];
        assert!(crossing_read_paths(a.map(rot), b.map(rot)));
    }
}
#[cfg(test)]
mod crossing_physical_evidence_tests {
    use super::*;
    #[test]
    fn crossed_equal_claims_without_source_evidence_stay_separate() {
        let a = [[20., 50.], [400., 50.], [400., 52.], [20., 52.]];
        let b = [[20., 30.], [400., 70.], [400., 72.], [20., 32.]];
        let make = |index, polygon| Candidate {
            index,
            coverage: polygon,
            observations: vec![],
            detections: vec![Detection {
                digits: [0; 13],
                polygon,
                support: 2,
                axis: 0,
            }],
            work: Work::default(),
            ms: 0.,
            error: false,
        };
        let data = vec![255; 512 * 100];
        let im = ImageView::new(&data, 512, 100, 1, 512).unwrap();
        for enabled in [false, true] {
            let f = reconcile_image(
                Some(im),
                vec![make(0, a), make(1, b)],
                Policy {
                    source_identity: enabled,
                    ..Default::default()
                },
            );
            assert_eq!(f.barcodes.len(), 2);
        }
    }
    #[test]
    fn endpoint_crossing_geometry_still_requires_source_identity() {
        let a = [[20., 50.], [400., 50.], [400., 52.], [20., 52.]];
        let b = [[20., 48.], [400., 68.], [400., 70.], [20., 50.]];
        assert!(crossing_read_paths(a, b));
        let make = |index, polygon| Candidate {
            index,
            coverage: polygon,
            observations: vec![],
            detections: vec![Detection {
                digits: [0; 13],
                polygon,
                support: 2,
                axis: 0,
            }],
            work: Work::default(),
            ms: 0.,
            error: false,
        };
        let data = vec![255; 512 * 100];
        let im = ImageView::new(&data, 512, 100, 1, 512).unwrap();
        let f = reconcile_image(
            Some(im),
            vec![make(0, a), make(1, b)],
            Policy {
                source_identity: true,
                ..Default::default()
            },
        );
        assert_eq!(f.barcodes.len(), 2);
    }
}
#[cfg(test)]
mod optional_identity_tests {
    use super::*;
    const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    fn q(y: f64) -> Quad {
        [[20., y], [400., y], [400., y + 5.], [20., y + 5.]]
    }
    fn c(index: usize, y: f64, digits: [u8; 13]) -> Candidate {
        Candidate {
            index,
            coverage: [[0., 0.], [419., 0.], [419., 159.], [0., 159.]],
            observations: vec![],
            detections: vec![Detection {
                digits,
                polygon: q(y),
                support: 2,
                axis: 0,
            }],
            work: Work::default(),
            ms: 0.,
            error: false,
        }
    }
    fn pixels(gap: bool) -> Vec<u8> {
        let mut p = vec![255; 420 * 160];
        let bits = crate::ean::encode(&A);
        for y in 0..160 {
            if gap && y == 60 {
                continue;
            }
            for x in 0..380 {
                if bits[x / 4] > 0.5 {
                    p[y * 420 + 20 + x] = 0;
                }
            }
        }
        p
    }
    #[test]
    fn optional_pixel_exhaustion_retains_bands_and_charges_shared_pixels() {
        let p = pixels(false);
        let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
        for limit in [0, 95, 199, 4096] {
            let f = reconcile_image(
                Some(im),
                vec![c(0, 20., A), c(1, 100., A)],
                Policy {
                    source_identity: true,
                    max_association_pixels: limit,
                    ..Policy::default()
                },
            );
            assert_eq!(f.barcodes.len(), 2);
            assert!(f.unfinished);
            assert!(!f.reconciliation.truncated);
            assert!(f.reconciliation.optional_identity_deferred > 0);
            assert!(f.reconciliation.source_pixels <= limit);
            assert_eq!(f.reconciliation.pending_observations, 0);
            assert_eq!(f.reconciliation.comparisons, 1);
            if limit == 199 {
                assert_eq!(f.reconciliation.source_pixels, 192);
            }
            assert!(crate::region_json::frame_json(&f).contains("\"optional_identity_deferred\":1"));
        }
    }
    #[test]
    fn mandatory_checks_still_quarantine_and_later_conflicts_are_examined() {
        let p = pixels(false);
        let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
        let f = reconcile_image(
            Some(im),
            vec![c(0, 20., A), c(1, 100., A)],
            Policy {
                source_identity: true,
                max_association_checks: 0,
                max_association_pixels: 0,
                ..Policy::default()
            },
        );
        assert!(f.barcodes.is_empty());
        assert!(f.reconciliation.truncated);
        assert_eq!(f.reconciliation.optional_identity_deferred, 0);
        let f = reconcile_image(
            Some(im),
            vec![c(0, 20., A), c(1, 100., A), c(2, 20., B)],
            Policy {
                source_identity: true,
                max_association_pixels: 0,
                ..Policy::default()
            },
        );
        assert!(!f.reconciliation.truncated);
        assert!(f.reconciliation.conflicting > 0);
        assert!(f.reconciliation.optional_identity_deferred > 0);
        assert_eq!(f.barcodes.len(), 1);
        assert_eq!(f.barcodes[0].candidate_indices, vec![1]);
        let f = reconcile_image(
            Some(im),
            vec![c(0, 20., A), c(1, 20., B)],
            Policy {
                source_identity: true,
                max_association_pixels: 0,
                ..Policy::default()
            },
        );
        assert!(f.barcodes.is_empty());
        assert!(f.reconciliation.conflicting > 0);
        assert_eq!(f.reconciliation.optional_identity_deferred, 0);
    }
    #[test]
    fn completed_continuity_merges_but_one_pixel_white_gap_keeps_equal_symbols() {
        for gap in [false, true] {
            let p = pixels(gap);
            let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
            let f = reconcile_image(
                Some(im),
                vec![c(0, 20., A), c(1, 100., A)],
                Policy {
                    source_identity: true,
                    ..Policy::default()
                },
            );
            assert_eq!(f.barcodes.len(), if gap { 2 } else { 1 });
            assert_eq!(f.reconciliation.optional_identity_deferred, 0);
            assert!(!f.reconciliation.truncated);
        }
    }
    #[test]
    fn phase_pixel_shortfall_is_deferred_not_global_truncation() {
        let mut p = vec![255; 420 * 160];
        for y in 0..160 {
            for x in 0..420 {
                p[y * 420 + x] = if (x / 2) % 2 == 0 { 0 } else { 255 };
            }
        }
        let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
        let mut budget = AssociationBudget {
            pixels_left: 1184,
            checks_left: 10,
        };
        let mut w = Work::default();
        let mut r = ReconciliationWork::default();
        {
            #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
            {
                assert!(!same_text_identity(
                    im,
                    [[20., 20.], [400., 20.]],
                    [[22., 100.], [402., 100.]],
                    &mut budget,
                    &mut w,
                    &mut r
                ));
            }
            #[cfg(feature = "mode-very-high")]
            {
                assert!(!same_text_identity(
                    im,
                    [[20., 20.], [400., 20.]],
                    [[22., 100.], [402., 100.]],
                    &mut budget,
                    &mut w,
                    &mut r,
                    &mut crate::identity::IdentityCache::default()
                ));
            }
        }

        assert_eq!(w.continuity_samples, 192);
        assert_eq!(budget.pixels_left, 992);
        assert_eq!(w.continuity_capped_links, 0);
        assert_eq!(r.optional_identity_deferred, 1);
        assert_eq!(w.association_truncated, 0);
        assert_eq!(budget.checks_left, 10);
    }
}
#[cfg(feature = "mode-very-high")]
#[cfg(test)]
mod source_conflict_probe_tests {
    use super::*;
    fn q(y: f64, h: f64) -> Quad {
        [[20., y], [400., y], [400., y + h], [20., y + h]]
    }
    #[test]
    fn source_pixels_resolve_alias_but_not_real_competing_code() {
        let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let other = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        let bits = crate::ean::encode(&good);
        let mut pixels = vec![255; 440 * 120];
        for y in 0..120 {
            for x in 0..380 {
                pixels[y * 440 + 20 + x] = if bits[x / 4] > 0.5 { 0 } else { 255 };
            }
        }
        let broad = Detection {
            digits: good,
            polygon: q(10., 100.),
            support: 20,
            axis: 0,
        };
        let thin = Detection {
            digits: other,
            polygon: q(58., 3.),
            support: 2,
            axis: 0,
        };
        let mut budget = AssociationBudget::default();
        let mut counter = Work::default();
        let before = budget.pixels_left;
        assert!(source_conflict_winner(
            ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
            &broad,
            &thin,
            &mut budget,
            &mut counter
        ));
        assert_eq!(before - budget.pixels_left, counter.continuity_samples);
        assert!(counter.continuity_samples > 0);
        let mut empty = AssociationBudget {
            checks_left: 0,
            pixels_left: 0,
        };
        assert!(!source_conflict_winner(
            ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
            &broad,
            &thin,
            &mut empty,
            &mut Work::default()
        ));
        let bits = crate::ean::encode(&other);
        for y in 55..65 {
            for x in 0..380 {
                pixels[y * 440 + 20 + x] = if bits[x / 4] > 0.5 { 0 } else { 255 };
            }
        }
        assert!(!source_conflict_winner(
            ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
            &broad,
            &thin,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        pixels.fill(128);
        assert!(!source_conflict_winner(
            ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
            &broad,
            &thin,
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
    }
}
#[cfg(feature = "mode-very-high")]
#[cfg(test)]
mod paired_pixel_tests {
    use super::*;
    fn band(y: f64, digits: [u8; 13]) -> Detection {
        Detection {
            digits,
            polygon: [[20., y], [305., y], [305., y + 15.], [20., y + 15.]],
            support: 4,
            axis: 0,
        }
    }
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
    )]
    fn render(top: [u8; 13], bottom: [u8; 13]) -> Vec<u8> {
        let mut p = vec![255u8; 330 * 100];
        for y in 0..100 {
            let bits = crate::ean::encode(&if y < 50 { top } else { bottom });
            for x in 20..305 {
                p[y * 330 + x] = if bits[(x - 20) / 3] == 1. { 0 } else { 255 };
            }
        }
        p
    }
    #[test]
    fn actual_contrary_code_is_not_outvoted_by_a_neighbor() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        let p = render(a, b);
        let im = ImageView::new(&p, 330, 100, 1, 330).unwrap();
        assert!(!source_pair_winner(
            im,
            &band(20., a),
            &band(65., b),
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        assert!(!source_pair_winner(
            im,
            &band(65., b),
            &band(20., a),
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
    }
    #[test]
    fn false_alias_can_only_be_rejected_when_both_bands_support_the_winner() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        let p = render(a, a);
        let im = ImageView::new(&p, 330, 100, 1, 330).unwrap();
        assert!(source_pair_winner(
            im,
            &band(20., a),
            &band(65., b),
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        assert!(!source_pair_winner(
            im,
            &band(65., b),
            &band(20., a),
            &mut AssociationBudget::default(),
            &mut Work::default()
        ));
        let mut budget = AssociationBudget {
            pixels_left: 0,
            checks_left: 0,
        };
        let mut work = Work::default();
        assert!(!source_pair_winner(
            im,
            &band(20., a),
            &band(65., b),
            &mut budget,
            &mut work
        ));
        assert_eq!(work.continuity_samples, 0);
    }
}

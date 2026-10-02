//! Frame-level reconciliation; original candidate evidence remains untouched.
#![forbid(unsafe_code)]
mod identity;
pub(crate) use identity::*;
mod conflict;
use crate::{
    candidate_scanner::{AssociationBudget, Candidate, CandidateScanner, Detection, Work},
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
                                    if let Ok(p) = crate::candidate_scanner::point(
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
mod crossing_identity_tests;
#[cfg(test)]
mod crossing_physical_evidence_tests;
#[cfg(test)]
mod optional_identity_tests;
#[cfg(feature = "mode-very-high")]
#[cfg(test)]
mod paired_pixel_tests;
#[cfg(feature = "mode-very-high")]
#[cfg(test)]
mod source_conflict_probe_tests;
#[cfg(test)]
mod tests;

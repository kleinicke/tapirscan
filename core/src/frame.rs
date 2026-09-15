//! Frame-level reconciliation; original candidate evidence remains untouched.
#![forbid(unsafe_code)]
use crate::{
    experiment::{AssociationBudget, Candidate, Detection, Experiment, Work},
    multi_scan::Policy,
    sampling::{Error, ImageView},
    scan::Quad,
};
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
fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
fn signed_area(p: &[[f64; 2]]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .take(p.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum::<f64>()
        * 0.5
}
fn intersection(a: Quad, b: Quad) -> f64 {
    let sign = signed_area(&b).signum();
    let mut p = a.to_vec();
    for i in 0..4 {
        let (a, b) = (b[i], b[(i + 1) % 4]);
        let mut out = Vec::with_capacity(8);
        if p.is_empty() {
            return 0.;
        }
        for j in 0..p.len() {
            let (x, y) = (p[j], p[(j + 1) % p.len()]);
            let (dx, dy) = (sign * cross(a, b, x), sign * cross(a, b, y));
            if dx >= 0. {
                out.push(x);
            }
            if (dx >= 0.) != (dy >= 0.) {
                let t = dx / (dx - dy);
                out.push([x[0] + t * (y[0] - x[0]), x[1] + t * (y[1] - x[1])]);
            }
        }
        p = out;
    }
    signed_area(&p).abs()
}
// Equal reads on crossing paths may have narrow, poorly overlapping polygons.
// Require aligned scale and the same interior module position at intersection.
fn crossing_read_paths(a: Quad, b: Quad) -> bool {
    let ends = |q: Quad| {
        [
            [0.5 * (q[0][0] + q[3][0]), 0.5 * (q[0][1] + q[3][1])],
            [0.5 * (q[1][0] + q[2][0]), 0.5 * (q[1][1] + q[2][1])],
        ]
    };
    let a = ends(a);
    let mut b = ends(b);
    let u = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
    let mut v = [b[1][0] - b[0][0], b[1][1] - b[0][1]];
    if u[0] * v[0] + u[1] * v[1] < 0. {
        b.reverse();
        v = [-v[0], -v[1]];
    }
    let la = u[0].hypot(u[1]);
    let lb = v[0].hypot(v[1]);
    if la < 76.
        || lb < 76.
        || la.min(lb) / la.max(lb) < 0.9
        || (u[0] * v[0] + u[1] * v[1]) / (la * lb) < 20f64.to_radians().cos()
    {
        return false;
    }
    let den = u[0] * v[1] - u[1] * v[0];
    if den.abs() < 1e-9 {
        return false;
    }
    let w = [b[0][0] - a[0][0], b[0][1] - a[0][1]];
    let t = (w[0] * v[1] - w[1] * v[0]) / den;
    let z = (w[0] * u[1] - w[1] * u[0]) / den;
    // Endpoint-crossing fits are valid only after the mandatory source-pixel
    // identity check in reconcile_image; geometry alone never merges a read.
    (-0.05..=1.05).contains(&t) && (-0.05..=1.05).contains(&z) && (t - z).abs() <= 2. / 95.
}
pub(crate) fn same_space(a: Quad, b: Quad) -> bool {
    let (aa, bb) = (signed_area(&a).abs(), signed_area(&b).abs());
    if !aa.is_finite() || !bb.is_finite() || aa <= 0. || bb <= 0. {
        return false;
    }
    let x = intersection(a, b).min(aa.min(bb));
    x / (aa + bb - x) >= 0.5 || x / aa.min(bb) >= 0.85
}
// Conservative, constant-size envelope check for mandatory pending quarantine.
// At most64pending envelopes per group member; separate from optional matching.
fn envelope(q: Quad) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in q {
        if !p[0].is_finite() || !p[1].is_finite() {
            return [
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::INFINITY,
            ];
        }
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b
}
fn envelopes_overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}
struct Group {
    members: Vec<(usize, usize)>,
    conflicted: bool,
}
impl Experiment {
    /// Primary frame surface reconciles physical reads while retaining all raw
    /// candidate results. Resource exhaustion returns a flagged partial frame.
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
fn same_text_identity(
    im: ImageView<'_>,
    a: [[f64; 2]; 2],
    b: [[f64; 2]; 2],
    budget: &mut AssociationBudget,
    counter: &mut Work,
    work: &mut ReconciliationWork,
) -> bool {
    #[cfg(not(feature = "experimental-identity-optional"))]
    {
        let _ = work;
        crate::identity::connected(im, a, b, budget, counter)
    }
    #[cfg(feature = "experimental-identity-optional")]
    {
        let mut local = Work::default();
        let connected = crate::identity::connected(im, a, b, budget, &mut local);
        counter.continuity_samples += local.continuity_samples;
        counter.continuity_capped_links += local.continuity_capped_links;
        counter.continuity_rejects += local.continuity_rejects;
        // This helper spends pixels only. Preserve outer mandatory failures and
        // conservatively propagate any future check-budget work added to identity.
        counter.association_checks += local.association_checks;
        if local.association_checks > 0 {
            counter.association_truncated |= local.association_truncated;
        }
        if local.association_truncated > 0 {
            work.optional_identity_deferred += 1;
            return false;
        }
        connected
    }
}
fn reconcile_image(im: Option<ImageView<'_>>, candidates: Vec<Candidate>, policy: Policy) -> Frame {
    let used_checks: usize = candidates.iter().map(|c| c.work.association_checks).sum();
    let mut budget = AssociationBudget {
        checks_left: policy.max_association_checks.saturating_sub(used_checks),
        pixels_left: policy
            .max_association_pixels
            .saturating_sub(candidates.iter().map(|c| c.work.continuity_samples).sum()),
    };
    let mut counter = Work::default();
    let mut work = ReconciliationWork::default();
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
                        if same_text_identity(im, a, b, &mut budget, &mut counter, &mut work) {
                            overlap = true;
                            work.source_matches += 1;
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
                    let shared_coverage = cfg!(feature = "experimental-shared-coverage-identity")
                        && ci != mi
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
        let o = crate::experiment::Observation {
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
        let pt =
            crate::experiment::point(m.0, o.axis, (o.left + o.right) * 0.5, o.fraction).unwrap();
        assert!(crate::identity::barrier_between(a, b, pt));
        candidate.observations.push(o);
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
        assert_eq!(f.candidates[0].observations[0].digits, o.digits);
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
        let o = crate::experiment::Observation {
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
        let pt =
            crate::experiment::point(m.0, o.axis, (o.left + o.right) * 0.5, o.fraction).unwrap();
        assert!(crate::identity::barrier_between(a, b, pt));
        candidate.observations.push(o);
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
        assert_eq!(f.candidates[0].observations[0].digits, o.digits);
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
        let o = crate::experiment::Observation {
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
        let p =
            crate::experiment::point(m.0, o.axis, (o.left + o.right) * 0.5, o.fraction).unwrap();
        assert!(crate::identity::barrier_between(a, b, p));
        candidate.observations.push(o);
        let pixels = vec![255; 800 * 1000];
        let f = reconcile_image(
            Some(ImageView::new(&pixels, 800, 1000, 1, 800).unwrap()),
            vec![candidate],
            Policy {
                source_identity: true,
                ..Policy::default()
            },
        );
        assert_eq!(f.barcodes.len(), 2);
        assert_eq!(f.reconciliation.source_pairs, 0);
        assert_eq!(f.reconciliation.source_pixels, 0);
        assert!(!f.reconciliation.truncated);
        assert_eq!(f.candidates[0].observations[0].digits, o.digits);
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
    #[cfg(feature = "experimental-shared-coverage-identity")]
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
        let frame = Experiment::default()
            .scan_frame(
                im,
                &[q(30., 10., 380., 220.), q(530., 10., 380., 220.)],
                policy,
            )
            .unwrap();
        assert!(frame.unfinished);
        assert!(frame.candidates.iter().all(|c| c.work.paths == 10));
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
        contradictory
            .observations
            .push(crate::experiment::Observation {
                short_quiet: false,
                ambiguous: false,
                digits: [2; 13],
                ..contradictory.observations[0]
            });
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

#[cfg(all(test, feature = "experimental-identity-optional"))]
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
        assert!(!same_text_identity(
            im,
            [[20., 20.], [400., 20.]],
            [[22., 100.], [402., 100.]],
            &mut budget,
            &mut w,
            &mut r
        ));
        assert_eq!(w.continuity_samples, 192);
        assert_eq!(budget.pixels_left, 992);
        assert_eq!(w.continuity_capped_links, 0);
        assert_eq!(r.optional_identity_deferred, 1);
        assert_eq!(w.association_truncated, 0);
        assert_eq!(budget.checks_left, 10);
    }
}

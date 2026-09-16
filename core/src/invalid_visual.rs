//! Development-only invalid-checksum evidence in normalized run-width MSE units.
//! Intensity/soft decoder losses are never numerically compared to these costs.
use crate::{
    experiment::{Observation, Work},
    multi_profile::Reads,
    run_profile::Read,
};
const LIMIT: usize = 512;
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunVisual {
    pub read: Read,
    pub checksum_valid: bool,
}
#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
pub(crate) fn append(out: &mut Vec<RunVisual>, capped: &mut bool, v: RunVisual) {
    if !v.read.left.is_finite()
        || !v.read.right.is_finite()
        || v.read.left >= v.read.right
        || !v.read.cost.is_finite()
        || !v.read.gap.is_finite()
    {
        *capped = true;
        return;
    }
    if let Some(old) = out.iter_mut().find(|a| {
        a.checksum_valid == v.checksum_valid
            && a.read.digits == v.read.digits
            && a.read.left == v.read.left
            && a.read.right == v.read.right
    }) {
        if v.read.cost < old.read.cost {
            *old = v;
        }
        return;
    }
    if out.len() == LIMIT {
        *capped = true;
        return;
    }
    out.push(v);
}
pub(crate) fn collect(out: &mut Vec<RunVisual>, capped: &mut bool, r: &Reads) {
    *capped |= r.run_visual_capped;
    for &v in &r.run_visual {
        append(out, capped, v);
    }
}
fn same(a: f64, b: f64, c: f64, d: f64) -> bool {
    (b.min(d) - a.max(c)).max(0.) >= 0.8 * (b - a).max(d - c)
}
#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
fn vetoes(e: &[RunVisual]) -> Vec<Read> {
    let mut out = Vec::new();
    for v in e
        .iter()
        .filter(|v| !v.checksum_valid && v.read.cost <= 0.06 && v.read.gap >= 0.1)
    {
        let best = e
            .iter()
            .filter(|a| {
                a.checksum_valid && same(a.read.left, a.read.right, v.read.left, v.read.right)
            })
            .map(|a| a.read.cost)
            .min_by(f32::total_cmp);
        // Compare against every qualified valid run model, including evidence that
        // ordinary duplicate merging did not retain. An intensity result is not here.
        if best.is_some_and(|cost| v.read.cost < cost)
            && !out
                .iter()
                .any(|a: &Read| a.left == v.read.left && a.right == v.read.right)
        {
            out.push(v.read);
        }
    }
    out
}
#[expect(
    clippy::too_many_arguments,
    reason = "Invalid-visual reconciliation jointly considers the independent raw, soft and normalized evidence with shared candidate/work state."
)]
pub(crate) fn apply(
    e: &[RunVisual],
    capped: bool,
    soft: &[Read],
    observations: &mut Vec<Observation>,
    start: usize,
    axis: usize,
    fraction: f64,
    lo: f64,
    hi: f64,
    n: usize,
    work: &mut Work,
) {
    work.invalid_visual_seen += e.iter().filter(|v| !v.checksum_valid).count();
    if capped {
        work.invalid_veto_capped += 1;
        work.truncated_paths += 1;
        return;
    }
    for v in vetoes(e) {
        let left = lo + (hi - lo) * (v.left + 0.5) / crate::numeric::usize_f64(n);
        let right = lo + (hi - lo) * (v.right + 0.5) / crate::numeric::usize_f64(n);
        let soft_conflict = soft.iter().any(|a| same(a.left, a.right, v.left, v.right));
        let mut i = 0;
        let before = observations.len();
        observations.retain(|o| {
            let keep = i < start || o.ambiguous || !same(o.left, o.right, left, right);
            i += 1;
            keep
        });
        let removed = before - observations.len();
        if removed == 0 && !soft_conflict {
            continue;
        }
        work.invalid_veto_reads += removed;
        work.invalid_veto_intervals += 1;
        work.invalid_soft_conflicts += usize::from(soft_conflict);
        work.conflicts += 1;
        // A soft/intensity contradiction is explicitly unresolved; its different
        // cost units never decide a winner. Existing spatial ambiguity rules apply.
        observations.push(Observation {
            short_quiet: false,
            ambiguous: true,
            digits: [0; 13],
            axis,
            fraction,
            left,
            right,
            cost: v.cost,
            gap: v.gap,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn evidence(valid: bool, left: f64, right: f64, cost: f32) -> RunVisual {
        RunVisual {
            read: Read {
                digits: [if valid { 1 } else { 2 }; 13],
                left,
                right,
                cost,
                gap: 0.2,
            },
            checksum_valid: valid,
        }
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn strongest_valid_protects_and_disjoint_symbols_do_not_interact() {
        let bad = evidence(false, 0., 95., 0.03);
        let weak = evidence(true, 0., 95., 0.08);
        let disjoint = evidence(true, 120., 215., 0.09);
        let v = vetoes(&[bad, weak, disjoint]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].right, 95.);
        assert!(vetoes(&[bad, disjoint]).is_empty());
        assert!(vetoes(&[bad, weak, evidence(true, 0., 95., 0.02)]).is_empty());
        assert!(vetoes(&[
            RunVisual {
                read: Read {
                    gap: 0.09,
                    ..bad.read
                },
                ..bad
            },
            weak
        ])
        .is_empty());
        assert!(vetoes(&[evidence(false, 0., 95., 0.061), weak]).is_empty());
    }
    #[test]
    fn capped_evidence_disables_veto_and_marks_pending() {
        let mut e = vec![];
        let mut capped = false;
        for i in 0..=LIMIT {
            append(
                &mut e,
                &mut capped,
                evidence(
                    false,
                    crate::numeric::usize_f64(i),
                    crate::numeric::usize_f64(i) + 95.,
                    0.03,
                ),
            );
        }
        assert!(capped);
        assert_eq!(e.len(), LIMIT);
        let mut w = Work::default();
        let mut obs = vec![];
        apply(&e, capped, &[], &mut obs, 0, 0, 0.5, 0., 1., 512, &mut w);
        assert_eq!(w.invalid_veto_capped, 1);
        assert_eq!(w.truncated_paths, 1);
        assert_eq!(w.invalid_veto_intervals, 0);
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn interval_local_removal_retains_other_symbols_and_soft_conflict() {
        let e = [
            evidence(false, 0., 95., 0.03),
            evidence(true, 0., 95., 0.08),
            evidence(true, 120., 215., 0.09),
        ];
        let obs = |left, right| Observation {
            digits: [1; 13],
            axis: 0,
            fraction: 0.5,
            left,
            right,
            cost: 0.08,
            gap: 0.2,
            ambiguous: false,
            short_quiet: false,
        };
        let mut rows = vec![obs(0.0005, 0.0955), obs(0.1205, 0.2155)];
        let mut w = Work::default();
        apply(
            &e,
            false,
            &[evidence(true, 0., 95., 0.00001).read],
            &mut rows,
            0,
            0,
            0.5,
            0.,
            1.,
            1000,
            &mut w,
        );
        assert_eq!(w.invalid_veto_reads, 1);
        assert_eq!(w.invalid_soft_conflicts, 1);
        assert!(rows.iter().any(|o| !o.ambiguous && o.left == 0.1205));
        assert!(rows.iter().any(|o| o.ambiguous));
    }
}
#[cfg(test)]
mod image_tests {
    use crate::{ean, experiment::Experiment, multi_scan::Policy, sampling::ImageView};
    #[test]
    fn clean_invalid_neighbor_never_removes_disjoint_equal_valid_instances() {
        let bad = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 8];
        let good = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        assert!(!ean::checksum(&bad));
        assert!(ean::checksum(&good));
        let (width, height) = (1500, 160);
        let mut pixels = vec![255u8; width * height];
        for (left, d) in [(48, bad), (548, good), (1048, good)] {
            let bits = ean::encode(&d);
            for y in 15..145 {
                for x in 0..285 {
                    if bits[x / 3] > 0.5 {
                        pixels[y * width + left + x] = 0;
                    }
                }
            }
        }
        for reverse in [false, true] {
            if reverse {
                pixels.reverse();
            }
            let im = ImageView::new(&pixels, width, height, 1, width).unwrap();
            let quad = [[0., 0.], [1499., 0.], [1499., 159.], [0., 159.]];
            let p = Policy {
                transition_cleanup: true,
                source_identity: true,
                interior_normalization: true,
                guard_bias: true,
                ..Policy::default()
            };
            let f = Experiment::default().scan_frame(im, &[quad], p).unwrap();
            assert_eq!(f.barcodes.len(), 2);
            assert!(f.barcodes.iter().all(|b| b.detection.digits == good));
            assert_ne!(
                f.barcodes[0].detection.polygon,
                f.barcodes[1].detection.polygon
            );
        }
    }
}

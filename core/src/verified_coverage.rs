//! Optional conservative subtraction of already verified source-pixel bands.
//! Claims never come from text-only identity or unsupported observations.
use super::{
    experiment, project_claim, projected_interval, scan, Candidate, Error, ImageView, Quad,
    Segment, Work,
};
#[cfg(test)]
use super::{Experiment, Policy};

#[derive(Default)]
pub(super) struct ReuseBudget {
    pub(super) remaining: usize,
    pub(super) pixels_remaining: usize,
    pub(super) cache: Vec<(Quad, Quad)>,
}
impl ReuseBudget {
    fn check(&mut self, work: &mut Work) -> bool {
        if self.remaining == 0 {
            work.reuse_checks_capped += 1;
            return false;
        }
        self.remaining -= 1;
        work.reuse_checks += 1;
        true
    }
}
fn bbox(q: &[[f64; 2]]) -> [f64; 4] {
    q.iter().fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        },
    )
}
fn bbox_overlap(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}
// Fixed-pass verified detections are retained by scan_policy until final assembly.
// The observations include mandatory native discovery and, when refreshed, all
// later reads. Any spatial contradiction disables that claim, including unknown
// (ambiguous) digits. Invalid evidence conservatively disables all optimization.
pub(super) fn verified_claims(
    outputs: &[Candidate],
    budget: &mut ReuseBudget,
    work: &mut Work,
    im: ImageView<'_>,
) -> Vec<Quad> {
    let mut detections = Vec::new();
    let mut detection_boxes = Vec::new();
    let mut observations = Vec::new();
    let all_detections: Vec<_> = outputs.iter().flat_map(|c| c.detections.iter()).collect();
    let all_detection_boxes: Vec<_> = all_detections.iter().map(|d| bbox(&d.polygon)).collect();
    for c in outputs {
        for d in &c.detections {
            if !c.error && c.work.association_truncated == 0 && d.support >= 2 {
                if scan::transform(d.polygon).is_err() {
                    continue;
                }
                detections.push(d);
                detection_boxes.push(bbox(&d.polygon));
            }
        }
        let Ok(m) = scan::transform(c.coverage) else {
            if !c.observations.is_empty() {
                return vec![];
            }
            continue;
        };
        for o in &c.observations {
            #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
            {
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                if o.invalid_checksum {
                    continue;
                }
            }
            let (Ok(a), Ok(b)) = (
                experiment::point(m.0, o.axis, o.left, o.fraction),
                experiment::point(m.0, o.axis, o.right, o.fraction),
            ) else {
                return vec![];
            };
            let line = [a, b];
            observations.push((o, line, bbox(&line)));
        }
    }
    let mut claims = Vec::new();
    'claim: for (d, original_box) in detections.iter().zip(&detection_boxes) {
        let q = extend_claim(im, d.polygon, work, budget);

        let db = if q == d.polygon {
            *original_box
        } else {
            bbox(&q)
        };
        for (other, other_box) in all_detections.iter().zip(&all_detection_boxes) {
            if other.digits == d.digits {
                continue;
            }
            if !budget.check(work) {
                return vec![];
            }
            if bbox_overlap(db, *other_box)
                && crate::association::overlap(&q, &other.polygon).unwrap_or(1.) > 0.
            {
                work.reuse_claims_rejected += 1;
                continue 'claim;
            }
        }
        for (o, line, line_box) in &observations {
            if !o.ambiguous && o.digits == d.digits {
                continue;
            }
            if !budget.check(work) {
                return vec![];
            }
            if bbox_overlap(db, *line_box)
                && crate::association::overlap(&q, line).unwrap_or(1.) > 0.
            {
                work.reuse_claims_rejected += 1;
                continue 'claim;
            }
        }
        // Exact geometry duplicates only. Equal text at separate locations never
        // merges claims and never establishes cross-location identity.
        if !claims.contains(&q) {
            claims.push(q);
        }
    }
    work.reuse_claims += claims.len();
    claims
}

// Coverage evidence only; never changes a reported detection or supplies digits.

fn extension_row(
    im: ImageView<'_>,
    edge: [[f64; 2]; 2],
    work: &mut Work,
    budget: &mut ReuseBudget,
) -> Option<[f64; 192]> {
    for p in edge {
        if !p[0].is_finite()
            || !p[1].is_finite()
            || p[0] < 0.
            || p[1] < 0.
            || p[0] > crate::numeric::usize_f64(im.width) - 1.
            || p[1] > crate::numeric::usize_f64(im.height) - 1.
        {
            return None;
        }
    }
    if budget.pixels_remaining < 192 {
        work.extension_capped += 1;
        return None;
    }
    budget.pixels_remaining -= 192;
    work.extension_samples += 192;
    let mut r = [0.; 192];
    let (mut lo, mut hi) = (255f64, 0f64);
    for (i, v) in r.iter_mut().enumerate() {
        let t = (crate::numeric::usize_f64(i) + 0.5) / 192.;
        let x = (edge[0][0] + t * (edge[1][0] - edge[0][0])).round();
        let y = (edge[0][1] + t * (edge[1][1] - edge[0][1])).round();
        // Endpoints were checked above; convex interpolation remains in bounds.
        *v = if im.channels == 1 {
            f64::from(
                im.data[crate::numeric::f64_usize(y) * im.stride + crate::numeric::f64_usize(x)],
            )
        } else {
            im.gray(x, y)
        };
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if hi - lo < 8. {
        return None;
    }
    let mean = r.iter().sum::<f64>() / 192.;
    let mut norm = 0.;
    for v in &mut r {
        *v -= mean;
        norm += *v * *v;
    }
    if norm < 1. {
        return None;
    }
    let norm = norm.sqrt();
    for v in &mut r {
        *v /= norm;
    }
    Some(r)
}

fn extension_correlation(
    im: ImageView<'_>,
    reference: &[f64; 192],
    edge: [[f64; 2]; 2],
    work: &mut Work,
    budget: &mut ReuseBudget,
) -> Option<f64> {
    for p in edge {
        if !p[0].is_finite()
            || !p[1].is_finite()
            || p[0] < 0.
            || p[1] < 0.
            || p[0] > crate::numeric::usize_f64(im.width) - 1.
            || p[1] > crate::numeric::usize_f64(im.height) - 1.
        {
            return None;
        }
    }
    if budget.pixels_remaining < 192 {
        work.extension_capped += 1;
        return None;
    }
    budget.pixels_remaining -= 192;
    work.extension_samples += 192;
    let mut r = [0.; 192];
    let (mut lo, mut hi) = (255f64, 0f64);
    for (i, v) in r.iter_mut().enumerate() {
        let t = (crate::numeric::usize_f64(i) + 0.5) / 192.;
        let x = (edge[0][0] + t * (edge[1][0] - edge[0][0])).round();
        let y = (edge[0][1] + t * (edge[1][1] - edge[0][1])).round();
        // Endpoints were checked above; convex interpolation remains in bounds.
        *v = if im.channels == 1 {
            f64::from(
                im.data[crate::numeric::f64_usize(y) * im.stride + crate::numeric::f64_usize(x)],
            )
        } else {
            im.gray(x, y)
        };
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if hi - lo < 8. {
        return None;
    }
    let mean = r.iter().sum::<f64>() / 192.;
    let mut norm = 0.;
    let mut dot = 0.;
    for (v, a) in r.iter_mut().zip(reference) {
        *v -= mean;
        norm += *v * *v;
        dot += a * *v;
    }
    if norm < 1. {
        return None;
    }
    let norm = norm.sqrt();
    let corr = dot / norm;
    // Preserve the original decision if floating-point reassociation is close
    // enough to the threshold to matter. No sampling or budget changes.
    if (corr - 0.98).abs() < 1e-12 {
        for v in &mut r {
            *v /= norm;
        }
        return Some(reference.iter().zip(r).map(|(a, b)| a * b).sum());
    }
    Some(corr)
}

fn extend_claim(im: ImageView<'_>, quad: Quad, work: &mut Work, budget: &mut ReuseBudget) -> Quad {
    if let Some((_, out)) = budget.cache.iter().find(|(key, _)| *key == quad) {
        work.extension_cache_hits += 1;
        return *out;
    }
    let width = experiment::distance(quad[0], quad[1]).min(experiment::distance(quad[3], quad[2]));
    let v = [
        (quad[3][0] - quad[0][0]) + (quad[2][0] - quad[1][0]),
        (quad[3][1] - quad[0][1]) + (quad[2][1] - quad[1][1]),
    ];
    let count = v[0].hypot(v[1]);
    if width < 76. || count < 1e-9 {
        return quad;
    }
    let v = [v[0] / count, v[1] / count];
    let steps = crate::numeric::f64_usize((width * 0.4).min(128.).mul_add(2., 0.).floor());
    let mut extent = [0.; 2];
    for (side, edge) in [[quad[0], quad[1]], [quad[3], quad[2]]]
        .into_iter()
        .enumerate()
    {
        let Some(reference) = extension_row(im, edge, work, budget) else {
            continue;
        };
        let sign = if side == 0 { -1. } else { 1. };
        for step in 1..=steps {
            let d = crate::numeric::usize_f64(step) * 0.5;
            let e = edge.map(|p| [p[0] + sign * d * v[0], p[1] + sign * d * v[1]]);
            let Some(corr) = extension_correlation(im, &reference, e, work, budget) else {
                break;
            };
            if corr < 0.98 {
                break;
            }
            extent[side] = d;
        }
    }
    let mut out = quad;
    for i in 0..4 {
        let side = usize::from(i >= 2);
        let sign = if side == 0 { -1. } else { 1. };
        out[i] = [
            quad[i][0] + sign * extent[side] * v[0],
            quad[i][1] + sign * extent[side] * v[1],
        ];
    }
    if scan::transform(out).is_err() {
        return quad;
    }
    work.extension_claims += usize::from(out != quad);
    if budget.cache.len() < 128 {
        budget.cache.push((quad, out));
    }
    out
}

fn reuse_projected_segment(
    m: [f64; 9],
    s: Segment,
    claims: &[Option<Quad>],
    budget: &mut ReuseBudget,
    work: &mut Work,
) -> Result<Vec<Segment>, Error> {
    let mut intervals = Vec::new();
    for q in claims {
        if !budget.check(work) {
            return Ok(vec![s]);
        }
        if let Some((a, b)) = q.and_then(|q| projected_interval(q, s.axis, s.fraction)) {
            let (a, b) = (a.max(s.lo), b.min(s.hi));
            if b > a {
                intervals.push((a, b));
            }
        }
    }
    if intervals.is_empty() {
        return Ok(vec![s]);
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut outside = Vec::new();
    let mut lo = s.lo;
    for (a, b) in intervals {
        if a > lo {
            outside.push((lo, a));
        }
        lo = lo.max(b);
    }
    if lo < s.hi {
        outside.push((lo, s.hi));
    }
    let mut pieces = Vec::new();
    let mut too_short = 0;
    for (lo, hi) in outside {
        let length = experiment::distance(
            experiment::point(m, s.axis, lo, s.fraction)?,
            experiment::point(m, s.axis, hi, s.fraction)?,
        );
        if length < 76. {
            too_short += 1;
            continue;
        }
        pieces.push(Segment {
            lo,
            hi,
            samples: crate::numeric::f64_usize(length.ceil().clamp(64., 4096.)),
            sample_cap: length > 4096.,
            unresolved: true,
            ..s
        });
    }
    work.reuse_paths_changed += 1;
    work.reuse_paths_removed += usize::from(pieces.is_empty());
    work.reuse_paths_split += pieces.len().saturating_sub(1);
    work.reuse_short_pieces += too_short;
    Ok(pieces)
}
pub(super) fn reuse_plan(
    m: [f64; 9],
    paths: Vec<Segment>,
    claims: &[Quad],
    budget: &mut ReuseBudget,
    work: &mut Work,
) -> Vec<Segment> {
    if claims.is_empty() {
        return paths;
    }
    let projected: Vec<_> = claims.iter().map(|q| project_claim(m, *q)).collect();
    let mut out = Vec::new();
    for s in paths {
        let pieces =
            reuse_projected_segment(m, s, &projected, budget, work).unwrap_or_else(|_| vec![s]);
        // The planner already counted this original segment. Unmaterialized
        // paths stay pending; only these concrete replaced paths are adjusted.
        work.retry_paths_pending = work.retry_paths_pending.saturating_sub(1) + pieces.len();
        out.extend(pieces);
    }
    out
}

#[cfg(test)]
fn reuse_segment(
    m: [f64; 9],
    s: Segment,
    claims: &[Quad],
    budget: &mut ReuseBudget,
    work: &mut Work,
) -> Result<Vec<Segment>, Error> {
    let projected: Vec<_> = claims.iter().map(|q| project_claim(m, *q)).collect();
    reuse_projected_segment(m, s, &projected, budget, work)
}
#[cfg(test)]
mod reuse_tests {
    use super::*;
    fn quad(x: f64, y: f64, w: f64, h: f64) -> Quad {
        [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
    }
    fn candidate(q: Quad, d: Vec<experiment::Detection>) -> Candidate {
        Candidate {
            index: 0,
            coverage: q,
            observations: vec![],
            detections: d,
            work: Work::default(),
            ms: 0.,
            error: false,
        }
    }
    fn detection(q: Quad, value: u8) -> experiment::Detection {
        experiment::Detection {
            digits: [value; 13],
            polygon: q,
            support: 3,
            axis: 0,
        }
    }
    fn budget() -> ReuseBudget {
        ReuseBudget {
            remaining: 200_000,
            pixels_remaining: 262_144,
            ..Default::default()
        }
    }
    #[test]
    fn equal_and_distinct_symbols_claim_only_their_actual_bands() {
        let q = quad(0., 0., 1200., 600.);
        let matrix = scan::transform(q).unwrap().0;
        for value in [1, 2] {
            let a = quad(100., 200., 200., 100.);
            let b = quad(700., 200., 200., 100.);
            let outputs = vec![
                candidate(q, vec![detection(a, 1)]),
                candidate(q, vec![detection(b, value)]),
            ];
            let claims = verified_claims(
                &outputs,
                &mut budget(),
                &mut Work::default(),
                ImageView::new(&[255], 1, 1, 1, 1).unwrap(),
            );
            assert_eq!(claims.len(), 2);
            let segment = Segment {
                axis: 0,
                fraction: 0.4,
                lo: 0.,
                hi: 1.,
                samples: 1200,
                sample_cap: false,
                unresolved: false,
            };
            let ps = reuse_segment(
                matrix,
                segment,
                &claims,
                &mut budget(),
                &mut Work::default(),
            )
            .unwrap();
            assert_eq!(ps.len(), 3);
            // Inter-symbol region and both exterior regions remain attempted.
            assert!(ps.iter().any(|p| p.lo < 0.26 && p.hi > 0.57));
            let outside = Segment {
                fraction: 0.8,
                ..segment
            };
            let ps = reuse_segment(
                matrix,
                outside,
                &claims,
                &mut budget(),
                &mut Work::default(),
            )
            .unwrap();
            assert_eq!(ps.len(), 1);
            assert_eq!((ps[0].lo, ps[0].hi), (0., 1.));
            let other_axis = Segment {
                axis: 1,
                fraction: 0.4,
                ..segment
            };
            let ps = reuse_segment(
                matrix,
                other_axis,
                &claims,
                &mut budget(),
                &mut Work::default(),
            )
            .unwrap();
            assert_eq!(ps.len(), 1);
        }
    }
    #[test]
    fn conflicting_or_ambiguous_evidence_never_claims_coverage() {
        let q = quad(0., 0., 1000., 500.);
        let band = quad(100., 100., 500., 200.);
        let cs = vec![
            candidate(q, vec![detection(band, 1)]),
            candidate(q, vec![detection(band, 2)]),
        ];
        assert!(verified_claims(
            &cs,
            &mut budget(),
            &mut Work::default(),
            ImageView::new(&[255], 1, 1, 1, 1).unwrap()
        )
        .is_empty());
        for ambiguous in [false, true] {
            let mut c = candidate(q, vec![detection(band, 1)]);
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    c.observations.push(experiment::Observation {
                        short_quiet: false,
                        ambiguous,
                        digits: if ambiguous { [1; 13] } else { [2; 13] },
                        axis: 0,
                        fraction: 0.4,
                        left: 0.1,
                        right: 0.6,
                        cost: 0.,
                        gap: 1.,
                    });
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    c.observations.push(experiment::Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous,
                        digits: if ambiguous { [1; 13] } else { [2; 13] },
                        axis: 0,
                        fraction: 0.4,
                        left: 0.1,
                        right: 0.6,
                        cost: 0.,
                        gap: 1.,
                    });
                }
            }

            assert!(verified_claims(
                &[c],
                &mut budget(),
                &mut Work::default(),
                ImageView::new(&[255], 1, 1, 1, 1).unwrap()
            )
            .is_empty());
        }
    }
    #[test]
    fn splitting_and_cap_fallback_preserve_pending_work() {
        let m = scan::transform(quad(0., 0., 1000., 500.)).unwrap().0;
        let s = Segment {
            axis: 0,
            fraction: 0.4,
            lo: 0.,
            hi: 1.,
            samples: 1000,
            sample_cap: false,
            unresolved: false,
        };
        let mut w = Work {
            retry_paths_pending: 8,
            ..Work::default()
        };
        let p = reuse_plan(
            m,
            vec![s],
            &[quad(400., 100., 200., 200.)],
            &mut budget(),
            &mut w,
        );
        assert_eq!(p.len(), 2);
        assert_eq!(w.retry_paths_pending, 9);
        assert_eq!(w.reuse_paths_split, 1);
        // Executing only one split under a limit leaves the second plus all
        // seven unmaterialized originals pending; no claim of completion.
        w.retry_paths_pending -= 1;
        assert_eq!(w.retry_paths_pending, 8);
        let mut zero = ReuseBudget::default();
        let p = reuse_segment(m, s, &[quad(0., 0., 1000., 500.)], &mut zero, &mut w).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].lo, p[0].hi), (s.lo, s.hi));
        assert!(w.reuse_checks_capped > 0);
    }
    #[test]
    fn actual_two_symbols_survive_overlapping_candidates_and_all_discovery() {
        let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        for second in [a, b] {
            let mut pixels = vec![255u8; 1000 * 240];
            for (left, digits) in [(50, a), (550, second)] {
                let bits = crate::ean::encode(&digits);
                for y in 40..200 {
                    for x in 0..380 {
                        if bits[x / 4] > 0.5 {
                            pixels[y * 1000 + left + x] = 0;
                        }
                    }
                }
            }
            let im = ImageView::new(&pixels, 1000, 240, 1, 1000).unwrap();
            let qs = [quad(0., 0., 1000., 240.), quad(0., 20., 1000., 200.)];
            let policy = Policy {
                transition_cleanup: true,
                interior_normalization: true,
                guard_bias: true,
                ..Policy::default()
            };
            let mut ex = Experiment::default();
            let result = ex.scan_scaled(im, &qs, policy).unwrap();
            assert_eq!(result.len(), 2);
            for (c, q) in result.iter().zip(qs) {
                assert_eq!(c.coverage, q);
                #[cfg(feature = "mode-low")]
                {
                    assert!(
                        (5..=10).contains(&c.work.discovery_paths),
                        "both normalized axes and at least five native module-axis rows"
                    );
                }
                #[cfg(feature = "mode-low")]
                {
                    assert!(matches!(c.work.paths - c.work.retry_paths, 3 | 6));
                }
                #[cfg(any(feature = "mode-medium", feature = "mode-high"))]
                {
                    assert_eq!(c.work.discovery_paths, 10);
                }
                #[cfg(feature = "mode-medium")]
                {
                    assert_eq!(c.work.paths, c.work.retry_paths + 6);
                }
                #[cfg(feature = "mode-very-high")]
                {
                    assert_eq!(c.work.discovery_requests, 10);
                }
                #[cfg(feature = "mode-very-high")]
                {
                    assert!(c.work.discovery_paths >= 4);
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    assert_eq!(c.work.paths, c.work.retry_paths + 10);
                }
                assert!(c
                    .detections
                    .iter()
                    .any(|d| d.digits == a && d.polygon.iter().all(|p| p[0] < 500.)));
                assert!(c
                    .detections
                    .iter()
                    .any(|d| d.digits == second && d.polygon.iter().all(|p| p[0] > 500.)));
            }
            assert!(
                result
                    .iter()
                    .map(|c| c.work.reuse_paths_changed)
                    .sum::<usize>()
                    > 0
            );
        }
    }
}

#[cfg(test)]
mod extension_tests {
    use super::*;
    fn q() -> Quad {
        [[20., 40.], [400., 40.], [400., 60.], [20., 60.]]
    }
    fn pixels() -> Vec<u8> {
        let mut p = vec![255; 420 * 140];
        for y in 20..110 {
            for x in 20..400 {
                p[y * 420 + x] = if (x / 4) % 3 == 0 { 0 } else { 255 }
            }
        }
        p
    }
    fn budget() -> ReuseBudget {
        ReuseBudget {
            remaining: 200_000,
            pixels_remaining: 262_144,
            ..Default::default()
        }
    }
    fn image(p: &[u8]) -> ImageView<'_> {
        ImageView::new(p, 420, 140, 1, 420).unwrap()
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn extends_across_bars_not_into_the_reading_direction() {
        let p = pixels();
        let mut w = Work::default();
        let mut b = budget();
        let out = extend_claim(image(&p), q(), &mut w, &mut b);
        assert!(out[0][1] < 21. && out[3][1] > 108., "{out:?}");
        for (i, out_entry) in out.iter().enumerate() {
            assert_eq!((*out_entry)[0], q()[i][0]);
        }
        assert_eq!(w.extension_claims, 1);
        assert!(w.extension_samples > 192);
        assert_eq!(262_144 - b.pixels_remaining, w.extension_samples);
        assert_eq!(w.association_checks, 0);
        assert_eq!(w.continuity_samples, 0);
    }
    #[test]
    fn stops_at_one_pixel_gap_changed_pattern_and_source_border() {
        for changed in [false, true] {
            let mut p = pixels();
            for x in 0..420 {
                p[80 * 420 + x] = if changed && (x / 4) % 3 == 1 { 0 } else { 255 }
            }
            let out = extend_claim(image(&p), q(), &mut Work::default(), &mut budget());
            assert!(out[2][1] < 80., "{out:?}");
            assert!(out[2][1] > 78.);
        }
        let p = pixels();
        let outside = [[-5., 40.], [400., 40.], [400., 60.], [-5., 60.]];
        assert_eq!(
            extend_claim(image(&p), outside, &mut Work::default(), &mut budget()),
            outside
        );
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn rotation_preserves_axis_and_blank_gap_veto() {
        let mut p = pixels();
        p[80 * 420..81 * 420].fill(255);
        let mut rot = vec![255; p.len()];
        for y in 0..140 {
            for x in 0..420 {
                rot[x * 140 + 139 - y] = p[y * 420 + x];
            }
        }
        let turn = |p: [f64; 2]| [139. - p[1], p[0]];
        let out = extend_claim(
            ImageView::new(&rot, 140, 420, 1, 140).unwrap(),
            q().map(turn),
            &mut Work::default(),
            &mut budget(),
        );
        assert!(out[2][0] > 59. && out[2][0] < 61., "{out:?}");
        for (i, out_entry) in out.iter().enumerate() {
            assert_eq!((*out_entry)[1], q()[i][0]);
        }
    }
    #[test]
    fn separate_pixel_budget_retains_unextended_claim() {
        let p = pixels();
        let mut b = ReuseBudget {
            remaining: 200_000,
            pixels_remaining: 192,
            ..Default::default()
        };
        let mut w = Work::default();
        let out = extend_claim(image(&p), q(), &mut w, &mut b);
        assert_eq!(out, q());
        assert_eq!(w.extension_samples, 192);
        assert!(w.extension_capped > 0);
        assert_eq!(b.remaining, 200_000);
    }
    #[test]
    fn extension_must_pass_new_conflict_and_ambiguity_checks() {
        let p = pixels();
        let coverage = [[0., 0.], [420., 0.], [420., 140.], [0., 140.]];
        let make = || Candidate {
            index: 0,
            coverage,
            observations: vec![],
            detections: vec![experiment::Detection {
                digits: [1; 13],
                polygon: q(),
                support: 3,
                axis: 0,
            }],
            work: Work::default(),
            ms: 0.,
            error: false,
        };
        let c = make();
        let claims = verified_claims(&[c], &mut budget(), &mut Work::default(), image(&p));
        assert_eq!(claims.len(), 1);
        assert!(claims[0][2][1] > 100.);
        for ambiguous in [false, true] {
            let mut c = make();
            {
                #[cfg(any(feature = "mode-low", feature = "mode-medium"))]
                {
                    c.observations.push(experiment::Observation {
                        short_quiet: false,
                        ambiguous,
                        digits: if ambiguous { [1; 13] } else { [2; 13] },
                        axis: 0,
                        fraction: 75. / 140.,
                        left: 20. / 420.,
                        right: 400. / 420.,
                        cost: 0.,
                        gap: 1.,
                    });
                }
                #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                {
                    c.observations.push(experiment::Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous,
                        digits: if ambiguous { [1; 13] } else { [2; 13] },
                        axis: 0,
                        fraction: 75. / 140.,
                        left: 20. / 420.,
                        right: 400. / 420.,
                        cost: 0.,
                        gap: 1.,
                    });
                }
            }

            assert!(
                verified_claims(&[c], &mut budget(), &mut Work::default(), image(&p)).is_empty()
            );
        }
        let mut c = make();
        c.detections.push(experiment::Detection {
            digits: [2; 13],
            polygon: [[20., 75.], [400., 75.], [400., 76.], [20., 76.]],
            support: 3,
            axis: 0,
        });
        assert!(verified_claims(&[c], &mut budget(), &mut Work::default(), image(&p)).is_empty());
    }
}

// Independent regression fixtures: real EAN modules, source-rasterized gap,
// and transformed stacked physical symbols (including repeated payloads).
#[cfg(test)]
mod stacked_extension_regressions {
    use super::*;
    const SIZE: usize = 640;
    const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    #[derive(Clone, Copy)]
    struct Geometry {
        angle: f64,
        shear: f64,
        perspective: f64,
    }
    impl Geometry {
        fn point(self, u: f64, v: f64) -> [f64; 2] {
            let den = 1. + self.perspective * u;
            let (x, y) = ((u + self.shear * v) / den, v / den);
            let (sin_angle, cos_angle) = self.angle.sin_cos();
            [
                320. + cos_angle * x - sin_angle * y,
                320. + sin_angle * x + cos_angle * y,
            ]
        }
        fn rotated(self, p: [f64; 2]) -> [f64; 2] {
            let (sin_angle, cos_angle) = self.angle.sin_cos();
            let (x, y) = (p[0] - 320., p[1] - 320.);
            [
                cos_angle * x + sin_angle * y,
                -sin_angle * x + cos_angle * y,
            ]
        }
        fn inverse(self, point: [f64; 2]) -> [f64; 2] {
            let [x, y] = self.rotated(point);
            let z = x - self.shear * y;
            let u = z / (1. - self.perspective * z);
            [u, y * (1. + self.perspective * u)]
        }
        fn quad(self, top: f64, bottom: f64) -> Quad {
            [
                self.point(-190., top),
                self.point(190., top),
                self.point(190., bottom),
                self.point(-190., bottom),
            ]
        }
    }
    fn geometries() -> [Geometry; 3] {
        [
            Geometry {
                angle: 0.,
                shear: 0.,
                perspective: 0.,
            },
            Geometry {
                angle: 40f64.to_radians(),
                shear: 0.,
                perspective: 0.,
            },
            Geometry {
                angle: 40f64.to_radians(),
                shear: 0.12,
                perspective: 0.00035,
            },
        ]
    }
    fn raster(g: Geometry, second: [u8; 13]) -> Vec<u8> {
        let patterns = [crate::ean::encode(&A), crate::ean::encode(&second)];
        let mut pixels = vec![255; SIZE * SIZE];
        for y in 0..SIZE {
            for x in 0..SIZE {
                let p = [crate::numeric::usize_f64(x), crate::numeric::usize_f64(y)];
                let [u, v] = g.inverse(p);
                // The gap is exactly one source pixel in continuous perpendicular
                // distance, independently of perspective scaling of local v.
                if g.rotated(p)[1].abs() < 0.5 || !(-190. ..190.).contains(&u) || v.abs() > 65. {
                    continue;
                }
                let module = crate::numeric::f64_usize(((u + 190.) / 4.).floor());
                if patterns[usize::from(v > 0.)][module] > 0.5 {
                    pixels[y * SIZE + x] = 0;
                }
            }
        }
        pixels
    }
    fn budget() -> ReuseBudget {
        ReuseBudget {
            remaining: 200_000,
            pixels_remaining: 262_144,
            ..Default::default()
        }
    }
    #[test]
    fn stacked_real_symbols_do_not_extend_across_source_pixel_gap() {
        for g in geometries() {
            for second in [A, B] {
                let pixels = raster(g, second);
                let im = ImageView::new(&pixels, SIZE, SIZE, 1, SIZE).unwrap();
                for (top, bottom, sign) in [(-35., -20., -1.), (20., 35., 1.)] {
                    let q = g.quad(top, bottom);
                    let mut work = Work::default();
                    let out = extend_claim(im, q, &mut work, &mut budget());
                    assert!(out.iter().all(|p|sign*g.rotated(*p)[1]>0.),
                    "claim crossed gap: angle={} shear={} second={second:?}, original={q:?}, extended={out:?}",g.angle,g.shear);
                    assert!(out.iter().all(|p| p
                        .iter()
                        .all(|v| *v >= 0. && *v < crate::numeric::usize_f64(SIZE))));
                    assert!(work.extension_samples <= 262_144);
                    // A no-op is safe but cannot satisfy this regression alone.
                    if g.angle == 0. {
                        assert!(
                            work.extension_claims > 0,
                            "axis-aligned fixture must exercise actual extension"
                        );
                    }
                    eprintln!(
                        "extension angle={} shear={} sign={sign}: grown={} samples={}",
                        g.angle, g.shear, work.extension_claims, work.extension_samples
                    );
                }
            }
        }
    }
    #[test]
    fn stacked_real_symbols_remain_separate_in_scaled_scan() {
        let mut failures = Vec::new();
        for g in geometries() {
            for second in [A, B] {
                let pixels = raster(g, second);
                let im = ImageView::new(&pixels, SIZE, SIZE, 1, SIZE).unwrap();
                // Both candidates see both physical symbols. Native discovery runs
                // before reuse; no identity grouping is supplied to the scanner.
                let qs = [g.quad(-75., 75.), g.quad(-70., 70.)];
                let policy = Policy {
                    transition_cleanup: true,
                    interior_normalization: true,
                    guard_bias: true,
                    ..Policy::default()
                };
                let result = Experiment::default().scan_scaled(im, &qs, policy).unwrap();
                for c in result {
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
                    for (digits, sign) in [(A, -1.), (second, 1.)] {
                        if !c.detections.iter().any(|d| {
                            d.digits == digits
                                && d.polygon.iter().all(|p| sign * g.rotated(*p)[1] > 0.)
                        }) {
                            failures.push(format!("missing stacked instance: angle={} shear={} sign={sign} equal={} detections={:?}",g.angle,g.shear,second==A,c.detections));
                        }
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
    #[test]
    fn cached_extension_spends_no_additional_pixels() {
        let g = geometries()[0];
        let pixels = raster(g, A);
        let im = ImageView::new(&pixels, SIZE, SIZE, 1, SIZE).unwrap();
        let mut b = budget();
        let mut w = Work::default();
        let q = g.quad(-35., -20.);
        let first = extend_claim(im, q, &mut w, &mut b);
        let samples = w.extension_samples;
        let remaining = b.pixels_remaining;
        assert!(w.extension_claims > 0);
        assert_eq!(extend_claim(im, q, &mut w, &mut b), first);
        assert_eq!(w.extension_samples, samples);
        assert_eq!(b.pixels_remaining, remaining);
        assert_eq!(w.extension_cache_hits, 1);
    }
}

#[cfg(test)]
#[path = "coverage_arithmetic_tests.rs"]
mod coverage_arithmetic_tests;

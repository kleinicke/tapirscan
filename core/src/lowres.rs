//! Selective, bounded multirow recovery of unresolved small EAN-13 candidates.
mod photo;
use crate::{
    candidate_scanner::{self, AssociationBudget, Candidate, Observation},
    sampling::ImageView,
    scan,
};

pub(crate) const MAX_ATTEMPTS: usize = if cfg!(feature = "mode-medium") {
    2
} else if cfg!(feature = "mode-high") {
    4
} else {
    6
};

fn eligible(candidate: &Candidate) -> bool {
    if candidate.error
        || !candidate.detections.is_empty()
        || candidate.work.low_contrast == candidate.work.paths
    {
        return false;
    }
    // Medium reuses evidence already gathered by ordinary decoding. Texture
    // without a guard, or repeated visually invalid checksum evidence, does
    // not justify another search. Higher efforts may revisit distorted reads.
    #[cfg(feature = "mode-medium")]
    if candidate.work.guard_pass == 0 || candidate.work.invalid_visual_seen >= 2 {
        return false;
    }
    true
}

pub(crate) fn recover(
    image: ImageView<'_>,
    candidate: &mut Candidate,
    attempts: &mut usize,
    association: &mut AssociationBudget,
) {
    if !eligible(candidate) {
        return;
    }
    for axis in 0..2 {
        let q = if axis == 0 {
            candidate.coverage
        } else {
            let q = candidate.coverage;
            [q[1], q[2], q[3], q[0]]
        };
        let width = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
        let height = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
        if !(45. ..=190.).contains(&width)
            || !(12. ..=110.).contains(&height)
            || width * height > 16000.
        {
            continue;
        }
        // Do not build/sort another pixel profile after this frame's bank is
        // exhausted. Existing stripe eligibility is sufficient to report work.
        if *attempts == 0 {
            candidate.work.retry_paths_pending += 1;
            return;
        }
        let regions = photo::regions(image, q, !cfg!(feature = "mode-medium"));
        for q in regions
            .into_iter()
            .take(if cfg!(feature = "mode-medium") { 1 } else { 2 })
        {
            if *attempts == 0 {
                candidate.work.retry_paths_pending += 1;
                return;
            }
            *attempts -= 1;
            candidate.work.retry_paths += 1;
            let mut output = photo::scan(image, q, false);
            candidate.work.decoder_calls += output.trials;
            candidate.work.samples += output.pixels;
            if cfg!(feature = "mode-very-high") && !photo::accepted(&output) && *attempts > 0 {
                *attempts -= 1;
                candidate.work.retry_paths += 1;
                output = photo::scan(image, q, true);
                candidate.work.decoder_calls += output.trials;
                candidate.work.samples += output.pixels;
            }
            if !photo::accepted(&output) {
                continue;
            }
            let h = output.hypotheses[0];
            if candidate
                .observations
                .iter()
                .any(|o| o.axis == axis && (o.ambiguous || o.digits != h.result.digits))
            {
                candidate.work.conflicts += 1;
                continue;
            }
            // These temporary anchors bound distributed pixel evidence; they do
            // not claim that either individual scanline decoded the full text.
            // Preserve cached observations and let ordinary association prove
            // source continuity and construct the physical/display geometry.
            let observations: Vec<_> = [0.225, 0.775]
                .into_iter()
                .map(|fraction| {
                    let offset = h.shear * (fraction - 0.5);
                    Observation {
                        #[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
                        invalid_checksum: false,
                        short_quiet: false,
                        ambiguous: false,
                        digits: h.result.digits,
                        axis: 0,
                        fraction,
                        left: (h.left - offset) / 95.,
                        right: (95. + h.right - offset) / 95.,
                        cost: h.result.cost,
                        gap: h.result.gap,
                    }
                })
                .collect();
            if let Ok(m) = scan::transform(q) {
                let mut detections = candidate_scanner::assemble_many_budget_options(
                    image,
                    m.0,
                    &observations,
                    &mut candidate.work,
                    true,
                    association,
                    false,
                );
                if !detections.is_empty() {
                    candidate.work.accepted_paths += 2;
                    for d in &mut detections {
                        d.axis = axis;
                    }
                    candidate.detections.extend(detections);
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        candidate_scanner::Work,
        ean,
        numeric::{f64_u8, f64_usize},
    };

    const DIGITS: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];

    fn fixture() -> (Vec<u8>, scan::Quad) {
        let (sine, cosine) = 7f64.to_radians().sin_cos();
        let at = |u: f64, v: f64| [30. + u * cosine - v * sine, 20. + u * sine + v * cosine];
        let q = [at(-18., 0.), at(113., 0.), at(113., 36.), at(-18., 36.)];
        let bits = ean::encode(&DIGITS);
        let mut bytes = vec![255; 160 * 100];
        for y in 0..100 {
            for x in 0..160 {
                let mut dark = 0;
                for sy in 0..8 {
                    for sx in 0..8 {
                        let dx = f64::from(x) + (f64::from(sx) + 0.5) / 8. - 30.;
                        let dy = f64::from(y) + (f64::from(sy) + 0.5) / 8. - 20.;
                        let u = cosine * dx + sine * dy;
                        let v = -sine * dx + cosine * dy;
                        if (0. ..95.).contains(&u)
                            && (0. ..36.).contains(&v)
                            && bits[f64_usize(u)] > 0.5
                        {
                            dark += 1;
                        }
                    }
                }
                bytes[usize::try_from(y * 160 + x).unwrap()] =
                    f64_u8(255. * (1. - f64::from(dark) / 64.));
            }
        }
        (bytes, q)
    }
    fn candidate(q: scan::Quad) -> Candidate {
        Candidate {
            index: 0,
            coverage: q,
            observations: vec![],
            detections: vec![],
            work: Work {
                paths: 1,
                guard_pass: 1,
                ..Work::default()
            },
            ms: 0.,
            error: false,
        }
    }
    fn budget() -> AssociationBudget {
        AssociationBudget {
            checks_left: 200_000,
            pixels_left: 2_000_000,
        }
    }

    #[test]
    fn coarse_bounds_recover_without_changing_search_coverage() {
        let (bytes, q) = fixture();
        let image = ImageView::new(&bytes, 160, 100, 1, 160).unwrap();
        let mut c = candidate(q);
        let mut attempts = MAX_ATTEMPTS;
        recover(image, &mut c, &mut attempts, &mut budget());
        assert_eq!(c.detections.len(), 1);
        assert_eq!(c.detections[0].digits, DIGITS);
        assert_eq!(c.coverage, q);
        let span = (c.detections[0].polygon[1][0] - c.detections[0].polygon[0][0])
            .hypot(c.detections[0].polygon[1][1] - c.detections[0].polygon[0][1]);
        assert!((90. ..100.).contains(&span));
        assert!(attempts < MAX_ATTEMPTS);
        let before = attempts;
        recover(image, &mut c, &mut attempts, &mut budget());
        assert_eq!(attempts, before);
        assert_eq!(c.detections.len(), 1);
    }

    #[test]
    fn exhausted_bank_defers_without_sampling_or_decoding() {
        let (bytes, q) = fixture();
        let image = ImageView::new(&bytes, 160, 100, 1, 160).unwrap();
        let mut c = candidate(q);
        recover(image, &mut c, &mut 0, &mut budget());
        assert!(c.detections.is_empty());
        assert_eq!(c.work.samples, 0);
        assert_eq!(c.work.decoder_calls, 0);
        assert_eq!(c.work.retry_paths_pending, 1);
    }

    #[cfg(feature = "mode-medium")]
    #[test]
    fn weak_or_repeated_invalid_evidence_does_not_enter_medium_recovery() {
        let (bytes, q) = fixture();
        let image = ImageView::new(&bytes, 160, 100, 1, 160).unwrap();
        for (guards, invalid) in [(0, 0), (3, 2)] {
            let mut c = candidate(q);
            c.work.guard_pass = guards;
            c.work.invalid_visual_seen = invalid;
            let mut attempts = MAX_ATTEMPTS;
            recover(image, &mut c, &mut attempts, &mut budget());
            assert_eq!(attempts, MAX_ATTEMPTS);
            assert_eq!(c.work.samples, 0);
            assert_eq!(c.work.decoder_calls, 0);
            assert!(c.detections.is_empty());
        }
    }
}

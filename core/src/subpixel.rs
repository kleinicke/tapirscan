//! Very High recovery of affine EAN-13 regions from original pixel areas.
//! A bounded bank fits legal patterns, then requires two disjoint image bands
//! to decode independently. It supplies observations to normal association;
//! checksum validity alone never creates a detection.
mod model;
mod sequence;
use crate::candidate_scanner::{Candidate, Observation};
use crate::sampling::ImageView;
use crate::scan::Quad;
use model::Settings;

/// Global work ceiling, including independent support fits. It also applies to
/// extended scans: these solves must not inherit an unlimited pixel budget.
pub(crate) const MAX_MODELS: usize = 64;
const BANK: usize = 17;
const MAX_ROI: f64 = 16_384.;

fn region(q: Quad, axis: usize) -> Option<Quad> {
    let q = if axis == 0 {
        q
    } else {
        [q[1], q[2], q[3], q[0]]
    };
    if q.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let a = [q[1][0] - q[0][0], q[1][1] - q[0][1]];
    let b = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
    let width = a[0].hypot(a[1]);
    let height = b[0].hypot(b[1]);
    // Below 0.3 px/module the earlier experiment contained unstable inversions.
    // Wider regions belong to existing readers, not this expensive fallback.
    if !(28.5..=114.).contains(&width) || !(12. ..=160.).contains(&height) {
        return None;
    }
    if a[0] * b[1] - a[1] * b[0] <= 0. {
        return None;
    }
    // The pixel kernel models a rectangle, not a perspective quadrilateral.
    if (a[0] * b[0] + a[1] * b[1]).abs() / width > 0.05
        || (q[2][0] - q[1][0] - b[0]).hypot(q[2][1] - q[1][1] - b[1]) > 0.05
    {
        return None;
    }
    let extent = |axis| {
        q.iter().map(|p| p[axis]).fold(f64::NEG_INFINITY, f64::max)
            - q.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min)
    };
    let margin = 10. * width / 95. + 2.;
    ((extent(0) + 2. * margin) * (extent(1) + 2. * margin) <= MAX_ROI).then_some(q)
}
fn band(q: Quad, low: f64, high: f64) -> Quad {
    let at = |a: usize, b: usize, t: f64| {
        [
            q[a][0] + t * (q[b][0] - q[a][0]),
            q[a][1] + t * (q[b][1] - q[a][1]),
        ]
    };
    [at(0, 3, low), at(1, 2, low), at(1, 2, high), at(0, 3, high)]
}

/// Costs reserve one retry slot per model, plus two support fits. Exhaustion
/// rejects incomplete banks rather than accepting an untested competing text.
pub(crate) fn recover(image: ImageView<'_>, c: &mut Candidate, models_left: &mut usize) {
    // A blank or structurally empty region does not create deferred model work.
    if c.error || !c.detections.is_empty() || c.work.max_run_count < 4 {
        return;
    }
    for axis in 0..2 {
        let Some(q) = region(c.coverage, axis) else {
            continue;
        };
        // Entire bank (three intensity responses, sharp reconstruction and four
        // sequence blur models), then at most two independent support solves.
        if *models_left < BANK {
            c.work.retry_paths_pending += 1;
            continue;
        }
        let mut accepted = None;
        let mut conflict = false;
        for gamma in [1., 0.45, 2.2] {
            for (sequence, sigma) in [
                (false, 0.),
                (true, 0.),
                (true, 0.15),
                (true, 0.3),
                (true, 0.5),
            ] {
                *models_left -= 1;
                c.work.retry_paths += 1;
                c.work.decoder_calls += 1;
                let settings = Settings {
                    sigma,
                    sequence,
                    gamma,
                    ..Settings::default()
                };
                if let Some(digits) = model::reconstruct_model(image, q, settings) {
                    if accepted.is_some_and(|(old, _)| old != digits) {
                        conflict = true;
                    }
                    if accepted.is_none() {
                        accepted = Some((digits, settings));
                    }
                }
            }
        }
        if conflict {
            c.work.conflicts += 1;
            continue;
        }
        let Some((digits, settings)) = accepted else {
            continue;
        };
        let mut supported = true;
        for (lo, hi) in [(0., 0.45), (0.55, 1.)] {
            *models_left -= 1;
            c.work.retry_paths += 1;
            c.work.decoder_calls += 1;
            supported &= model::reconstruct_model(image, band(q, lo, hi), settings) == Some(digits);
        }
        if !supported {
            continue;
        }
        // Do not overwrite already-observed conflicting/invalid visual evidence.
        if c.observations
            .iter()
            .any(|o| o.axis == axis && (o.ambiguous || o.invalid_checksum || o.digits != digits))
        {
            c.work.conflicts += 1;
            continue;
        }
        for fraction in [0.225, 0.775] {
            c.observations.push(Observation {
                invalid_checksum: false,
                short_quiet: false,
                ambiguous: false,
                digits,
                axis,
                fraction,
                left: 0.,
                right: 1.,
                cost: 0.1,
                gap: 0.,
            });
        }
        c.work.accepted_paths += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate_scanner::Work;
    const DIGITS: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    // Independent supersampled rasterizer; the production decoder never sees bits.
    fn fixture(angle: f64, second: Option<[u8; 13]>) -> (Vec<u8>, Quad) {
        let (cosine, sine) = (angle.cos(), angle.sin());
        let origin = [
            60. - 28.5 * cosine + 20. * sine,
            60. - 28.5 * sine - 20. * cosine,
        ];
        let at = |u: f64, v: f64| {
            [
                origin[0] + u * cosine - v * sine,
                origin[1] + u * sine + v * cosine,
            ]
        };
        let q = [at(0., 0.), at(57., 0.), at(57., 40.), at(0., 40.)];
        let first = crate::ean::encode(&DIGITS);
        let second = crate::ean::encode(&second.unwrap_or(DIGITS));
        let mut bytes = vec![255; 120 * 120];
        for y in 0..120 {
            for x in 0..120 {
                let mut dark = 0;
                for sy in 0..16 {
                    for sx in 0..16 {
                        let rx = f64::from(x) + (f64::from(sx) + 0.5) / 16. - origin[0];
                        let ry = f64::from(y) + (f64::from(sy) + 0.5) / 16. - origin[1];
                        let u = cosine * rx + sine * ry;
                        let v = -sine * rx + cosine * ry;
                        if (0. ..57.).contains(&u) && (0. ..40.).contains(&v) {
                            let bits = if v < 20. { first } else { second };
                            dark += usize::from(bits[crate::numeric::f64_usize(u / 0.6)] > 0.5);
                        }
                    }
                }
                bytes[usize::try_from(y * 120 + x).unwrap()] =
                    crate::numeric::f64_u8(255. * (1. - crate::numeric::usize_f64(dark) / 256.));
            }
        }
        (bytes, q)
    }
    fn candidate(q: Quad) -> Candidate {
        Candidate {
            index: 0,
            coverage: q,
            observations: vec![],
            detections: vec![],
            work: Work {
                max_run_count: 8,
                ..Work::default()
            },
            ms: 0.,
            error: false,
        }
    }
    #[test]
    fn subpixel_recovery_requires_disjoint_support_and_respects_budgets() {
        let (bytes, q) = fixture(7f64.to_radians(), None);
        let image = ImageView::new(&bytes, 120, 120, 1, 120).unwrap();
        let mut c = candidate(q);
        let mut budget = MAX_MODELS;
        recover(image, &mut c, &mut budget);
        assert_eq!(c.observations.len(), 2);
        assert!(c.observations.iter().all(|o| o.digits == DIGITS));
        assert!(budget < MAX_MODELS);
        let mut c = candidate(q);
        let mut budget = 16;
        recover(image, &mut c, &mut budget);
        assert!(c.observations.is_empty());
        assert_eq!(budget, 16);
        assert!(c.work.retry_paths_pending > 0);
    }
    #[test]
    fn region_pipeline_recovers_rotated_subpixel_code() {
        let (bytes, q) = fixture(7f64.to_radians(), None);
        let image = ImageView::new(&bytes, 120, 120, 1, 120).unwrap();
        let result = crate::region_scan::RegionScanner::default()
            .scan(image, &[q], crate::multi_scan::Policy::default())
            .unwrap();
        assert!(
            result
                .frame
                .barcodes
                .iter()
                .any(|b| b.detection.digits == DIGITS)
        );
    }
    #[test]
    fn conflicting_bands_and_blank_images_are_not_reads() {
        let (bytes, q) = fixture(
            7f64.to_radians(),
            Some([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]),
        );
        for pixels in [&bytes, &vec![255; 120 * 120]] {
            let image = ImageView::new(pixels, 120, 120, 1, 120).unwrap();
            let mut c = candidate(q);
            let mut budget = MAX_MODELS;
            recover(image, &mut c, &mut budget);
            assert!(c.observations.is_empty());
        }
    }
    #[test]
    fn rgba_padding_and_retry_mask_preserve_the_contract() {
        let (bytes, q) = fixture(7f64.to_radians(), None);
        let stride = 120 * 4 + 13;
        let mut rgba = vec![17; stride * 120];
        for y in 0..120 {
            for x in 0..120 {
                let value = bytes[y * 120 + x];
                rgba[y * stride + x * 4..y * stride + x * 4 + 4]
                    .copy_from_slice(&[value, value, value, 255]);
            }
        }
        let image = ImageView::new(&rgba, 120, 120, 4, stride).unwrap();
        let mut candidate = candidate(q);
        let mut budget = MAX_MODELS;
        recover(image, &mut candidate, &mut budget);
        assert_eq!(candidate.observations.len(), 2);
        assert!(candidate.observations.iter().all(|o| o.digits == DIGITS));
        let policy = crate::multi_scan::Policy {
            candidate_retry_mask: 0,
            ..crate::multi_scan::Policy::default()
        };
        let result = crate::region_scan::RegionScanner::default()
            .scan(image, &[q], policy)
            .unwrap();
        assert!(result.frame.barcodes.is_empty());
    }
    #[test]
    fn no_bar_evidence_does_not_create_pending_work() {
        let pixels = vec![255; 120 * 120];
        let image = ImageView::new(&pixels, 120, 120, 1, 120).unwrap();
        let mut c = candidate([[20., 20.], [80., 20.], [80., 60.], [20., 60.]]);
        c.work.max_run_count = 0;
        let mut budget = 0;
        recover(image, &mut c, &mut budget);
        assert_eq!(c.work.retry_paths_pending, 0);
        assert!(c.observations.is_empty());
    }
    #[test]
    fn geometry_outside_model_and_extreme_pitch_are_rejected() {
        let (_, q) = fixture(0., None);
        assert!(region(q, 0).is_some());
        let mut warped = q;
        warped[2][0] += 1.;
        assert!(region(warped, 0).is_none());
        let tiny = q.map(|p| [p[0] * 0.1, p[1]]);
        assert!(region(tiny, 0).is_none());
        let mut invalid = q;
        invalid[0][0] = f64::NAN;
        assert!(region(invalid, 0).is_none());
    }
}

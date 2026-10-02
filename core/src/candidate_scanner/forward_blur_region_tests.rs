use super::*;
fn physical_row(d: &[u8; 13]) -> [u8; 512] {
    let bits = ean::encode(d);
    std::array::from_fn(|i| {
        let (mut sum, mut norm) = (0., 0.);
        for n in -640..=640 {
            let dx = f64::from(n) / 32.;
            let weight = (-dx * dx / (2. * 2.6 * 2.6)).exp();
            let module =
                crate::numeric::f64_i32(((crate::numeric::usize_f64(i) + dx - 64.) / 4.).floor());
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
    let result = CandidateScanner::default()
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

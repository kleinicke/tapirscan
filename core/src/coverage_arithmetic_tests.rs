use super::*;
#[test]
fn fused_coverage_correlation_preserves_threshold_decisions() {
    for channels in [1, 3, 4] {
        let stride = 400 * channels + 7;
        let mut pixels = vec![0; stride * 16];
        for y in 0..16 {
            for x in 0..400 {
                for c in 0..channels {
                    pixels[y * stride + x * channels + c] =
                        u8::try_from(30 + ((x * 17 + c * 13) % 150) + (y * (x % 5))).unwrap();
                }
            }
        }
        let im = ImageView::new(&pixels, 400, 16, channels, stride).unwrap();
        for y in 0..15 {
            for offset in 0..16 {
                let edge = [[4., 0.], [395., 0.]];
                let mut budget = ReuseBudget {
                    pixels_remaining: 100_000,
                    ..Default::default()
                };
                let mut work = Work::default();
                let reference = extension_row(im, edge, &mut work, &mut budget).unwrap();
                let d = crate::numeric::usize_f64(offset) / 20.;
                let row_edge = [
                    [4. + d, crate::numeric::usize_f64(y)],
                    [395. + d, crate::numeric::usize_f64(y)],
                ];
                let row = extension_row(im, row_edge, &mut work, &mut budget).unwrap();
                let expected: f64 = reference.iter().zip(row).map(|(a, b)| a * b).sum();
                let observed =
                    extension_correlation(im, &reference, row_edge, &mut work, &mut budget)
                        .unwrap();
                assert!((expected - observed).abs() < 1e-12);
                assert_eq!(expected < 0.98, observed < 0.98);
            }
        }
    }
}

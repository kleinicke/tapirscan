use super::*;
#[test]
fn explicit_window_matches_native_and_interior_for_both_axes() {
    let pixels: Vec<u8> = (0usize..128 * 96)
        .map(|i| ((i * 37 + i / 128 * 13) % 256).to_le_bytes()[0])
        .collect();
    let im = ImageView::new(&pixels, 128, 96, 1, 128).unwrap();
    let q = [[15., 12.], [110., 17.], [105., 80.], [20., 85.]];
    let mut e = CandidateScanner::default();
    for (axis, f, lo, hi, n) in [
        (2, 0.5, 0., 1., 64),
        (0, f64::NAN, 0., 1., 64),
        (0, 0.5, 1., 0., 64),
        (0, 0.5, 0., 1., 63),
        (0, 0.5, 0., 1., 4097),
        (0, 0.5, f64::NAN, 1., 64),
    ] {
        assert!(e
            .diagnostic_segment(im, q, axis, f, lo, hi, n, false)
            .is_err());
    }
}

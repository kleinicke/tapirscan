use super::*;
#[test]
fn both_profile_exports_reject_invalid_paths() {
    let pixels = vec![255; 64 * 64];
    let im = ImageView::new(&pixels, 64, 64, 1, 64).unwrap();
    let q = [[0., 0.], [64., 0.], [64., 64.], [0., 64.]];
    let mut e = CandidateScanner::default();
    for native in [false, true] {
        for (axis, f) in [(2, 0.5), (0, f64::NAN), (0, -0.01), (1, 1.01)] {
            assert!(e.diagnostic_profile(im, q, axis, f, native).is_err());
            assert!(
                e.diagnostic_interior_profile(im, q, axis, f, native)
                    .is_err()
            );
        }
    }
}

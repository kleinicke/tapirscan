use super::*;
#[test]
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
)]
fn thin_black_pixel_is_retained_by_nearest_sample() {
    let im = ImageView::new(&[255u8, 0, 255], 3, 1, 1, 3).unwrap();
    let bilinear = im.bilinear(1.4, 0.);
    let nearest = crate::numeric::f64_f32(im.gray(1.4f64.round(), 0f64.round()));
    assert_eq!(bilinear, 102.);
    assert_eq!(nearest, 0.);
    assert!(nearest < bilinear);
}

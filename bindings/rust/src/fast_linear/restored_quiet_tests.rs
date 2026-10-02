use super::*;

#[test]
fn restored_upce_requires_source_quiet_space_or_actual_image_edge() {
    let pixels = vec![255; 100 * 60];
    let image = ImageView::new(&pixels, 100, 60, 1, 100).unwrap();
    let read = linear::Read {
        decoded: true,
        addon: None,
        format: "UPCE",
        text: "01234565".into(),
        start: 1,
        end: 8,
        error: 0.,
        gs1: false,
    };
    let mut runs = vec![0.5, 1., 1., 1., 1., 3., 1., 1., 0.5];
    let interior = [[20., 30.], [80., 30.]];
    assert!(!recovered_upce_quiet(&runs, &read, image, interior));
    assert!(recovered_upce_quiet(
        &runs,
        &read,
        image,
        [[0., 30.], [99., 30.]]
    ));
    assert!(!recovered_upce_quiet(
        &runs,
        &read,
        image,
        [[0., 30.], [80., 30.]]
    ));
    runs[0] = 7.;
    runs[8] = 7.;
    assert!(recovered_upce_quiet(&runs, &read, image, interior));
    runs[8] = 6.;
    assert!(!recovered_upce_quiet(&runs, &read, image, interior));
}

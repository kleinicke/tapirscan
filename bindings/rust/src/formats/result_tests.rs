use super::*;
#[test]
fn explicit_unread_regions_survive_without_diagnostics() {
    let q = [[0., 0.], [2., 0.], [2., 1.], [0., 1.]];
    let read = Read::primary([0; 13], q, 4, 0, vec![0]);
    let unread = Region::unknown(q.map(|[x, y]| [x + 3., y]));
    let pixels = [255_u8; 64];
    let image = Image {
        data: &pixels,
        width: 8,
        height: 8,
        channels: 1,
        stride: 8,
    };
    let result = typed_result(image, vec![read], vec![unread], false, None).unwrap();
    assert_eq!(result.barcodes.len(), 1);
    assert_eq!(result.undecoded.len(), 1);
    assert!((result.undecoded[0].polygon[0][0] - 3.).abs() < f64::EPSILON);
    assert!(result.undecoded[0].format.is_none());
    assert!(result.diagnostics.is_none());
}

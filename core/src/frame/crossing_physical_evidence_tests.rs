use super::*;
#[test]
fn crossed_equal_claims_without_source_evidence_stay_separate() {
    let a = [[20., 50.], [400., 50.], [400., 52.], [20., 52.]];
    let b = [[20., 30.], [400., 70.], [400., 72.], [20., 32.]];
    let make = |index, polygon| Candidate {
        index,
        coverage: polygon,
        observations: vec![],
        detections: vec![Detection {
            digits: [0; 13],
            polygon,
            support: 2,
            axis: 0,
        }],
        work: Work::default(),
        ms: 0.,
        error: false,
    };
    let data = vec![255; 512 * 100];
    let im = ImageView::new(&data, 512, 100, 1, 512).unwrap();
    for enabled in [false, true] {
        let f = reconcile_image(
            Some(im),
            vec![make(0, a), make(1, b)],
            Policy {
                source_identity: enabled,
                ..Default::default()
            },
        );
        assert_eq!(f.barcodes.len(), 2);
    }
}
#[test]
fn endpoint_crossing_geometry_still_requires_source_identity() {
    let a = [[20., 50.], [400., 50.], [400., 52.], [20., 52.]];
    let b = [[20., 48.], [400., 68.], [400., 70.], [20., 50.]];
    assert!(crossing_read_paths(a, b));
    let make = |index, polygon| Candidate {
        index,
        coverage: polygon,
        observations: vec![],
        detections: vec![Detection {
            digits: [0; 13],
            polygon,
            support: 2,
            axis: 0,
        }],
        work: Work::default(),
        ms: 0.,
        error: false,
    };
    let data = vec![255; 512 * 100];
    let im = ImageView::new(&data, 512, 100, 1, 512).unwrap();
    let f = reconcile_image(
        Some(im),
        vec![make(0, a), make(1, b)],
        Policy {
            source_identity: true,
            ..Default::default()
        },
    );
    assert_eq!(f.barcodes.len(), 2);
}

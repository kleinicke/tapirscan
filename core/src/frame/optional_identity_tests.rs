use super::*;
const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
fn q(y: f64) -> Quad {
    [[20., y], [400., y], [400., y + 5.], [20., y + 5.]]
}
fn c(index: usize, y: f64, digits: [u8; 13]) -> Candidate {
    Candidate {
        index,
        coverage: [[0., 0.], [419., 0.], [419., 159.], [0., 159.]],
        observations: vec![],
        detections: vec![Detection {
            digits,
            polygon: q(y),
            support: 2,
            axis: 0,
        }],
        work: Work::default(),
        ms: 0.,
        error: false,
    }
}
fn pixels(gap: bool) -> Vec<u8> {
    let mut p = vec![255; 420 * 160];
    let bits = crate::ean::encode(&A);
    for y in 0..160 {
        if gap && y == 60 {
            continue;
        }
        for x in 0..380 {
            if bits[x / 4] > 0.5 {
                p[y * 420 + 20 + x] = 0;
            }
        }
    }
    p
}
#[test]
fn optional_pixel_exhaustion_retains_bands_and_charges_shared_pixels() {
    let p = pixels(false);
    let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
    for limit in [0, 95, 199, 4096] {
        let f = reconcile_image(
            Some(im),
            vec![c(0, 20., A), c(1, 100., A)],
            Policy {
                source_identity: true,
                max_association_pixels: limit,
                ..Policy::default()
            },
        );
        assert_eq!(f.barcodes.len(), 2);
        assert!(f.unfinished);
        assert!(!f.reconciliation.truncated);
        assert!(f.reconciliation.optional_identity_deferred > 0);
        assert!(f.reconciliation.source_pixels <= limit);
        assert_eq!(f.reconciliation.pending_observations, 0);
        assert_eq!(f.reconciliation.comparisons, 1);
        if limit == 199 {
            assert_eq!(f.reconciliation.source_pixels, 192);
        }
        assert!(crate::region_json::frame_json(&f).contains("\"optional_identity_deferred\":1"));
    }
}
#[test]
fn mandatory_checks_still_quarantine_and_later_conflicts_are_examined() {
    let p = pixels(false);
    let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
    let f = reconcile_image(
        Some(im),
        vec![c(0, 20., A), c(1, 100., A)],
        Policy {
            source_identity: true,
            max_association_checks: 0,
            max_association_pixels: 0,
            ..Policy::default()
        },
    );
    assert!(f.barcodes.is_empty());
    assert!(f.reconciliation.truncated);
    assert_eq!(f.reconciliation.optional_identity_deferred, 0);
    let f = reconcile_image(
        Some(im),
        vec![c(0, 20., A), c(1, 100., A), c(2, 20., B)],
        Policy {
            source_identity: true,
            max_association_pixels: 0,
            ..Policy::default()
        },
    );
    assert!(!f.reconciliation.truncated);
    assert!(f.reconciliation.conflicting > 0);
    assert!(f.reconciliation.optional_identity_deferred > 0);
    assert_eq!(f.barcodes.len(), 1);
    assert_eq!(f.barcodes[0].candidate_indices, vec![1]);
    let f = reconcile_image(
        Some(im),
        vec![c(0, 20., A), c(1, 20., B)],
        Policy {
            source_identity: true,
            max_association_pixels: 0,
            ..Policy::default()
        },
    );
    assert!(f.barcodes.is_empty());
    assert!(f.reconciliation.conflicting > 0);
    assert_eq!(f.reconciliation.optional_identity_deferred, 0);
}
#[test]
fn completed_continuity_merges_but_one_pixel_white_gap_keeps_equal_symbols() {
    for gap in [false, true] {
        let p = pixels(gap);
        let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
        let f = reconcile_image(
            Some(im),
            vec![c(0, 20., A), c(1, 100., A)],
            Policy {
                source_identity: true,
                ..Policy::default()
            },
        );
        assert_eq!(f.barcodes.len(), if gap { 2 } else { 1 });
        assert_eq!(f.reconciliation.optional_identity_deferred, 0);
        assert!(!f.reconciliation.truncated);
    }
}
#[test]
fn phase_pixel_shortfall_is_deferred_not_global_truncation() {
    let mut p = vec![255; 420 * 160];
    for y in 0..160 {
        for x in 0..420 {
            p[y * 420 + x] = if (x / 2) % 2 == 0 { 0 } else { 255 };
        }
    }
    let im = ImageView::new(&p, 420, 160, 1, 420).unwrap();
    let mut budget = AssociationBudget {
        pixels_left: 1184,
        checks_left: 10,
    };
    let mut w = Work::default();
    let mut r = ReconciliationWork::default();
    {
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        {
            assert!(!same_text_identity(
                im,
                [[20., 20.], [400., 20.]],
                [[22., 100.], [402., 100.]],
                &mut budget,
                &mut w,
                &mut r
            ));
        }
        #[cfg(feature = "mode-very-high")]
        {
            assert!(!same_text_identity(
                im,
                [[20., 20.], [400., 20.]],
                [[22., 100.], [402., 100.]],
                &mut budget,
                &mut w,
                &mut r,
                &mut crate::identity::IdentityCache::default()
            ));
        }
    }

    assert_eq!(w.continuity_samples, 192);
    assert_eq!(budget.pixels_left, 992);
    assert_eq!(w.continuity_capped_links, 0);
    assert_eq!(r.optional_identity_deferred, 1);
    assert_eq!(w.association_truncated, 0);
    assert_eq!(budget.checks_left, 10);
}

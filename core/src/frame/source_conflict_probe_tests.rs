use super::*;
fn q(y: f64, h: f64) -> Quad {
    [[20., y], [400., y], [400., y + h], [20., y + h]]
}
#[test]
fn source_pixels_resolve_alias_but_not_real_competing_code() {
    let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let other = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    let bits = crate::ean::encode(&good);
    let mut pixels = vec![255; 440 * 120];
    for y in 0..120 {
        for x in 0..380 {
            pixels[y * 440 + 20 + x] = if bits[x / 4] > 0.5 { 0 } else { 255 };
        }
    }
    let broad = Detection {
        digits: good,
        polygon: q(10., 100.),
        support: 20,
        axis: 0,
    };
    let thin = Detection {
        digits: other,
        polygon: q(58., 3.),
        support: 2,
        axis: 0,
    };
    let mut budget = AssociationBudget::default();
    let mut counter = Work::default();
    let before = budget.pixels_left;
    assert!(source_conflict_winner(
        ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
        &broad,
        &thin,
        &mut budget,
        &mut counter
    ));
    assert_eq!(before - budget.pixels_left, counter.continuity_samples);
    assert!(counter.continuity_samples > 0);
    let mut empty = AssociationBudget {
        checks_left: 0,
        pixels_left: 0,
    };
    assert!(!source_conflict_winner(
        ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
        &broad,
        &thin,
        &mut empty,
        &mut Work::default()
    ));
    let bits = crate::ean::encode(&other);
    for y in 55..65 {
        for x in 0..380 {
            pixels[y * 440 + 20 + x] = if bits[x / 4] > 0.5 { 0 } else { 255 };
        }
    }
    assert!(!source_conflict_winner(
        ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
        &broad,
        &thin,
        &mut AssociationBudget::default(),
        &mut Work::default()
    ));
    pixels.fill(128);
    assert!(!source_conflict_winner(
        ImageView::new(&pixels, 440, 120, 1, 440).unwrap(),
        &broad,
        &thin,
        &mut AssociationBudget::default(),
        &mut Work::default()
    ));
}

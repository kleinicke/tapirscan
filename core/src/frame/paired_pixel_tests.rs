use super::*;
fn band(y: f64, digits: [u8; 13]) -> Detection {
    Detection {
        digits,
        polygon: [[20., y], [305., y], [305., y + 15.], [20., y + 15.]],
        support: 4,
        axis: 0,
    }
}
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples and unchanged geometry; approximate equality would hide a behavior change."
)]
fn render(top: [u8; 13], bottom: [u8; 13]) -> Vec<u8> {
    let mut p = vec![255u8; 330 * 100];
    for y in 0..100 {
        let bits = crate::ean::encode(&if y < 50 { top } else { bottom });
        for x in 20..305 {
            p[y * 330 + x] = if bits[(x - 20) / 3] == 1. { 0 } else { 255 };
        }
    }
    p
}
#[test]
fn actual_contrary_code_is_not_outvoted_by_a_neighbor() {
    let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    let p = render(a, b);
    let im = ImageView::new(&p, 330, 100, 1, 330).unwrap();
    assert!(!source_pair_winner(
        im,
        &band(20., a),
        &band(65., b),
        &mut AssociationBudget::default(),
        &mut Work::default()
    ));
    assert!(!source_pair_winner(
        im,
        &band(65., b),
        &band(20., a),
        &mut AssociationBudget::default(),
        &mut Work::default()
    ));
}
#[test]
fn false_alias_can_only_be_rejected_when_both_bands_support_the_winner() {
    let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    let p = render(a, a);
    let im = ImageView::new(&p, 330, 100, 1, 330).unwrap();
    assert!(source_pair_winner(
        im,
        &band(20., a),
        &band(65., b),
        &mut AssociationBudget::default(),
        &mut Work::default()
    ));
    assert!(!source_pair_winner(
        im,
        &band(65., b),
        &band(20., a),
        &mut AssociationBudget::default(),
        &mut Work::default()
    ));
    let mut budget = AssociationBudget {
        pixels_left: 0,
        checks_left: 0,
    };
    let mut work = Work::default();
    assert!(!source_pair_winner(
        im,
        &band(20., a),
        &band(65., b),
        &mut budget,
        &mut work
    ));
    assert_eq!(work.continuity_samples, 0);
}

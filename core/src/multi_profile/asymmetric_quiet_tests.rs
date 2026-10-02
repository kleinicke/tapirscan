use super::*;
fn runs(d: [u8; 13], left: usize, right: usize, reverse: bool) -> Vec<(usize, usize, bool)> {
    let mut p = vec![0.; left * 4];
    for bit in crate::ean::encode(&d) {
        p.extend([bit; 4]);
    }
    p.extend(vec![0.; right * 4]);
    if reverse {
        p.reverse();
    }
    let mut r = vec![];
    sample_runs(&p, 64, &mut r).unwrap();
    r
}
#[test]
fn asymmetric_quiet_follows_reading_direction_and_keeps_checksum() {
    let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    for reverse in [false, true] {
        assert_eq!(
            decode_short_quiet(&runs(d, 6, 3, reverse), 64, true).symbols[0].digits,
            d
        );
        assert!(decode_short_quiet(&runs(d, 3, 6, reverse), 64, true)
            .symbols
            .is_empty());
        assert!(decode_short_quiet(&runs(d, 6, 2, reverse), 64, true)
            .symbols
            .is_empty());
        let mut bad = d;
        bad[12] = 8;
        assert!(decode_short_quiet(&runs(bad, 6, 3, reverse), 64, true)
            .symbols
            .is_empty());
    }
}

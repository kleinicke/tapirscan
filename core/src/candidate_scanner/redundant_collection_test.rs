use super::*;
#[test]
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
)]
fn collect_exercises_reuse_without_altering_input_or_observation_shape() {
    let mut ex = CandidateScanner::default();
    let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let mut signal = vec![0.; 40];
    for bit in crate::ean::encode(&digits) {
        signal.extend([bit; 4]);
    }
    signal.extend([0.; 40]);
    ex.signal = signal.clone();
    let mut work = Work::default();
    let mut observations = vec![];
    ex.collect_policy(
        0,
        0.35,
        -0.15,
        1.15,
        &mut work,
        &mut observations,
        true,
        true,
    );
    assert!(work.redundant_decode_calls_avoided >= 2);
    assert!(work.accepted_paths > 0);
    assert_eq!(ex.signal, signal);
    assert!(observations
        .iter()
        .any(|o| !o.ambiguous && o.digits == digits));
    assert!(observations
        .iter()
        .all(|o| o.axis == 0 && o.fraction == 0.35));
}

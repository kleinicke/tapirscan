use super::*;
#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
fn shallow(d: [u8; 13]) -> Vec<f32> {
    let bits = crate::ean::encode(&d);
    let mut p = vec![0.; 36];
    let mut at = 0;
    let mut changed = 0;
    while at < bits.len() {
        let mut end = at + 1;
        while end < bits.len() && bits[end] == bits[at] {
            end += 1;
        }
        let mut value = bits[at];
        if at > 3 && at < 45 && value > 0.5 && end - at == 1 && changed < 2 {
            value = 0.4;
            changed += 1;
        }
        for _ in at..end {
            p.extend([value; 3]);
        }
        at = end;
    }
    assert_eq!(changed, 2);
    p.extend([0.; 36]);
    p
}
#[test]
fn threshold_reads_existing_shallow_elements_without_checksum_repair() {
    let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let mut bad = good;
    bad[12] = 8;
    for d in [good, bad] {
        for reverse in [false, true] {
            let mut p = shallow(d);
            if reverse {
                p.reverse();
            }
            let original = p.clone();
            let mut ex = CandidateScanner {
                signal: p,
                ..CandidateScanner::default()
            };
            let mut obs = vec![];
            ex.collect_policy(0, 0.5, 0., 1., &mut Work::default(), &mut obs, true, true);
            assert_eq!(ex.signal, original);
            assert_eq!(obs.iter().any(|o| !o.ambiguous && o.digits == d), d == good);
        }
    }
}

use super::*;
const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
const B: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
fn encoded(d: &[u8; 13]) -> Vec<f32> {
    let mut p = vec![0.; 144];
    for v in crate::ean::encode(d) {
        p.extend(std::iter::repeat_n(v, 12));
    }
    p.extend([0.; 144]);
    p
}
fn excursion(p: &mut [f32]) {
    p[149..151].fill(0.45);
}
fn active(p: &[f32]) -> Vec<(usize, usize, bool)> {
    let mut runs = vec![];
    sample_runs(p, 64, &mut runs).unwrap();
    let cleaned = weak_excursions(p, &runs).expect("new branch must activate");
    assert!(cleaned.len() < runs.len());
    cleaned
}
fn final_reads(p: &[f32]) -> Reads {
    decode_local_variants(p, 64, true, &mut LocalRuns::default()).unwrap()
}
#[test]
fn weak_activated_ean_negatives_and_shallow_elements() {
    let mut good = encoded(&A);
    excursion(&mut good);
    let cleaned = active(&good);
    assert!(decode_many(&good, 64).unwrap().symbols.is_empty());
    assert_eq!(decode_positions(&cleaned, 64, false).symbols[0].digits, A);
    assert_eq!(final_reads(&good).symbols[0].digits, A);
    for check in 0..10 {
        if check == A[12] {
            continue;
        }
        let mut bad = A;
        bad[12] = check;
        let mut p = encoded(&bad);
        excursion(&mut p);
        active(&p);
        assert!(final_reads(&p).symbols.is_empty(), "checksum {check}");
        p.reverse();
        active(&p);
        assert!(final_reads(&p).symbols.is_empty());
    }
    // Genuine complete one-module guard bar stays present even when shallow;
    // a completely absent one is never invented. A separate weak pulse activates.
    let mut shallow = good.clone();
    shallow[168..180].fill(0.55);
    active(&shallow);
    assert_eq!(final_reads(&shallow).symbols[0].digits, A);
    let mut missing = good.clone();
    missing[168..180].fill(0.);
    active(&missing);
    assert!(final_reads(&missing).symbols.is_empty());
}
#[test]
fn weak_activated_reads_preserve_conflicts_and_separate_instances() {
    assert!(crate::ean::checksum(&B));
    let mut a = encoded(&A);
    excursion(&mut a);
    active(&a);
    let ra = final_reads(&a);
    assert_eq!(ra.symbols.len(), 1);
    let mut b = encoded(&B);
    excursion(&mut b);
    active(&b);
    let rb = final_reads(&b);
    assert_eq!(rb.symbols.len(), 1);
    let conflict = crate::transition::merge_reads(ra.clone(), rb.clone(), 64);
    assert!(conflict.symbols.is_empty());
    assert!(!conflict.rejected_intervals.is_empty());
    let mut veto = rb;
    veto.rejected_intervals
        .push((ra.symbols[0].left, ra.symbols[0].right));
    veto.symbols.clear();
    assert!(crate::transition::merge_reads(veto.clone(), ra.clone(), 64)
        .symbols
        .is_empty());
    assert!(crate::transition::merge_reads(ra, veto, 64)
        .symbols
        .is_empty());
    for right in [a.clone(), b] {
        let mut both = a.clone();
        both.extend(right);
        active(&both);
        let rs = final_reads(&both);
        assert_eq!(rs.symbols.len(), 2);
        assert_eq!(rs.symbols[0].digits, A);
        assert!(rs.symbols[0].right < rs.symbols[1].left);
    }
}
#[test]
fn weak_submodule_only() {
    let base: Vec<f32> = (0..30)
        .flat_map(|i| std::iter::repeat_n(crate::numeric::f64_f32(f64::from(i % 2)), 12))
        .collect();
    let runs = |p: &[f32]| {
        let mut r = vec![];
        sample_runs(p, 64, &mut r).unwrap();
        r
    };
    let r = runs(&base);
    assert_eq!(weak_excursions(&base, &r), None);
    let mut p = base.clone();
    p[54] = 0.55;
    p[55] = 0.55;
    let noisy = runs(&p);
    assert_eq!(weak_excursions(&p, &noisy), Some(r));
    p[54] = 1.;
    p[55] = 1.;
    let strong = runs(&p);
    assert_eq!(weak_excursions(&p, &strong), None);
    let mut p = base.clone();
    p[60..72].fill(0.55);
    let shallow = runs(&p);
    assert_eq!(weak_excursions(&p, &shallow), None);
    let mut p = base.clone();
    p[60..72].fill(0.);
    let missing = runs(&p);
    assert_eq!(weak_excursions(&p, &missing), None);
}

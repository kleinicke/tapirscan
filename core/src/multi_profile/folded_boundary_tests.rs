use super::*;
const A: [u8; 13] = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
fn profile(d: [u8; 13], l: usize, r: usize) -> Vec<f32> {
    let mut p = vec![1.; 16];
    p.extend(vec![0.; l * 4]);
    for v in crate::ean::encode(&d) {
        p.extend([v; 4]);
    }
    p.extend(vec![0.; r * 4]);
    p.extend([1.; 16]);
    p
}
fn read(p: &[f32]) -> Reads {
    let mut runs = vec![];
    sample_runs(p, 64, &mut runs).unwrap();
    decode_short_quiet(&runs, 64, false)
}
#[test]
fn asymmetric_exact_checksum_and_direction() {
    for reverse in [false, true] {
        for invalid in [false, true] {
            let mut d = A;
            if invalid {
                d[12] = 8;
            }
            let mut p = profile(d, 10, 2);
            if reverse {
                p.reverse();
            }
            let r = read(&p);
            assert_eq!(r.symbols.len(), usize::from(!invalid));
            if !r.symbols.is_empty() {
                assert_eq!(r.symbols[0].digits, d);
            }
        }
    }
    for (l, r) in [(8, 2), (10, 0), (2, 10), (2, 2)] {
        assert!(read(&profile(A, l, r)).symbols.is_empty(), "quiet {l}/{r}");
    }
}
#[test]
fn asymmetric_missing_guard_and_window_edge_are_not_reconstructed() {
    for offset in [0, 2, 92, 94] {
        let mut p = profile(A, 10, 2);
        let start = 16 + 40 + offset * 4;
        p[start..start + 4].fill(0.);
        assert!(read(&p).symbols.is_empty());
    }
    let mut clipped = profile(A, 10, 2);
    clipped.truncate(clipped.len() - 16);
    assert!(
        read(&clipped).symbols.is_empty(),
        "sample-window boundary is not observed exterior ink"
    );
    for check in 0..10 {
        if check == A[12] {
            continue;
        }
        let mut invalid = A;
        invalid[12] = check;
        assert!(read(&profile(invalid, 10, 2)).symbols.is_empty());
    }
}
#[test]
fn asymmetric_noise_never_becomes_barcode() {
    let mut seed = 19u32;
    for _ in 0..128 {
        let mut p = vec![1.; 16];
        p.extend([0.; 40]);
        for i in 0..59 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = 1 + (seed >> 24) as usize % 15;
            p.extend(std::iter::repeat_n(if i % 2 == 0 { 1. } else { 0. }, n));
        }
        p.extend([0.; 8]);
        p.extend([1.; 16]);
        assert!(read(&p).symbols.is_empty());
    }
}
#[test]
fn folded_source_profiles_require_experimental_boundary_model() {
    let mut successes = 0;
    for line in include_str!("../../fixtures/folded_profiles.txt").lines() {
        let p: Vec<f32> = line
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        let normalized = crate::local_signal::normalize(&p).unwrap();
        let r = read(&normalized);
        for s in r.symbols {
            successes += 1;
            assert_eq!(s.digits, [8, 0, 0, 2, 3, 3, 0, 1, 1, 2, 7, 5, 2]);
        }
    }
    {
        assert!(successes >= 1, "native saved profiles must recover");
    }
}

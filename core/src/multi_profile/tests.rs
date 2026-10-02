use super::*;

#[test]
fn fused_local_runs_preserve_all_evidence_and_scratch_ownership() {
    let a = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let b = digits([4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3]);
    let clean = signal(&[a, b, a]);
    let mut reverse = clean.clone();
    reverse.reverse();
    let soft: Vec<_> = (0..clean.len())
        .map(|i| {
            0.15 + 0.7
                * (0.25 * clean[i.saturating_sub(1)]
                    + 0.5 * clean[i]
                    + 0.25 * clean[(i + 1).min(clean.len() - 1)])
        })
        .collect();
    let noise: Vec<_> = (0..4096)
        .map(|i| crate::numeric::f64_f32(f64::from((i * 73 + 19) % 101)) / 100.)
        .collect();
    let mut scratch = LocalRuns::default();
    for p in [clean, reverse, noise, vec![0.; 64], soft, vec![1.; 4096]] {
        for max in [1, 64] {
            for guard in [false, true] {
                let expected = crate::transition::merge_reads(
                    decode_fractional(&p, max).unwrap(),
                    decode_many(&p, max).unwrap(),
                    max,
                );
                let expected = if guard {
                    crate::transition::merge_reads(
                        expected,
                        decode_guard_bias(&p, max).unwrap(),
                        max,
                    )
                } else {
                    expected
                };

                let expected = if guard {
                    crate::transition::merge_reads(
                        expected,
                        decode_fractional_guard_bias(&p, max).unwrap(),
                        max,
                    )
                } else {
                    expected
                };

                let actual = decode_local_variants(&p, max, guard, &mut scratch).unwrap();
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
            }
        }
    }
    assert!(decode_local_variants(&[f32::NAN; 64], 64, true, &mut scratch).is_err());
    assert!(decode_local_variants(&[0.; 64], 0, true, &mut scratch).is_err());
}
#[test]
fn fractional_guard_observes_blurred_bars_and_rejects_bad_checksum() {
    let good = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let mut successes = 0;
    for invalid in [false, true] {
        let mut d = good;
        if invalid {
            d[12] = (d[12] + 1) % 10;
        }
        for pitch in [2.2f64, 3.3, 5.7] {
            for bias in [-0.3, 0.3] {
                for phase in [0.1, 0.4, 0.8] {
                    let bits = crate::ean::encode(&d);
                    let count = crate::numeric::f64_usize((119. * pitch).ceil());
                    let mut profile = vec![0.; count];
                    // Pixel-area integration of independently positioned bar intervals.
                    let mut i = 0;
                    while i < 95 {
                        if bits[i] < 0.5 {
                            i += 1;
                            continue;
                        }
                        let mut j = i + 1;
                        while j < 95 && bits[j] >= 0.5 {
                            j += 1;
                        }
                        let lo = (12. + crate::numeric::usize_f64(i)) * pitch - bias * pitch / 2.
                            + phase;
                        let hi = (12. + crate::numeric::usize_f64(j)) * pitch
                            + bias * pitch / 2.
                            + phase;
                        for (x, v) in profile.iter_mut().enumerate() {
                            *v += crate::numeric::f64_f32(
                                (hi.min(crate::numeric::usize_f64(x) + 1.)
                                    - lo.max(crate::numeric::usize_f64(x)))
                                .max(0.),
                            );
                        }
                        i = j;
                    }
                    let r = decode_fractional_guard_bias(&profile, 64).unwrap();
                    if invalid {
                        assert!(r.symbols.is_empty());
                    } else {
                        for height in r.symbols {
                            assert_eq!(height.digits, good);
                            successes += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(successes >= 12, "{successes}");
}
#[test]
fn relative_edges_preserve_clean_instances_reverse_and_negatives() {
    let a = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let b = digits([4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3]);
    for codes in [vec![a], vec![a, a], vec![a, b, a]] {
        let p = signal(&codes);
        let r = decode_relative_edges(&p, 64, true).unwrap();
        assert_eq!(
            r.symbols.iter().map(|s| s.digits).collect::<Vec<_>>(),
            codes
        );
        let mut rev = p;
        rev.reverse();
        let r = decode_relative_edges(&rev, 64, true).unwrap();
        assert_eq!(
            r.symbols.iter().map(|s| s.digits).collect::<Vec<_>>(),
            codes.into_iter().rev().collect::<Vec<_>>()
        );
    }
    let mut bad = a;
    bad[12] = (bad[12] + 1) % 10;
    assert!(decode_relative_edges(&signal(&[bad]), 64, true)
        .unwrap()
        .symbols
        .is_empty());
    for p in [
        vec![0.; 512],
        vec![1.; 512],
        (0..512)
            .map(|i| crate::numeric::f64_f32(f64::from(i % 2)))
            .collect(),
        (0..512)
            .map(|i| crate::numeric::f64_f32(f64::from(i)) / 511.)
            .collect(),
    ] {
        assert!(decode_relative_edges(&p, 64, true)
            .unwrap()
            .symbols
            .is_empty());
    }
    assert!(decode_relative_edges(&[f32::NAN; 512], 64, true).is_err());
}

#[test]
fn short_quiet_is_observed_and_does_not_repair_symbols() {
    for (first, reversed) in (0..10).flat_map(|n| [false, true].map(move |r| (n, r))) {
        let d = digits([first, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
        let build = |d: &[u8; 13], quiet: usize, missing: bool| {
            let mut p = vec![0.; quiet];
            let mut bits = crate::ean::encode(d);
            if missing {
                bits[0] = 0.;
            }
            for b in bits {
                p.extend([b; 4]);
            }
            p.extend(vec![0.; quiet]);
            if reversed {
                p.reverse();
            }
            let mut runs = vec![];
            sample_runs(&p, 64, &mut runs).unwrap();
            runs
        };
        let r = build(&d, 20, false);
        assert!(decode_runs(&r, 64).symbols.is_empty());
        assert_eq!(decode_short_quiet(&r, 64, true).symbols[0].digits, d);
        let mut bad = d;
        bad[12] = (bad[12] + 1) % 10;
        assert!(decode_short_quiet(&build(&bad, 20, false), 64, true)
            .symbols
            .is_empty());
        assert!(decode_short_quiet(&build(&d, 12, false), 64, true)
            .symbols
            .is_empty());
        assert!(decode_short_quiet(&build(&d, 20, true), 64, true)
            .symbols
            .is_empty());
    }
}
fn digits(prefix: [u8; 12]) -> [u8; 13] {
    let mut d = [0; 13];
    d[..12].copy_from_slice(&prefix);
    let sum: u32 = prefix
        .iter()
        .enumerate()
        .map(|(i, &n)| u32::from(n) * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    d[12] = ((10 - sum % 10) % 10) as u8;
    d
}
fn signal(codes: &[[u8; 13]]) -> Vec<f32> {
    let mut p = vec![0.; 40];
    for d in codes {
        for bit in crate::ean::encode(d) {
            p.extend([bit; 3]);
        }
        p.extend([0.; 40]);
    }
    p
}
#[test]
fn multiple_values_and_equal_instances_survive_both_directions() {
    let a = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let b = digits([4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3]);
    for codes in [vec![a, b], vec![a, a], vec![a, b, a]] {
        let mut p = signal(&codes);
        for reverse in [false, true] {
            if reverse {
                p.reverse();
            }
            let r = decode_many(&p, 64).unwrap();
            let expected: Vec<_> = if reverse {
                codes.iter().rev().copied().collect()
            } else {
                codes.clone()
            };
            assert_eq!(
                r.symbols.iter().map(|r| r.digits).collect::<Vec<_>>(),
                expected
            );
            assert!(!r.truncated);
            assert!(r.symbols.windows(2).all(|r| r[0].right < r[1].left));
        }
    }
}
#[test]
fn cap_is_explicit_and_does_not_hide_search() {
    let a = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let p = signal(&[a, a]);
    let limited = decode_many(&p, 1).unwrap();
    let all = decode_many(&p, 64).unwrap();
    assert!(limited.truncated);
    assert_eq!(limited.symbols.len(), 1);
    assert_eq!(limited.windows_examined, all.windows_examined);
}
#[test]
fn rejects_bad_buffers_and_negative_profiles() {
    assert!(decode_many(&[], 64).is_err());
    assert!(decode_many(&vec![0.; 4097], 64).is_err());
    assert!(decode_many(&[0.; 512], 0).is_err());
    assert!(decode_many(&[f32::NAN; 512], 64).is_err());
    for n in [64, 512, 4096] {
        assert!(decode_many(&vec![0.; n], 64).unwrap().symbols.is_empty());
        assert!(decode_many(&vec![1.; n], 64).unwrap().symbols.is_empty());
    }
    let noise: Vec<_> = (0..4096)
        .map(|i| crate::numeric::u64_f32((i * 1_103_515_245_u64 + 12345) % 65536) / 65535.)
        .collect();
    assert!(decode_many(&noise, 64).unwrap().symbols.is_empty());
}
#[test]
fn fractional_preserves_binary_geometry_caps_and_input_gates() {
    let a = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let b = digits([4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3]);
    for codes in [vec![a], vec![a, a], vec![a, b]] {
        let mut p = signal(&codes);
        for _ in 0..2 {
            p.reverse();
            let raw = decode_many(&p, 64).unwrap();
            let f = decode_fractional(&p, 64).unwrap();
            assert_eq!(raw.symbols.len(), f.symbols.len());
            for (a, b) in raw.symbols.iter().zip(&f.symbols) {
                assert_eq!(
                    (a.digits, a.left, a.right, a.cost),
                    (b.digits, b.left, b.right, b.cost)
                );
            }
            assert_eq!(raw.windows_examined, f.windows_examined);
            assert_eq!(decode_fractional(&p, 1).unwrap().truncated, codes.len() > 1);
        }
    }
    for p in [vec![], vec![0.; 4097], vec![f32::NAN; 512], vec![1.1; 512]] {
        assert!(decode_fractional(&p, 64).is_err());
    }
    assert!(decode_fractional(&[0.; 512], 0).is_err());
    assert!(decode_fractional(&[0.; 512], 65).is_err());
    for p in [
        vec![0.; 512],
        vec![1.; 512],
        (0..512)
            .map(|i| crate::numeric::f64_f32(f64::from(i % 2)))
            .collect(),
    ] {
        assert!(decode_fractional(&p, 64).unwrap().symbols.is_empty());
    }
}
#[test]
fn fractional_crossings_are_bounded_and_reflection_symmetric() {
    let a = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let p = signal(&[a]);
    let blur: Vec<f32> = (0..p.len())
        .map(|i| 0.2 * p[i.saturating_sub(1)] + 0.6 * p[i] + 0.2 * p[(i + 1).min(p.len() - 1)])
        .collect();
    let forward = decode_fractional(&blur, 64).unwrap();
    let mut reverse = blur.clone();
    reverse.reverse();
    let reverse = decode_fractional(&reverse, 64).unwrap();
    assert_eq!(forward.symbols.len(), 1);
    assert_eq!(reverse.symbols.len(), 1);
    let (f, r) = (&forward.symbols[0], &reverse.symbols[0]);
    assert_eq!(f.digits, r.digits);
    assert!((f.left + r.right - crate::numeric::usize_f64(p.len() - 1)).abs() < 1e-6);
    assert!((f.right + r.left - crate::numeric::usize_f64(p.len() - 1)).abs() < 1e-6);
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
)]
fn bias_recovery_counts_and_overlap_conflicts_remain_explicit() {
    let d = digits([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5]);
    let bits = crate::ean::encode(&d);
    let mut p = vec![0.; 240];
    let mut start = 0;
    for i in 1..=95 {
        if i == 95 || bits[i] != bits[start] {
            let n = (i - start) * 20;
            let n = if bits[start] > 0.5 { n + 7 } else { n - 7 };
            p.extend(std::iter::repeat_n(bits[start], n));
            start = i;
        }
    }
    p.extend([0.; 240]);
    assert!(decode_many(&p, 64).unwrap().symbols.is_empty());
    for _ in 0..2 {
        p.reverse();
        let r = decode_guard_bias(&p, 64).unwrap();
        assert_eq!(r.symbols.len(), 1);
        assert_eq!(r.symbols[0].digits, d);
        assert_eq!(r.bias_model_pass, 1);
        assert_eq!(r.bias_guard_pass, 1);
        let mut conflict = r.clone();
        conflict.symbols[0].digits = digits([4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3]);
        let merged = crate::transition::merge_reads(r, conflict, 64);
        assert!(merged.symbols.is_empty());
        assert!(!merged.rejected_intervals.is_empty());
        assert_eq!(merged.bias_model_pass, 2);
    }
}

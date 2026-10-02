use super::*;
fn reference_local_variants(
    p: &[f32],
    max_symbols: usize,
    guard_bias: bool,
    scratch: &mut LocalRuns,
) -> Result<Reads, Error> {
    if !(64..=4096).contains(&p.len()) || !(1..=64).contains(&max_symbols) {
        return Err(Error::Length);
    }
    if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err(Error::Value);
    }
    scratch.integer.clear();
    scratch.fractional.clear();
    let (mut start, mut edge, mut black) = (0, 0., p[0] >= 0.5);
    for i in 1..=p.len() {
        let next = i < p.len() && p[i] >= 0.5;
        if i == p.len() || next != black {
            let end = if i == p.len() {
                crate::numeric::usize_f64(i)
            } else {
                crate::numeric::usize_f64(i) - 0.5
                    + (0.5 - f64::from(p[i - 1])) / (f64::from(p[i]) - f64::from(p[i - 1]))
            };
            scratch.integer.push((start, i, black));
            scratch.fractional.push((edge, end, black));
            start = i;
            edge = end;
            black = next;
        }
    }
    let fractional = decode_positions(&scratch.fractional, max_symbols, false);
    let integer = decode_positions(&scratch.integer, max_symbols, false);
    let reads = crate::transition::merge_reads(fractional, integer, max_symbols);
    let reads = if guard_bias {
        crate::transition::merge_reads(
            reads,
            decode_positions(&scratch.integer, max_symbols, true),
            max_symbols,
        )
    } else {
        reads
    };

    let reads = if guard_bias {
        crate::transition::merge_reads(
            reads,
            decode_positions(&scratch.fractional, max_symbols, true),
            max_symbols,
        )
    } else {
        reads
    };

    // Short native-scale profiles only. Keep the historical full relative arm
    // distinct, and never evaluate the same model twice when both are enabled.

    let reads = {
        if let Some(cleaned) = weak_excursions(p, &scratch.integer) {
            let extra = decode_positions(&cleaned, max_symbols, false);
            let extra = if guard_bias {
                crate::transition::merge_reads(
                    extra,
                    decode_positions(&cleaned, max_symbols, true),
                    max_symbols,
                )
            } else {
                extra
            };
            crate::transition::merge_reads(reads, extra, max_symbols)
        } else {
            reads
        }
    };
    Ok(reads)
}

fn reference_prepared_short(runs: &LocalRuns, max_symbols: usize, guard_bias: bool) -> Reads {
    let a = decode_positions_mode(&runs.integer, max_symbols, false, true);
    let b = decode_positions_mode(&runs.fractional, max_symbols, false, true);
    let reads = crate::transition::merge_reads(a, b, max_symbols);
    if guard_bias {
        let a = decode_positions_mode(&runs.integer, max_symbols, true, true);
        let b = decode_positions_mode(&runs.fractional, max_symbols, true, true);
        crate::transition::merge_reads(
            reads,
            crate::transition::merge_reads(a, b, max_symbols),
            max_symbols,
        )
    } else {
        reads
    }
}

fn assert_reads(a: &Reads, b: &Reads) {
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    for (x, y) in a.symbols.iter().zip(&b.symbols) {
        assert_eq!(
            (
                x.left.to_bits(),
                x.right.to_bits(),
                x.cost.to_bits(),
                x.gap.to_bits()
            ),
            (
                y.left.to_bits(),
                y.right.to_bits(),
                y.cost.to_bits(),
                y.gap.to_bits()
            )
        );
    }
}
#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
fn barcode(d: [u8; 13], quiet: usize, edge_offset: isize) -> Vec<f32> {
    let bits = crate::ean::encode(&d);
    let mut p = vec![0.; quiet];
    let mut i = 0;
    while i < bits.len() {
        let mut j = i + 1;
        while j < bits.len() && bits[j] == bits[i] {
            j += 1;
        }
        let width = ((j - i) * 4).cast_signed()
            + if bits[i] > 0.5 {
                edge_offset
            } else {
                -edge_offset
            };
        p.extend(std::iter::repeat_n(bits[i], (width).cast_unsigned()));
        i = j;
    }
    p.extend(std::iter::repeat_n(0., quiet));
    p
}
#[test]
fn parity_clean_reverse_bias_noise_multi_and_options() {
    let a = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let b = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    let mut cases = vec![
        vec![0.; 64],
        vec![1.; 512],
        barcode(a, 40, 0),
        barcode(a, 16, 0),
        barcode(a, 40, 1),
        barcode(a, 40, -1),
        barcode([5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 8], 40, 0),
    ];
    let mut multi = barcode(a, 40, 0);
    multi.extend(barcode(b, 40, 0));
    multi.extend(barcode(a, 40, 0));
    cases.push(multi);
    let mut seed = 19u32;
    for n in [64, 128, 512, 4096] {
        let v = (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                crate::numeric::f64_f32(f64::from(seed >> 8)) / 16_777_215.
            })
            .collect();
        cases.push(v);
    }
    let mut unequal = barcode(a, 40, 0);
    for v in &mut unequal {
        *v = if *v > 0.5 { 0.9 } else { 0.2 }
    }
    cases.push(unequal);
    let mut actual = LocalRuns::default();
    let mut reference = LocalRuns::default();
    let mut hits = 0;
    let mut misses = 0;
    let mut truncated = false;
    for p in cases {
        for reverse in [false, true] {
            let mut p = p.clone();
            if reverse {
                p.reverse();
            }
            for max in [1, 2, 64] {
                for guard in [false, true] {
                    let want = reference_local_variants(&p, max, guard, &mut reference).unwrap();
                    let got = decode_local_variants(&p, max, guard, &mut actual).unwrap();
                    assert_reads(&got, &want);
                    truncated |= got.truncated;
                    assert_reads(
                        &decode_prepared_short(&actual, max, guard),
                        &reference_prepared_short(&reference, max, guard),
                    );
                    if actual.identical_models {
                        hits += actual.reused_calls.get();
                        assert!(actual.reused_calls.get() > 0);
                    } else {
                        misses += 1;
                        assert_eq!(actual.reused_calls.get(), 0);
                    }
                }
            }
        }
    }
    assert!(
        hits > 0 && misses > 0 && truncated,
        "exercise reuse, fallthrough, and truncation"
    );
}
#[test]
fn exact_geometry_key_rejects_near_equal_polarity_shape_nan() {
    let i = [(0, 10, false), (10, 20, true)];
    let f = [(0., 10., false), (10., 20., true)];
    assert!(exact_same_runs(&i, &f));
    let mut near = f;
    near[1].1 = f64::from_bits(20f64.to_bits() + 1);
    assert!(!exact_same_runs(&i, &near));
    let mut polarity = f;
    polarity[1].2 = false;
    assert!(!exact_same_runs(&i, &polarity));
    assert!(!exact_same_runs(&i, &f[..1]));
    let mut nan = f;
    nan[0].0 = f64::NAN;
    assert!(!exact_same_runs(&i, &nan));
    let mut signed = f;
    signed[0].0 = -0.;
    assert!(!exact_same_runs(&i, &signed));
    let mut actual = LocalRuns::default();
    let mut reference = LocalRuns::default();
    for p in [
        vec![0.; 63],
        vec![0.; 4097],
        vec![f32::NAN; 64],
        vec![f32::INFINITY; 64],
        vec![1.1; 64],
        vec![-0.1; 64],
    ] {
        assert_eq!(
            decode_local_variants(&p, 64, true, &mut actual).unwrap_err(),
            reference_local_variants(&p, 64, true, &mut reference).unwrap_err()
        );
    }
    for max in [0, 65] {
        assert!(decode_local_variants(&[0.; 64], max, true, &mut actual).is_err());
    }
}

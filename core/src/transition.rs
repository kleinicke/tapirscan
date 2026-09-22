//! Experimental scale-relative removal of isolated threshold excursions.
//! Works on run topology, preserving sample coordinates and original intensities.
#![forbid(unsafe_code)]
use crate::{
    multi_profile::{self, Reads},
    profile::Error,
};
#[derive(Debug, Default)]
pub struct CleanupWork {
    pub input_runs: usize,
    pub output_runs: usize,
    pub examined: usize,
    pub neighborhood_values: usize,
    pub removed_runs: usize,
    pub removed_pixels: usize,
    pub truncated: bool,
}
/// Single pass over at most4096 runs, each using at most33 local widths. A short
/// pulse is removable only below20% of the local lower-quartile run scale and
/// with both neighboring runs at least3times wider. The lower quartile avoids
/// treating ordinary1module elements between4module elements as noise. This is
/// a topology hypothesis, not digit/checksum repair; strict decoder gates remain.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for invalid normalized samples.
pub fn decode_cleaned(p: &[f32], max_symbols: usize) -> Result<(Reads, CleanupWork), Error> {
    if !(64..=4096).contains(&p.len()) || !(1..=64).contains(&max_symbols) {
        return Err(Error::Length);
    }
    if p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return Err(Error::Value);
    }
    let mut runs = vec![];
    let (mut start, mut dark) = (0, p[0] >= 0.5);
    for i in 1..=p.len() {
        let next = i < p.len() && p[i] >= 0.5;
        if i == p.len() || next != dark {
            runs.push((start, i, dark));
            start = i;
            dark = next;
        }
    }
    let mut work = CleanupWork {
        input_runs: runs.len(),
        ..CleanupWork::default()
    };
    let mut out = Vec::with_capacity(runs.len());
    let mut i = 0;
    while i < runs.len() {
        if i + 2 < runs.len() {
            let pulse = i + 1;
            let w = runs[pulse].1 - runs[pulse].0;
            work.examined += 1;
            if runs[i].1 - runs[i].0 >= 3 * w && runs[i + 2].1 - runs[i + 2].0 >= 3 * w {
                let lo = pulse.saturating_sub(16);
                let hi = (pulse + 17).min(runs.len());
                let mut widths: Vec<_> = runs[lo..hi].iter().map(|r| r.1 - r.0).collect();
                work.neighborhood_values += widths.len();
                widths.sort_unstable();
                let scale = widths[widths.len() / 4];
                if w * 5 < scale {
                    out.push((runs[i].0, runs[i + 2].1, runs[i].2));
                    i += 3;
                    work.removed_runs += 2;
                    work.removed_pixels += w;
                    continue;
                }
            }
        }
        out.push(runs[i]);
        i += 1;
    }
    // Adjacent output runs can share polarity after two non-overlapping repairs.
    let mut merged: Vec<(usize, usize, bool)> = Vec::with_capacity(out.len());
    for r in out {
        if let Some(last) = merged.last_mut() {
            if last.2 == r.2 {
                last.1 = r.1;
                continue;
            }
        }
        merged.push(r);
    }
    work.output_runs = merged.len();
    Ok((multi_profile::decode_runs(&merged, max_symbols), work))
}
/// Consolidate a cluster of sub-module runs near one edge. For opposite
/// flanks, preserve the original normalized dark mass when choosing the edge;
/// for equal flanks, remove only a bounded excursion. No iterative smoothing.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for invalid normalized samples.
pub fn decode_clustered(p: &[f32], max_symbols: usize) -> Result<(Reads, CleanupWork), Error> {
    let mut runs = vec![];
    multi_profile::sample_runs(p, max_symbols, &mut runs)?;
    Ok(decode_clustered_runs(p, max_symbols, None, &runs))
}
/// Caller supplies the exact validated profile's runs and raw reads, with the
/// same cap. Reuse unchanged topology without dropping any rejection evidence.
pub(crate) fn decode_clustered_reusing(
    p: &[f32],
    max_symbols: usize,
    raw: &Reads,
    runs: &[(usize, usize, bool)],
) -> (Reads, CleanupWork) {
    decode_clustered_runs(p, max_symbols, Some(raw), runs)
}
fn decode_clustered_runs(
    p: &[f32],
    max_symbols: usize,
    raw: Option<&Reads>,
    runs: &[(usize, usize, bool)],
) -> (Reads, CleanupWork) {
    let mut work = CleanupWork {
        input_runs: runs.len(),
        ..CleanupWork::default()
    };
    let mut changes: Vec<(usize, usize, bool)> = vec![];
    let mut i = 0;
    while i + 2 < runs.len() {
        work.examined += 1;
        let w = runs[i + 1].1 - runs[i + 1].0;
        if w * 3 >= runs[i].1 - runs[i].0 {
            i += 1;
            continue;
        }
        let lo = i.saturating_sub(15);
        let hi = (i + 18).min(runs.len());
        let mut widths: Vec<_> = runs[lo..hi].iter().map(|r| r.1 - r.0).collect();
        work.neighborhood_values += widths.len();
        widths.sort_unstable();
        let scale = widths[widths.len() / 4];
        let mut j = i + 1;
        while j < runs.len() && j <= i + 4 && (runs[j].1 - runs[j].0) * 5 < scale {
            j += 1;
        }
        if j == i + 1 || j == runs.len() {
            i += 1;
            continue;
        }
        let (left, right) = (runs[i + 1].0, runs[j].0);
        if (right - left) * 100 >= 35 * scale {
            i += 1;
            continue;
        }
        let min_flank = if runs[i].2 == runs[j].2 {
            3 * (right - left)
        } else {
            scale / 2
        };
        if runs[j].1 - runs[j].0 < min_flank {
            i += 1;
            continue;
        }
        if runs[i].2 == runs[j].2 {
            changes.push((left, right, runs[i].2));
        } else {
            let mass = p[left..right].iter().map(|&v| f64::from(v)).sum::<f64>();
            let edge = if runs[i].2 {
                crate::numeric::usize_f64(left) + mass
            } else {
                crate::numeric::usize_f64(right) - mass
            };
            let edge = crate::numeric::f64_usize(edge.round()).clamp(left, right);
            changes.push((left, edge, runs[i].2));
            changes.push((edge, right, runs[j].2));
        }
        work.removed_pixels += right - left;
        i = j;
    }
    if changes.is_empty() {
        work.output_runs = runs.len();
        let reads = if let Some(raw) = raw {
            let mut r = raw.clone();
            r.windows_examined = 0;
            r.quiet_pass = 0;
            r.guard_pass = 0;
            r.bias_model_pass = 0;
            r.bias_guard_pass = 0;
            r.decoder_calls = 0;
            r
        } else {
            multi_profile::decode_runs(runs, max_symbols)
        };
        return (reads, work);
    }
    let mut binary: Vec<bool> = p.iter().map(|&v| v >= 0.5).collect();
    for (a, b, value) in changes {
        binary[a..b].fill(value);
    }
    let mut cleaned = vec![];
    let (mut a, mut value) = (0, binary[0]);
    for b in 1..=binary.len() {
        let next = b < binary.len() && binary[b];
        if b == binary.len() || next != value {
            cleaned.push((a, b, value));
            a = b;
            value = next;
        }
    }
    work.output_runs = cleaned.len();
    work.removed_runs = work.input_runs.saturating_sub(work.output_runs);
    (multi_profile::decode_runs(&cleaned, max_symbols), work)
}
/// Union visual hypotheses, never oracle-selected values. Either provider's
/// rejection intervals veto both; conflicting overlapping texts veto each other.
/// Equal reads from one source row contribute only one observation.
#[must_use]
pub fn merge_reads(mut raw: Reads, clean: Reads, max_symbols: usize) -> Reads {
    {
        raw.run_visual_capped |= clean.run_visual_capped;
        for v in &clean.run_visual {
            crate::invalid_visual::append(&mut raw.run_visual, &mut raw.run_visual_capped, *v);
        }
    }
    raw.bias_model_pass += clean.bias_model_pass;
    raw.bias_guard_pass += clean.bias_guard_pass;
    raw.windows_examined += clean.windows_examined;
    raw.quiet_pass += clean.quiet_pass;
    raw.guard_pass += clean.guard_pass;
    raw.decoder_calls += clean.decoder_calls;
    raw.truncated |= clean.truncated;
    raw.ambiguous_intervals += clean.ambiguous_intervals;
    raw.rejected_intervals.extend(clean.rejected_intervals);
    let mut all = raw.symbols;
    all.extend(clean.symbols);
    let mut bad = vec![false; all.len()];
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if all[i].left < all[j].right
                && all[j].left < all[i].right
                && all[i].digits != all[j].digits
            {
                bad[i] = true;
                bad[j] = true;
            }
        }
    }
    for (i, h) in all.iter().enumerate() {
        if bad[i] {
            raw.rejected_intervals.push((h.left, h.right));
            raw.ambiguous_intervals += 1;
        }
    }
    raw.ambiguous_intervals = raw.ambiguous_intervals.max(raw.rejected_intervals.len());
    raw.symbols = vec![];
    for (i, h) in all.into_iter().enumerate() {
        if bad[i]
            || raw
                .rejected_intervals
                .iter()
                .any(|&(l, r)| h.left < r && l < h.right)
        {
            continue;
        }
        if raw.symbols.iter().any(|r| {
            r.digits == h.digits
                && r.right.min(h.right) - r.left.max(h.left)
                    > 0.8 * (r.right - r.left).max(h.right - h.left)
        }) {
            continue;
        }
        raw.symbols.push(h);
    }
    raw.symbols.sort_by(|a, b| a.left.total_cmp(&b.left));
    raw.truncated |= raw.symbols.len() > max_symbols;
    raw.symbols.truncate(max_symbols);
    raw
}
#[cfg(test)]
mod tests {
    use super::*;
    const BITS:&[u8]=b"10100010110100111011001100100110111101001110101010110011011011001000010101110010011101000100101";
    fn signal(module: usize) -> Vec<f32> {
        let mut p = vec![0.; 12 * module];
        for &b in BITS {
            p.extend(std::iter::repeat_n(if b == b'1' { 1. } else { 0. }, module));
        }
        p.extend(vec![0.; 12 * module]);
        p
    }
    #[test]
    fn reuse_keeps_vetoes_and_decodes_changed_topology() {
        let mut noisy = signal(16);
        noisy[12 * 16 + 8] = 0.;
        for p in [
            signal(3),
            vec![0.; 512],
            (0..512)
                .map(|i| crate::numeric::f64_f32(f64::from(i % 2)))
                .collect(),
            noisy,
        ] {
            let raw = multi_profile::decode_many(&p, 64).unwrap();
            let (old, ow) = decode_clustered(&p, 64).unwrap();
            let mut runs = vec![];
            multi_profile::sample_runs(&p, 64, &mut runs).unwrap();
            let (new, nw) = decode_clustered_reusing(&p, 64, &raw, &runs);
            assert_eq!(format!("{ow:?}"), format!("{nw:?}"));
            let evidence = |r: &Reads| {
                format!(
                    "{:?} {:?} {} {}",
                    r.symbols, r.rejected_intervals, r.ambiguous_intervals, r.truncated
                )
            };
            assert_eq!(evidence(&old), evidence(&new));
            if nw.removed_pixels == 0 {
                assert_eq!(new.decoder_calls, 0);
                assert_eq!(new.windows_examined, 0);
            } else {
                assert!(new.decoder_calls > 0);
                assert_eq!(new.decoder_calls, old.decoder_calls);
                assert_eq!(new.symbols.len(), 1);
            }
            assert_eq!(
                evidence(&merge_reads(raw.clone(), old, 64)),
                evidence(&merge_reads(raw, new, 64))
            );
        }
        let p = signal(3);
        let mut raw = multi_profile::decode_many(&p, 64).unwrap();
        raw.symbols.clear();
        raw.ambiguous_intervals = 1;
        raw.rejected_intervals.push((30., 300.));
        let mut runs = vec![];
        multi_profile::sample_runs(&p, 64, &mut runs).unwrap();
        let (reused, _) = decode_clustered_reusing(&p, 64, &raw, &runs);
        assert_eq!(reused.rejected_intervals, raw.rejected_intervals);
        assert_eq!(reused.ambiguous_intervals, 1);
    }
    #[test]
    fn isolated_excursions_recover_without_changing_clean_symbols() {
        for module in [1, 3, 8, 16] {
            let p = signal(module);
            let (r, w) = decode_cleaned(&p, 64).unwrap();
            assert_eq!(r.symbols.len(), 1);
            assert_eq!(w.removed_runs, 0);
            if module >= 8 {
                let mut noisy = p.clone();
                let k = 12 * module + module / 2;
                noisy[k] = 1. - noisy[k];
                assert!(multi_profile::decode_many(&noisy, 64)
                    .unwrap()
                    .symbols
                    .is_empty());
                let (r, w) = decode_cleaned(&noisy, 64).unwrap();
                assert_eq!(r.symbols.len(), 1);
                assert_eq!(r.symbols[0].digits, [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
                assert_eq!(w.removed_pixels, 1);
            }
        }
    }
    #[test]
    fn clustered_edge_chatter_preserves_clean_and_low_resolution() {
        for module in [1, 3, 12] {
            let p = signal(module);
            let (r, w) = decode_clustered(&p, 64).unwrap();
            assert_eq!(r.symbols.len(), 1);
            assert_eq!(w.removed_runs, 0);
        }
        let mut pulse = signal(12);
        pulse[12 * 12 + 6] = 0.;
        assert_eq!(decode_clustered(&pulse, 64).unwrap().0.symbols.len(), 1);
        let mut p = signal(16);
        let boundary = 13 * 16;
        p[boundary - 2] = 0.;
        p[boundary - 1] = 1.;
        p[boundary] = 0.;
        p[boundary + 1] = 1.;
        assert!(multi_profile::decode_many(&p, 64)
            .unwrap()
            .symbols
            .is_empty());
        let (r, w) = decode_clustered(&p, 64).unwrap();
        assert_eq!(r.symbols.len(), 1);
        assert!(w.removed_runs >= 2);
    }
    #[test]
    fn preserves_separate_instances_and_rejects_invalid_inputs() {
        let mut p = signal(3);
        p.extend(signal(16));
        let (r, _) = decode_cleaned(&p, 64).unwrap();
        assert_eq!(r.symbols.len(), 2);
        assert!(r.symbols[0].right < r.symbols[1].left);
        for p in [vec![], vec![0.; 4097], vec![f32::NAN; 64]] {
            assert!(decode_cleaned(&p, 64).is_err());
        }
        for p in [
            vec![0.; 4096],
            vec![1.; 4096],
            (0..4096)
                .map(|i| crate::numeric::f64_f32(f64::from(i % 2)))
                .collect(),
        ] {
            let (r, w) = decode_cleaned(&p, 64).unwrap();
            assert!(r.symbols.is_empty());
            assert!(w.examined <= 4096);
            assert!(w.neighborhood_values <= 4096 * 33);
        }
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn combined_hypotheses_preserve_barriers_and_distinct_equal_reads() {
        fn read(l: f64, r: f64, d: u8) -> crate::run_profile::Read {
            crate::run_profile::Read {
                digits: [d; 13],
                left: l,
                right: r,
                cost: 0.,
                gap: 1.,
            }
        }
        fn arm(
            symbols: Vec<crate::run_profile::Read>,
            rejected_intervals: Vec<(f64, f64)>,
        ) -> Reads {
            Reads {
                run_visual: Vec::new(),

                run_visual_capped: false,
                symbols,
                rejected_intervals,
                windows_examined: 0,
                quiet_pass: 0,
                guard_pass: 0,
                bias_model_pass: 0,
                bias_guard_pass: 0,
                decoder_calls: 0,
                ambiguous_intervals: 0,
                truncated: false,
            }
        }
        let r = merge_reads(
            arm(vec![read(0., 100., 1)], vec![]),
            arm(vec![read(1., 101., 2)], vec![]),
            64,
        );
        assert!(r.symbols.is_empty());
        assert_eq!(r.rejected_intervals.len(), 2);
        for reverse in [false, true] {
            let a = arm(vec![read(0., 100., 1)], vec![]);
            let b = arm(vec![], vec![(20., 90.)]);
            let r = if reverse {
                merge_reads(b, a, 64)
            } else {
                merge_reads(a, b, 64)
            };
            assert!(r.symbols.is_empty());
            assert_eq!(r.rejected_intervals.len(), 1);
        }
        let r = merge_reads(
            arm(vec![read(0., 100., 1), read(200., 300., 1)], vec![]),
            arm(vec![read(1., 101., 1), read(201., 301., 1)], vec![]),
            64,
        );
        assert_eq!(r.symbols.len(), 2);
        assert_eq!(r.symbols[0].left, 0.);
        assert_eq!(r.symbols[1].left, 200.);
    }
}

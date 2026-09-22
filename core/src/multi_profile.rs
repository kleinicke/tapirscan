//! Bounded multi-symbol run decoding. Separate source intervals are separate
//! observations, even when they contain equal text. No image-wide early exit.
#![forbid(unsafe_code)]
use crate::{profile::Error, run_profile::Read};
#[derive(Clone, Debug)]
pub struct Reads {
    pub(crate) run_visual: Vec<crate::invalid_visual::RunVisual>,

    pub(crate) run_visual_capped: bool,
    pub symbols: Vec<Read>,
    pub windows_examined: usize,
    pub quiet_pass: usize,
    pub guard_pass: usize,
    pub bias_model_pass: usize,
    pub bias_guard_pass: usize,
    pub decoder_calls: usize,
    pub ambiguous_intervals: usize,
    pub rejected_intervals: Vec<(f64, f64)>,
    pub truncated: bool,
}
/// Decode all admissible run windows in an already normalized dark-is-one path.
/// Limits are explicit: 64..=4096 samples, <=64 returned symbols. This remains
/// an experimental path decoder; callers must assemble spatial support across
/// paths and retain unresolved candidate coverage independently of these reads.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for non-finite samples or values outside [0, 1].
pub fn decode_many(p: &[f32], max_symbols: usize) -> Result<Reads, Error> {
    decode_mode(p, max_symbols, false)
}
/// Diagnostic guard-derived bar/space correction, outside region policy.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for non-finite samples or values outside [0, 1].
pub fn decode_guard_bias(p: &[f32], max_symbols: usize) -> Result<Reads, Error> {
    decode_mode(p, max_symbols, true)
}
fn decode_mode(p: &[f32], max_symbols: usize, guard_bias: bool) -> Result<Reads, Error> {
    let mut runs = Vec::new();
    sample_runs(p, max_symbols, &mut runs)?;
    Ok(decode_positions(&runs, max_symbols, guard_bias))
}
/// One validated threshold extraction shared by raw, cleanup and guard arms.
pub(crate) fn sample_runs(
    p: &[f32],
    max_symbols: usize,
    runs: &mut Vec<(usize, usize, bool)>,
) -> Result<(), Error> {
    if !(64..=4096).contains(&p.len()) || !(1..=64).contains(&max_symbols) {
        return Err(Error::Length);
    }
    if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err(Error::Value);
    }
    sample_runs_validated(p, runs);
    Ok(())
}
/// Private caller has just validated this unchanged profile.
pub(crate) fn sample_runs_validated(p: &[f32], runs: &mut Vec<(usize, usize, bool)>) {
    runs.clear();
    let (mut start, mut black) = (0, p[0] >= 0.5);
    for i in 1..=p.len() {
        let next = i < p.len() && p[i] >= 0.5;
        if i == p.len() || next != black {
            runs.push((start, i, black));
            start = i;
            black = next;
        }
    }
}
pub(crate) fn decode_guard_runs(runs: &[(usize, usize, bool)], max_symbols: usize) -> Reads {
    decode_positions(runs, max_symbols, true)
}
/// Experimental linear threshold-crossing positions on the identical signal.
/// No transitions are added/removed; structural and visual gates are unchanged.
/// This is a diagnostic alternative, not enabled by the region scanner.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for non-finite samples or values outside [0, 1].
pub fn decode_fractional(p: &[f32], max_symbols: usize) -> Result<Reads, Error> {
    decode_fractional_mode(p, max_symbols, false)
}
/// Observed guard correction using fractional edges, without digit-guided fitting.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for non-finite samples or values outside [0, 1].
pub fn decode_fractional_guard_bias(p: &[f32], max_symbols: usize) -> Result<Reads, Error> {
    decode_fractional_mode(p, max_symbols, true)
}
fn decode_fractional_mode(p: &[f32], max_symbols: usize, guard_bias: bool) -> Result<Reads, Error> {
    if !(64..=4096).contains(&p.len()) || !(1..=64).contains(&max_symbols) {
        return Err(Error::Length);
    }
    if p.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err(Error::Value);
    }
    let mut runs = Vec::with_capacity(p.len());
    let (mut start, mut black) = (0., p[0] >= 0.5);
    for i in 1..=p.len() {
        let next = i < p.len() && p[i] >= 0.5;
        if i == p.len() || next != black {
            // Run coordinates include a half-sample offset, matching the legacy
            // integer boundary convention. Output endpoints subtract it below.
            let end = if i == p.len() {
                crate::numeric::usize_f64(i)
            } else {
                crate::numeric::usize_f64(i) - 0.5
                    + (0.5 - f64::from(p[i - 1])) / (f64::from(p[i]) - f64::from(p[i - 1]))
            };
            runs.push((start, end, black));
            start = end;
            black = next;
        }
    }
    Ok(decode_positions(&runs, max_symbols, guard_bias))
}
/// Adjacent-extrema midpoint crossings on an unchanged threshold-run topology.
/// Weak local contrast cannot invent an edge. Every new coordinate remains
/// between the adjacent observed extrema; digit/checksum results never fit it.
/// # Errors
/// Returns `Length` for unsupported profile size or symbol limit, or `Value` for non-finite samples or values outside [0, 1].
pub fn decode_relative_edges(
    profile: &[f32],
    max_symbols: usize,
    guard_bias: bool,
) -> Result<Reads, Error> {
    if !(64..=4096).contains(&profile.len()) || !(1..=64).contains(&max_symbols) {
        return Err(Error::Length);
    }
    if profile
        .iter()
        .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
    {
        return Err(Error::Value);
    }
    let mut starts = vec![0];
    for i in 1..profile.len() {
        if (profile[i] >= 0.5) != (profile[i - 1] >= 0.5) {
            starts.push(i);
        }
    }
    starts.push(profile.len());
    let mut edges = vec![0.];
    for k in 1..starts.len() - 1 {
        let (a, b, c) = (starts[k - 1], starts[k], starts[k + 1]);
        let dark = profile[b] >= 0.5;
        // Choose nearest extrema on ties, keeping the edge's own flanks local.
        let left = (a..b)
            .rev()
            .max_by(|&i, &j| {
                let order = profile[i].total_cmp(&profile[j]);
                (if dark { order.reverse() } else { order }).then_with(|| i.cmp(&j))
            })
            .unwrap_or(a);
        let right = (b..c)
            .max_by(|&i, &j| {
                let order = profile[i].total_cmp(&profile[j]);
                (if dark { order } else { order.reverse() }).then_with(|| j.cmp(&i))
            })
            .unwrap_or(a);
        let threshold = (f64::from(profile[left]) + f64::from(profile[right])) * 0.5;
        let original = crate::numeric::usize_f64(b) - 0.5
            + (0.5 - f64::from(profile[b - 1]))
                / (f64::from(profile[b]) - f64::from(profile[b - 1]));
        let mut edge = original;
        let mut distance = f64::INFINITY;
        if (profile[left] - profile[right]).abs() >= 0.25 {
            for i in left + 1..=right {
                let (v, next_value) = (f64::from(profile[i - 1]), f64::from(profile[i]));
                if (dark && v < threshold && next_value >= threshold)
                    || (!dark && v >= threshold && next_value < threshold)
                {
                    let crossing =
                        crate::numeric::usize_f64(i) - 0.5 + (threshold - v) / (next_value - v);
                    let crossing_distance = (crossing - original).abs();
                    if crossing_distance < distance {
                        distance = crossing_distance;
                        edge = crossing;
                    }
                }
            }
        }
        edges.push(edge);
    }
    edges.push(crate::numeric::usize_f64(profile.len()));
    let runs: Vec<_> = (0..starts.len() - 1)
        .map(|i| (edges[i], edges[i + 1], profile[starts[i]] >= 0.5))
        .collect();
    // A non-monotone model has no physical interpretation. Retain the original
    // decoder's evidence in the caller instead of accepting crossed boundaries.
    if runs.iter().any(|r| r.1 <= r.0) {
        return Ok(decode_positions::<f64>(&[], max_symbols, false));
    }
    let raw = decode_positions(&runs, max_symbols, false);
    Ok(if guard_bias {
        crate::transition::merge_reads(raw, decode_positions(&runs, max_symbols, true), max_symbols)
    } else {
        raw
    })
}

/// Scratch storage for the three local-signal hypotheses. Threshold decisions
/// are identical across these arms; only edge coordinates/guard correction vary.
#[derive(Default)]
pub(crate) struct LocalRuns {
    identical_models: bool,

    pub(crate) reused_calls: std::cell::Cell<usize>,
    integer: Vec<(usize, usize, bool)>,
    fractional: Vec<(f64, f64, bool)>,
}
pub(crate) fn decode_local_variants(
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
    decode_local_variants_validated(p, max_symbols, guard_bias, scratch)
}
/// Private caller has already validated profile length and values.
#[expect(
    clippy::unnecessary_wraps,
    reason = "The validated decoder shares the fallible interface used by profile decoding callers."
)]
pub(crate) fn decode_local_variants_validated(
    p: &[f32],
    max_symbols: usize,
    guard_bias: bool,
    scratch: &mut LocalRuns,
) -> Result<Reads, Error> {
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

    {
        scratch.reused_calls.set(0);
        scratch.identical_models = exact_same_runs(&scratch.integer, &scratch.fractional);
    }
    let fractional = decode_positions(&scratch.fractional, max_symbols, false);

    let integer = if scratch.identical_models {
        scratch.reused_calls.set(scratch.reused_calls.get() + 1);
        fractional.clone()
    } else {
        decode_positions(&scratch.integer, max_symbols, false)
    };

    let reads = crate::transition::merge_reads(fractional, integer, max_symbols);

    let mut duplicate_guard = None;
    let reads = if guard_bias {
        let bias = decode_positions(&scratch.integer, max_symbols, true);

        if scratch.identical_models {
            duplicate_guard = Some(bias.clone());
        }
        crate::transition::merge_reads(reads, bias, max_symbols)
    } else {
        reads
    };

    let reads = if guard_bias {
        let bias = if let Some(reads) = duplicate_guard {
            scratch.reused_calls.set(scratch.reused_calls.get() + 1);
            reads
        } else {
            decode_positions(&scratch.fractional, max_symbols, true)
        };

        crate::transition::merge_reads(reads, bias, max_symbols)
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
/// Remove only weak, sub-half-module threshold excursions between strong
/// equal-polarity flanks. Original hypotheses still veto conflicting reads.
/// No digit information is used and no missing transition is inserted.
fn weak_excursions(
    profile: &[f32],
    runs: &[(usize, usize, bool)],
) -> Option<Vec<(usize, usize, bool)>> {
    let mut out: Option<Vec<(usize, usize, bool)>> = None;
    let mut i = 0;
    while i < runs.len() {
        if i + 2 < runs.len() {
            let (a, b, dark) = runs[i + 1];
            let width = b - a;
            if runs[i].1 - runs[i].0 >= 2 * width
                && runs[i + 2].1 - runs[i + 2].0 >= 2 * width
                && profile[a..b].iter().all(|&v| (v - 0.5).abs() <= 0.10)
            {
                let mut widths: Vec<_> = runs[i.saturating_sub(15)..(i + 18).min(runs.len())]
                    .iter()
                    .map(|r| r.1 - r.0)
                    .collect();
                let quad = widths.len() / 4;
                let scale = *widths.select_nth_unstable(quad).1;
                let strong = |r: (usize, usize, bool)| {
                    profile[r.0..r.1]
                        .iter()
                        .any(|&v| if dark { v <= 0.25 } else { v >= 0.75 })
                };
                if width * 2 < scale && strong(runs[i]) && strong(runs[i + 2]) {
                    let out = out.get_or_insert_with(|| {
                        let mut v = Vec::with_capacity(runs.len());
                        v.extend_from_slice(&runs[..i]);
                        v
                    });
                    out.push((runs[i].0, runs[i + 2].1, runs[i].2));
                    i += 3;
                    continue;
                }
            }
        }
        if let Some(out) = out.as_mut() {
            if let Some(last) = out.last_mut() {
                if last.2 == runs[i].2 {
                    last.1 = runs[i].1;
                    i += 1;
                    continue;
                }
            }
            out.push(runs[i]);
        }
        i += 1;
    }
    out
}
#[cfg(test)]
mod weak_tests {
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
}
pub(crate) fn decode_runs(runs: &[(usize, usize, bool)], max_symbols: usize) -> Reads {
    decode_positions(runs, max_symbols, false)
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
pub(crate) trait Position: Copy {
    fn value(self) -> f64;
}
#[cfg(any(feature = "mode-high", feature = "mode-very-high"))]
trait Position: Copy {
    fn value(self) -> f64;
}
impl Position for usize {
    fn value(self) -> f64 {
        crate::numeric::usize_f64(self)
    }
}
impl Position for f64 {
    fn value(self) -> f64 {
        self
    }
}
fn decode_positions<T: Position>(
    runs: &[(T, T, bool)],
    max_symbols: usize,
    guard_bias: bool,
) -> Reads {
    decode_positions_mode(runs, max_symbols, guard_bias, false)
}

pub(crate) fn decode_short_quiet(
    runs: &[(usize, usize, bool)],
    max_symbols: usize,
    guard_bias: bool,
) -> Reads {
    let raw = decode_positions_mode(runs, max_symbols, false, true);
    if guard_bias {
        crate::transition::merge_reads(
            raw,
            decode_positions_mode(runs, max_symbols, true, true),
            max_symbols,
        )
    } else {
        raw
    }
}
#[expect(
    clippy::float_cmp,
    reason = "These values identify the same sampled path or decoded interval; approximate equality would merge distinct evidence and change work ordering."
)]
#[expect(
    clippy::too_many_lines,
    reason = "Ordered decoding alternatives share ambiguity vetoes, symbol limits and duplicate accounting within one profile."
)]
fn decode_positions_mode<T: Position>(
    runs: &[(T, T, bool)],
    max_symbols: usize,
    guard_bias: bool,
    short_quiet: bool,
) -> Reads {
    let mut hypotheses: Vec<Read> = Vec::new();

    let (mut run_visual, mut run_visual_capped) = (Vec::new(), false);
    let mut windows_examined = 0;
    let (mut quiet_pass, mut guard_pass, mut decoder_calls) = (0, 0, 0);
    let (mut bias_model_pass, mut bias_guard_pass) = (0, 0);
    for r in runs.windows(61) {
        if !r[1].2 {
            continue;
        }
        windows_examined += 1;
        let (left, right) = (r[1].0.value(), r[59].1.value());
        let module = crate::numeric::f64_f32(right - left) / 95.;
        let qleft = crate::numeric::f64_f32(r[0].1.value() - r[0].0.value()) / module;
        let qright = crate::numeric::f64_f32(r[60].1.value() - r[60].0.value()) / module;
        // Quiet-zone width follows the adjacent observed guard pitch on curved
        // labels. Full-quiet acceptance is unchanged; this remains four-row evidence.
        let left_pitch = crate::numeric::f64_f32(r[3].1.value() - r[1].0.value()) / 3.;
        let right_pitch = crate::numeric::f64_f32(r[59].1.value() - r[57].0.value()) / 3.;
        let local_left = crate::numeric::f64_f32(r[0].1.value() - r[0].0.value()) / left_pitch;
        let local_right = crate::numeric::f64_f32(r[60].1.value() - r[60].0.value()) / right_pitch;
        let ordinary_short = (local_left >= 4. && local_right >= 4.)
            || (local_left >= 6. && local_right >= 3.)
            || (local_left >= 3. && local_right >= 6.);
        // A full, visually strong 59-run symbol may end beside packaging. Keep
        // this weaker boundary observation in the existing four-row source
        // confirmation path; never synthesize a guard, run, or missing payload.
        // The narrow side must end at an observed exterior dark transition,
        // not at a truncated sampling-window edge.
        let asymmetric_short = !ordinary_short
            && ((local_left >= 9.
                && local_right >= 1.
                && r[60].1.value() < runs.last().unwrap().1.value())
                || (local_right >= 9. && local_left >= 1. && r[0].0.value() > runs[0].0.value()));
        if module < 0.8
            || if short_quiet {
                !(ordinary_short || asymmetric_short) || (qleft >= 7. && qright >= 7.)
            } else {
                crate::numeric::f64_f32(r[0].1.value() - r[0].0.value()) < 7. * module
                    || crate::numeric::f64_f32(r[60].1.value() - r[60].0.value()) < 7. * module
            }
        {
            continue;
        }
        quiet_pass += 1;
        let mut widths = [0.; 59];
        for j in 0..59 {
            widths[j] = crate::numeric::f64_f32(r[j + 1].1.value() - r[j + 1].0.value());
        }
        if [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58]
            .iter()
            .all(|&j| (widths[j] / module - 1.).abs() <= 0.65)
        {
            guard_pass += 1;
        }
        if guard_bias {
            let Some(corrected) = crate::run_ean::guard_bias_widths(&widths) else {
                continue;
            };
            bias_model_pass += 1;
            let pitch = corrected.iter().sum::<f32>() / 95.;
            if [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58]
                .iter()
                .all(|&j| (corrected[j] / pitch - 1.).abs() <= 0.65)
            {
                bias_guard_pass += 1;
            }
            widths = corrected;
        }
        for reversed in [false, true] {
            decoder_calls += 1;
            if reversed {
                widths.reverse();
            }
            if short_quiet && !(local_left >= 4. && local_right >= 4.) {
                let (l, r) = if reversed {
                    (local_right, local_left)
                } else {
                    (local_left, local_right)
                };
                if asymmetric_short {
                    if l < 9. || r < 1. {
                        continue;
                    }
                } else if l < 6. || r < 3. {
                    continue;
                }
            }

            let evidence = crate::run_ean::decode_visual_evidence(&widths).and_then(|e| {
                let valid = crate::ean::checksum(&e.digits);
                if !(short_quiet && (e.cost > 0.06 || e.gap < 0.1))
                    && !(short_quiet && asymmetric_short && (e.cost > 0.035 || e.gap < 0.2))
                    && (valid || (e.cost <= 0.06 && e.gap >= 0.1))
                {
                    let read = Read {
                        digits: e.digits,
                        left: left - 0.5,
                        right: right - 0.5,
                        cost: e.cost,
                        gap: e.gap,
                    };
                    crate::invalid_visual::append(
                        &mut run_visual,
                        &mut run_visual_capped,
                        crate::invalid_visual::RunVisual {
                            read,
                            checksum_valid: valid,
                        },
                    );
                }
                valid.then_some(e)
            });

            if let Some(e) = evidence {
                if short_quiet && (e.cost > 0.06 || e.gap < 0.1) {
                    continue;
                }
                if short_quiet && asymmetric_short && (e.cost > 0.035 || e.gap < 0.2) {
                    continue;
                }
                hypotheses.push(Read {
                    digits: e.digits,
                    left: left - 0.5,
                    right: right - 0.5,
                    cost: e.cost,
                    gap: e.gap,
                });
            }
        }
    }
    // Any competing text on overlapping evidence vetoes that interval. Do not
    // let equal-value grouping bridge two physical instances through a broad fit.
    let overlaps = |a: &Read, b: &Read| a.left < b.right && b.left < a.right;
    let mut veto = vec![false; hypotheses.len()];
    for i in 0..hypotheses.len() {
        for j in i + 1..hypotheses.len() {
            if overlaps(&hypotheses[i], &hypotheses[j])
                && hypotheses[i].digits != hypotheses[j].digits
            {
                veto[i] = true;
                veto[j] = true;
            }
        }
    }
    let ambiguous_intervals = veto.iter().filter(|&&x| x).count();
    let rejected_intervals = hypotheses
        .iter()
        .zip(&veto)
        .filter_map(|(r, &bad)| bad.then_some((r.left, r.right)))
        .collect();
    let mut symbols: Vec<Read> = Vec::new();
    // Only identical boundaries/text are duplicate hypotheses. Other observations
    // retain their geometry; broader cross-path association is the caller's job.
    for (i, h) in hypotheses.into_iter().enumerate() {
        if veto[i] {
            continue;
        }
        if let Some(old) = symbols
            .iter_mut()
            .find(|r| r.left == h.left && r.right == h.right && r.digits == h.digits)
        {
            if h.cost < old.cost {
                *old = h;
            }
        } else {
            symbols.push(h);
        }
    }
    symbols.sort_by(|a, b| a.left.total_cmp(&b.left));
    let truncated = symbols.len() > max_symbols;
    symbols.truncate(max_symbols);
    Reads {
        run_visual,

        run_visual_capped,
        symbols,
        windows_examined,
        quiet_pass,
        guard_pass,
        bias_model_pass,
        bias_guard_pass,
        decoder_calls,
        ambiguous_intervals,
        rejected_intervals,
        truncated,
    }
}
#[cfg(test)]
mod tests {
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
                            let lo = (12. + crate::numeric::usize_f64(i)) * pitch
                                - bias * pitch / 2.
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
}

/// Reuse observed locally normalized runs, preserving stricter short-quiet gates.
pub(crate) fn decode_prepared_short(
    runs: &LocalRuns,
    max_symbols: usize,
    guard_bias: bool,
) -> Reads {
    let a = decode_positions_mode(&runs.integer, max_symbols, false, true);

    let b = if runs.identical_models {
        runs.reused_calls.set(runs.reused_calls.get() + 1);
        a.clone()
    } else {
        decode_positions_mode(&runs.fractional, max_symbols, false, true)
    };

    let reads = crate::transition::merge_reads(a, b, max_symbols);
    if guard_bias {
        let a = decode_positions_mode(&runs.integer, max_symbols, true, true);

        let b = if runs.identical_models {
            runs.reused_calls.set(runs.reused_calls.get() + 1);
            a.clone()
        } else {
            decode_positions_mode(&runs.fractional, max_symbols, true, true)
        };

        crate::transition::merge_reads(
            reads,
            crate::transition::merge_reads(a, b, max_symbols),
            max_symbols,
        )
    } else {
        reads
    }
}
#[cfg(test)]
mod asymmetric_quiet_tests {
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
}

// Position::value feeds exactly these f64 coordinates to the pure decoder.
// No tolerance, text identity or checksum participates in cache eligibility.

fn exact_same_runs(integer: &[(usize, usize, bool)], fractional: &[(f64, f64, bool)]) -> bool {
    integer.len() == fractional.len()
        && integer
            .iter()
            .zip(fractional)
            .all(|(&(a, b, d), &(x, y, e))| {
                crate::numeric::usize_f64(a).to_bits() == x.to_bits()
                    && crate::numeric::usize_f64(b).to_bits() == y.to_bits()
                    && d == e
            })
}
#[cfg(test)]
mod redundant_exact_tests {
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
                        let want =
                            reference_local_variants(&p, max, guard, &mut reference).unwrap();
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
}

#[path = "extrema_runs.rs"]
mod extrema_runs;

pub(crate) use extrema_runs::{decode_extrema, decode_extrema_validated, ExtremaScratch};
#[cfg(test)]
mod folded_boundary_tests {
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
        for line in include_str!("../fixtures/folded_profiles.txt").lines() {
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
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
/// Research adapter: original EAN13 decisions and evidence remain intact.
/// Short layouts consume the same extracted runs, with a separate output cap.
#[derive(Default)]
pub struct RetailScratch {
    primary: LocalRuns,
    starts: Vec<usize>,
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
pub struct RetailReads {
    pub ean13: Reads,
    pub short: retail_short::ShortReads,
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
/// # Errors
/// Rejects invalid profile geometry or unsupported family masks.
pub fn decode_retail_profile(
    p: &[f32],
    max_symbols: usize,
    mask: u32,
    guard_bias: bool,
    scratch: &mut RetailScratch,
) -> Result<RetailReads, Error> {
    // This first adapter always keeps EAN13 enabled. UPC-A is an output alias.
    if mask & 1 == 0 || mask & !15 != 0 {
        return Err(Error::Value);
    }
    let ean13 = decode_local_variants(p, max_symbols, guard_bias, &mut scratch.primary)?;
    let short = short_from_existing(
        &scratch.primary,
        mask,
        max_symbols,
        &ean13,
        &mut scratch.starts,
    );
    Ok(RetailReads { ean13, short })
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
/// Baseline access to the original local-profile stage and its scratch reuse.
/// # Errors
/// Rejects invalid profile geometry or unsupported family masks.
pub fn decode_legacy_local(
    p: &[f32],
    max_symbols: usize,
    guard_bias: bool,
    scratch: &mut RetailScratch,
) -> Result<Reads, Error> {
    decode_local_variants(p, max_symbols, guard_bias, &mut scratch.primary)
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
pub(crate) fn short_from_existing(
    runs: &LocalRuns,
    mask: u32,
    max_symbols: usize,
    ean13: &Reads,
    starts: &mut Vec<usize>,
) -> retail_short::ShortReads {
    let mut out = retail_short::ShortReads::default();
    if mask & 12 != 0 {
        let fractional_covered = retail_short::fully_covered(&runs.fractional, ean13);
        let integer_covered = retail_short::fully_covered(&runs.integer, ean13);
        if fractional_covered && integer_covered {
            return out;
        }
        // Both edge models have exactly the same topology. Fractional edges
        // lie within adjacent samples, so a >=5.5-sample quiet run must have
        // an integer width >=4. Collect admissible starts once, without pruning
        // any window that the existing acceptance gates could admit.
        starts.clear();
        starts.extend(
            (1..runs.integer.len()).filter(|&i| {
                runs.integer[i].2 && runs.integer[i - 1].1 - runs.integer[i - 1].0 >= 1
            }),
        );
        if !fractional_covered {
            retail_short::decode_selected_runs(
                &runs.fractional,
                starts,
                mask,
                max_symbols,
                ean13,
                &mut out,
            );
        }

        let same = runs.identical_models;

        if !same && !integer_covered {
            retail_short::decode_selected_runs(
                &runs.integer,
                starts,
                mask,
                max_symbols,
                ean13,
                &mut out,
            );
        }
    }
    out
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
pub(crate) fn short_from_extrema(
    scratch: &ExtremaScratch,
    mask: u32,
    max_symbols: usize,
    primary: &Reads,
    out: &mut retail_short::ShortReads,
) {
    if mask & 12 != 0 && !scratch.capped && !scratch.ambiguous {
        retail_short::decode_runs(&scratch.runs, mask, max_symbols, primary, out);
    }
}
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
#[path = "retail_short.rs"]
pub mod retail_short;

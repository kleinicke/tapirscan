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
mod weak_tests;
pub(crate) fn decode_runs(runs: &[(usize, usize, bool)], max_symbols: usize) -> Reads {
    decode_positions(runs, max_symbols, false)
}
pub(crate) trait Position: Copy {
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
mod tests;

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
mod asymmetric_quiet_tests;

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
mod redundant_exact_tests;

#[path = "extrema_runs.rs"]
mod extrema_runs;

pub(crate) use extrema_runs::{decode_extrema, decode_extrema_validated, ExtremaScratch};
#[cfg(test)]
mod folded_boundary_tests;
/// Research adapter: original EAN13 decisions and evidence remain intact.
/// Short layouts consume the same extracted runs, with a separate output cap.
#[derive(Default)]
pub struct RetailScratch {
    primary: LocalRuns,
    starts: Vec<usize>,
}
pub struct RetailReads {
    pub ean13: Reads,
    pub short: retail_short::ShortReads,
}
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
#[path = "retail_short.rs"]
pub mod retail_short;

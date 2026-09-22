//! Additional layouts over existing primary run evidence. No image search,
//! expected text, checksum repair, resampling, or threshold sweep.
// Preserve validated arithmetic and observation layout. Coordinates are bounded
// by image/profile limits; digit and pixel casts follow explicit clamps.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    clippy::needless_range_loop,
    clippy::trivially_copy_pass_by_ref
)]
use super::{Position, Reads};
use crate::run_ean::{digit, digit_pair};

const UPC_PARITY: [u8; 10] = [56, 52, 50, 49, 44, 38, 35, 42, 41, 37];
#[derive(Clone, Debug)]
pub struct ShortRead {
    pub digits: [u8; 8],
    pub format: u32,
    pub left: f64,
    pub right: f64,
    pub cost: f32,
    pub gap: f32,
    pub recovery: bool,
}
#[derive(Default, Debug)]
pub struct ShortReads {
    pub symbols: Vec<ShortRead>,
    pub starts: usize,
    pub quiet_windows: usize,
    pub digit_calls: usize,
    pub conflicts: Vec<(f64, f64)>,
    pub truncated: bool,
}
fn checksum(d: &[u8]) -> bool {
    d.iter()
        .rev()
        .enumerate()
        .map(|(i, v)| usize::from(*v) * if i % 2 == 0 { 1 } else { 3 })
        .sum::<usize>()
        % 10
        == 0
}
#[must_use]
pub fn expand_upce(d: &[u8; 8]) -> [u8; 12] {
    let mut out = [0; 12];
    out[..3].copy_from_slice(&d[..3]);
    match d[6] {
        0..=2 => {
            out[3] = d[6];
            out[8..11].copy_from_slice(&d[3..6]);
        }
        3 => {
            out[3] = d[3];
            out[9..11].copy_from_slice(&d[4..6]);
        }
        4 => {
            out[3..5].copy_from_slice(&d[3..5]);
            out[10] = d[5];
        }
        _ => {
            out[3..6].copy_from_slice(&d[3..6]);
            out[10] = d[6];
        }
    }
    out[11] = d[7];
    out
}
fn decode_widths_base(w: &[f32], format: u32, calls: &mut usize) -> Option<([u8; 8], f32, f32)> {
    let (modules, guards): (f32, &[usize]) = if format == 4 {
        (67., &[0, 1, 2, 19, 20, 21, 22, 23, 40, 41, 42])
    } else {
        (51., &[0, 1, 2, 27, 28, 29, 30, 31, 32])
    };
    let module = w.iter().sum::<f32>() / modules;
    if module < 0.8 || guards.iter().any(|&i| (w[i] / module - 1.).abs() > 0.65) {
        return None;
    }
    let count = if format == 4 { 8 } else { 6 };
    for i in 0..count {
        let at = 3 + i * 4 + if format == 4 && i >= 4 { 5 } else { 0 };
        let scale = w[at..at + 4].iter().sum::<f32>() / 7.;
        if !(0.55 * module..=1.8 * module).contains(&scale)
            || w[at..at + 4]
                .iter()
                .any(|v| !(0.4..=4.6).contains(&(v / scale)))
        {
            return None;
        }
    }
    *calls += 1;
    if format == 4 {
        let mut digits = [0; 8];
        let (mut cost, mut max, mut gap) = (0f32, 0f32, f32::INFINITY);
        for (i, output) in digits.iter_mut().enumerate() {
            let at = 3 + i * 4 + if i >= 4 { 5 } else { 0 };
            let d = digit(&w[at..at + 4], b'L');
            *output = d.value;
            cost += d.cost;
            max = max.max(d.cost);
            gap = gap.min(d.gap);
        }
        cost /= 8.;
        return (cost <= 0.12 && max <= 0.35 && gap >= 0.05 && checksum(&digits))
            .then_some((digits, cost, gap));
    }
    let pairs: [_; 6] = std::array::from_fn(|i| digit_pair(&w[3 + i * 4..7 + i * 4]));
    let mut best = None;
    let mut best_cost = f32::INFINITY;
    let mut tied = false;
    // Select parity using visual evidence first; checksum only rejects it.
    for system in 0..=1u8 {
        for (check, &base_parity) in UPC_PARITY.iter().enumerate() {
            let parity = base_parity ^ if system == 0 { 0 } else { 63 };
            let mut digits = [0; 8];
            digits[0] = system;
            digits[7] = check as u8;
            let (mut cost, mut max, mut gap) = (0f32, 0f32, f32::INFINITY);
            for i in 0..6 {
                let d = pairs[i][usize::from((parity >> (5 - i)) & 1)];
                digits[i + 1] = d.value;
                cost += d.cost;
                max = max.max(d.cost);
                gap = gap.min(d.gap);
            }
            cost /= 6.;
            if cost < best_cost {
                best_cost = cost;
                best = Some((digits, max, gap));
                tied = false;
            } else if cost == best_cost {
                tied = true;
            }
        }
    }
    let (digits, max, gap) = best?;
    (!tied && best_cost <= 0.12 && max <= 0.35 && gap >= 0.05 && checksum(&expand_upce(&digits)))
        .then_some((digits, best_cost, gap))
}
fn overlap(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}
pub(crate) fn insert(out: &mut ShortReads, read: ShortRead, max_symbols: usize) {
    let interval = (read.left, read.right);
    if out.conflicts.iter().any(|&v| overlap(v, interval)) {
        return;
    }
    if let Some(i) = out
        .symbols
        .iter()
        .position(|r| overlap((r.left, r.right), interval))
    {
        let old = &out.symbols[i];
        if old.format != read.format || old.digits != read.digits {
            let rejected = (old.left.min(read.left), old.right.max(read.right));
            out.symbols
                .retain(|r| !overlap((r.left, r.right), rejected));
            out.conflicts.push(rejected);
        } else if read.cost < old.cost {
            out.symbols[i] = read;
        }
    } else if out.symbols.len() < max_symbols {
        out.symbols.push(read);
    } else {
        out.truncated = true;
    }
}
pub(crate) fn decode_runs<T: Position>(
    runs: &[(T, T, bool)],
    mask: u32,
    max_symbols: usize,
    primary: &Reads,
    out: &mut ShortReads,
) {
    if fully_covered(runs, primary) {
        return;
    }
    decode_indices(runs, 1..runs.len(), mask, max_symbols, primary, out);
}
pub(super) fn fully_covered<T: Position>(runs: &[(T, T, bool)], primary: &Reads) -> bool {
    if primary.symbols.is_empty() {
        return false;
    }
    let Some(first) = runs.iter().find(|r| r.2) else {
        return true;
    };
    let last = runs.iter().rev().find(|r| r.2).unwrap();
    // The existing policy already rejects any short interpretation overlapping
    // a primary read. If all observed ink lies inside one such interval, no
    // additional disjoint read can survive. This is exact policy reuse.
    primary
        .symbols
        .iter()
        .any(|r| r.left <= first.0.value() - 0.5 && r.right >= last.1.value() - 0.5)
}
pub(super) fn decode_selected_runs<T: Position>(
    runs: &[(T, T, bool)],
    starts: &[usize],
    mask: u32,
    max_symbols: usize,
    primary: &Reads,
    out: &mut ShortReads,
) {
    decode_indices(
        runs,
        starts.iter().copied(),
        mask,
        max_symbols,
        primary,
        out,
    );
}
fn decode_indices<T: Position>(
    runs: &[(T, T, bool)],
    indices: impl Iterator<Item = usize>,
    mask: u32,
    max_symbols: usize,
    primary: &Reads,
    out: &mut ShortReads,
) {
    for start in indices {
        if !runs[start].2 {
            continue;
        }
        out.starts += 1;
        if start + 33 >= runs.len() {
            continue;
        }
        // Necessary consequences of existing acceptance gates, with slack for
        // float rounding. Not a heuristic transition-count rejection.
        let quiet = runs[start - 1].1.value() - runs[start - 1].0.value();
        if quiet < 1.5 {
            continue;
        }
        let guard_max = (0..3)
            .map(|i| runs[start + i].1.value() - runs[start + i].0.value())
            .fold(0f64, f64::max);
        if quiet < 1.2 * guard_max {
            continue;
        }
        for (format, count, modules) in [(4, 43, 67.), (8, 33, 51.)] {
            if mask & format == 0 || start + count >= runs.len() {
                continue;
            }
            let end = start + count;
            let left = runs[start].0.value();
            let right = runs[end - 1].1.value();
            let module = (right - left) / modules;
            // Actual exterior white runs on both ends; no crop-edge exception.
            if module < 0.8
                || runs[start - 1].1.value() - runs[start - 1].0.value()
                    < (if format == 4 { 2. } else { 4. }) * module
                || runs[end].1.value() - runs[end].0.value()
                    < (if format == 4 { 2. } else { 4. }) * module
            {
                continue;
            }
            // Keep the primary's rejection evidence as well as its successes.
            if primary
                .symbols
                .iter()
                .any(|r| overlap((r.left, r.right), (left, right)))
                || primary
                    .rejected_intervals
                    .iter()
                    .any(|&r| overlap(r, (left, right)))
            {
                continue;
            }
            out.quiet_windows += 1;
            let mut widths = [0.; 43];
            for i in 0..count {
                widths[i] = (runs[start + i].1.value() - runs[start + i].0.value()) as f32;
            }
            for reverse in [false, true] {
                if reverse {
                    widths[..count].reverse();
                }
                if let Some((digits, cost, gap)) =
                    decode_widths(&widths[..count], format, &mut out.digit_calls)
                {
                    if format == 8 && !confirms_upce(&widths[..count], &digits) {
                        continue;
                    }
                    insert(
                        out,
                        ShortRead {
                            digits,
                            format,
                            left: left - 0.5,
                            right: right - 0.5,
                            cost,
                            gap,
                            recovery: false,
                        },
                        max_symbols,
                    );
                }
            }
        }
    }
}

// Guard-derived print/threshold bias. The image supplies the estimate; there is
// no expected-text input or search for a checksum-valid weaker digit sequence.
fn guard_gain(w: &[f32], start: usize, count: usize) -> f32 {
    let (mut dark, mut light, mut nd, mut nl) = (0., 0., 0., 0.);
    for i in start..start + count {
        if i % 2 == 0 {
            dark += w[i];
            nd += 1.;
        } else {
            light += w[i];
            nl += 1.;
        }
    }
    let (a, b) = (dark / nd, light / nl);
    ((a - b) / (a + b)).clamp(-0.45, 0.45)
}
fn decode_widths(w: &[f32], format: u32, calls: &mut usize) -> Option<([u8; 8], f32, f32)> {
    let original = decode_widths_base(w, format, calls);
    if original.is_some() || format != 4 {
        return original;
    }
    let mut gains = [
        guard_gain(w, 0, 3),
        guard_gain(w, 19, 5),
        guard_gain(w, 40, 3),
    ];
    let mut sorted = gains;
    sorted.sort_by(f32::total_cmp);
    if 2 == 1 {
        gains.fill(sorted[1]);
    }
    if gains.iter().all(|g| g.abs() < 0.025) {
        return None;
    }
    let mut corrected = [0.; 43];
    for i in 0..43 {
        let module = if i < 3 {
            w[..3].iter().sum::<f32>() / 3.
        } else if i >= 40 {
            w[40..].iter().sum::<f32>() / 3.
        } else if (19..24).contains(&i) {
            w[19..24].iter().sum::<f32>() / 5.
        } else {
            let start = if i < 19 {
                3 + (i - 3) / 4 * 4
            } else {
                24 + (i - 24) / 4 * 4
            };
            w[start..start + 4].iter().sum::<f32>() / 7.
        };
        let g = if i < 21 {
            gains[0] + (gains[1] - gains[0]) * i as f32 / 21.
        } else {
            gains[1] + (gains[2] - gains[1]) * (i - 21) as f32 / 21.
        };
        corrected[i] = w[i] - g * module * if i % 2 == 0 { 1. } else { -1. };
        if corrected[i] <= 0. {
            return None;
        }
    }
    decode_widths_base(&corrected, format, calls)
}

// Existing independent retail parser, restricted to the already sampled runs.
// Fallback observations are weak and need four distinct source rows.
pub(crate) fn legacy_runs(
    runs: &[(f64, f64, bool)],
    mask: u32,
    max_symbols: usize,
    primary: &Reads,
    out: &mut ShortReads,
) {
    if runs.is_empty() || fully_covered(runs, primary) {
        return;
    }
    let mut widths: Vec<f32> = runs.iter().map(|r| (r.1 - r.0) as f32).collect();
    for reverse in [false, true] {
        if reverse {
            widths.reverse();
        }
        let dark = if reverse {
            runs.last().unwrap().2
        } else {
            runs[0].2
        };
        for r in barcode_multiformat::linear::decode(&widths, dark, mask & 12) {
            if r.error > 0.14 {
                continue;
            }
            let (start, end) = if reverse {
                (runs.len() - r.end, runs.len() - r.start)
            } else {
                (r.start, r.end)
            };
            if r.text.len() != 8 || start >= end || end > runs.len() {
                continue;
            }
            let (left, right) = (runs[start].0 - 0.5, runs[end - 1].1 - 0.5);
            if primary
                .symbols
                .iter()
                .any(|r| overlap((r.left, r.right), (left, right)))
                || primary
                    .rejected_intervals
                    .iter()
                    .any(|&r| overlap(r, (left, right)))
            {
                continue;
            }
            let mut digits = [0; 8];
            for (i, b) in r.text.bytes().enumerate() {
                digits[i] = b - b'0';
            }
            insert(
                out,
                ShortRead {
                    digits,
                    format: if r.format == "EAN8" { 4 } else { 8 },
                    left,
                    right,
                    cost: 0.12,
                    gap: 0.,
                    recovery: true,
                },
                max_symbols,
            );
        }
    }
}
fn confirms_upce(w: &[f32], digits: &[u8; 8]) -> bool {
    let module = w.iter().sum::<f32>() / 51.;
    let mut input = Vec::with_capacity(35);
    input.push(8. * module);
    input.extend_from_slice(w);
    input.push(8. * module);
    // Exterior whites have already passed the shared reader's actual-pixel gate;
    // these padding widths isolate agreement on the symbol's observed runs.
    barcode_multiformat::linear::decode(&input, false, 8)
        .iter()
        .any(|r| {
            r.text
                .as_bytes()
                .iter()
                .zip(digits)
                .all(|(a, b)| *a == b'0' + *b)
        })
}

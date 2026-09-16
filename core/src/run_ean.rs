//! Experimental visual EAN run-width likelihood. Checksum only rejects the best
//! visual parity; it never selects a weaker valid alternative. No heap allocation.
#![forbid(unsafe_code)]
use crate::ean;
#[derive(Clone, Copy)]
struct Digit {
    value: u8,
    cost: f32,
    gap: f32,
}
const fn pattern_table(side: u8) -> [[u8; 4]; 10] {
    let mut table = [[0; 4]; 10];
    let mut d = 0;
    while d < 10 {
        table[d] = ean::digit_runs(d, side);
        d += 1;
    }
    table
}
const PATTERNS: [[[u8; 4]; 10]; 3] = [
    pattern_table(b'L'),
    pattern_table(b'G'),
    pattern_table(b'R'),
];
#[cfg(test)]
fn digit_reference(widths: &[f32], side: u8) -> Digit {
    let sum = widths.iter().sum::<f32>();
    let mut best = (f32::INFINITY, 0);
    let mut second = f32::INFINITY;
    let normalized: [f32; 4] = std::array::from_fn(|i| 7. * widths[i] / sum);
    let patterns = &PATTERNS[match side {
        b'L' => 0,
        b'G' => 1,
        _ => 2,
    }];
    for (d, patterns_entry) in patterns.iter().enumerate().take(10) {
        let pattern = *patterns_entry;
        let cost = (0..4)
            .map(|i| (normalized[i] - f32::from(pattern[i])).powi(2))
            .sum::<f32>()
            / 4.;
        if cost < best.0 {
            second = best.0;
            best = (cost, (d).to_le_bytes()[0]);
        } else {
            second = second.min(cost);
        }
    }
    Digit {
        value: best.1,
        cost: best.0,
        gap: second - best.0,
    }
}
fn digit_errors(widths: &[f32]) -> [[f32; 4]; 4] {
    let sum = widths.iter().sum::<f32>();
    let normalized: [f32; 4] = std::array::from_fn(|i| 7. * widths[i] / sum);
    std::array::from_fn(|i| {
        std::array::from_fn(|w| (normalized[i] - crate::numeric::usize_f32(w + 1)).powi(2))
    })
}
fn digit(widths: &[f32], side: u8) -> Digit {
    digit_from_errors(&digit_errors(widths), side)
}
fn digit_from_errors(errors: &[[f32; 4]; 4], side: u8) -> Digit {
    let mut best = (f32::INFINITY, 0);
    let mut second = f32::INFINITY;
    let patterns = &PATTERNS[match side {
        b'L' => 0,
        b'G' => 1,
        _ => 2,
    }];
    for (d, patterns_entry) in patterns.iter().enumerate().take(10) {
        let pattern = *patterns_entry;
        let cost = (errors[0][pattern[0] as usize - 1]
            + errors[1][pattern[1] as usize - 1]
            + errors[2][pattern[2] as usize - 1]
            + errors[3][pattern[3] as usize - 1])
            / 4.;
        if cost < best.0 {
            second = best.0;
            best = (cost, (d).to_le_bytes()[0]);
        } else {
            second = second.min(cost);
        }
    }
    Digit {
        value: best.1,
        cost: best.0,
        gap: second - best.0,
    }
}

fn digit_pair(widths: &[f32]) -> [Digit; 2] {
    let errors = digit_errors(widths);
    [
        digit_from_errors(&errors, b'L'),
        digit_from_errors(&errors, b'G'),
    ]
}
/// 59 alternating widths from the first black start-guard run to the final black
/// end-guard run. Quiet zones are verified by the caller. Fixed research gates.
#[derive(Clone, Copy, Debug)]
pub struct Evidence {
    pub digits: [u8; 13],
    pub cost: f32,
    pub gap: f32,
}
#[must_use]
pub fn decode(widths: &[f32]) -> Option<[u8; 13]> {
    decode_evidence(widths).map(|e| e.digits)
}
#[must_use]
#[cfg_attr(
    not(feature = "experimental-invalid-visual-veto"),
    expect(
        clippy::float_cmp,
        reason = "Equal decoder costs are exact ambiguity ties; approximate equality would change accepted identities."
    )
)]
pub fn decode_evidence(widths: &[f32]) -> Option<Evidence> {
    #[cfg(feature = "experimental-invalid-visual-veto")]
    {
        let e = decode_visual_evidence(widths)?;
        ean::checksum(&e.digits).then_some(e)
    }
    #[cfg(not(feature = "experimental-invalid-visual-veto"))]
    {
        if widths.len() != 59 || widths.iter().any(|x| !x.is_finite() || *x <= 0.) {
            return None;
        }
        let module = widths.iter().sum::<f32>() / 95.;
        if !module.is_finite() || module < 0.8 {
            return None;
        }
        for i in [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58] {
            if (widths[i] / module - 1.).abs() > 0.65 {
                return None;
            }
        }
        let empty = Digit {
            value: 0,
            cost: 0.,
            gap: 0.,
        };
        let mut left = [[empty; 2]; 6];
        let mut right = [empty; 6];
        for j in 0..12 {
            let start = if j < 6 { 3 + j * 4 } else { 32 + (j - 6) * 4 };
            let w = &widths[start..start + 4];
            let scale = w.iter().sum::<f32>() / 7.;
            if !(0.55 * module..=1.8 * module).contains(&scale)
                || w.iter().any(|v| !(0.4..=4.6).contains(&(v / scale)))
            {
                return None;
            }
            if j < 6 {
                left[j] = digit_pair(w);
            } else {
                right[j - 6] = digit(w, b'R');
            }
        }
        let mut best = None;
        let mut best_cost = f32::INFINITY;
        let mut tied = false;
        for first in 0u8..10 {
            let mut value = [0u8; 13];
            value[0] = first;
            let (mut cost, mut max, mut gap) = (0f32, 0f32, f32::INFINITY);
            for j in 0..12 {
                let d = if j < 6 {
                    left[j][usize::from(ean::parity_side(usize::from(first), j) == b'G')]
                } else {
                    right[j - 6]
                };
                value[j + 1] = d.value;
                cost += d.cost;
                max = max.max(d.cost);
                gap = gap.min(d.gap);
            }
            cost /= 12.;
            if cost < best_cost {
                best_cost = cost;
                best = Some((value, max, gap));
                tied = false;
            } else if cost == best_cost {
                tied = true;
            }
        }
        let (value, max, gap) = best?;
        (!tied && best_cost <= 0.12 && max <= 0.35 && gap >= 0.05 && ean::checksum(&value))
            .then_some(Evidence {
                digits: value,
                cost: best_cost,
                gap,
            })
    }
}
#[cfg(feature = "experimental-invalid-visual-veto")]
#[expect(
    clippy::float_cmp,
    reason = "Encoded samples are binary and equal decoder costs are exact ambiguity ties; epsilon matching would change accepted identities."
)]
pub(crate) fn decode_visual_evidence(widths: &[f32]) -> Option<Evidence> {
    if widths.len() != 59 || widths.iter().any(|x| !x.is_finite() || *x <= 0.) {
        return None;
    }
    let module = widths.iter().sum::<f32>() / 95.;
    if !module.is_finite() || module < 0.8 {
        return None;
    }
    for i in [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58] {
        if (widths[i] / module - 1.).abs() > 0.65 {
            return None;
        }
    }
    let empty = Digit {
        value: 0,
        cost: 0.,
        gap: 0.,
    };
    let mut left = [[empty; 2]; 6];
    let mut right = [empty; 6];
    for j in 0..12 {
        let start = if j < 6 { 3 + j * 4 } else { 32 + (j - 6) * 4 };
        let w = &widths[start..start + 4];
        let scale = w.iter().sum::<f32>() / 7.;
        if !(0.55 * module..=1.8 * module).contains(&scale)
            || w.iter().any(|v| !(0.4..=4.6).contains(&(v / scale)))
        {
            return None;
        }
        if j < 6 {
            left[j] = digit_pair(w);
        } else {
            right[j - 6] = digit(w, b'R');
        }
    }
    let mut best = None;
    let mut best_cost = f32::INFINITY;
    let mut tied = false;
    for first in 0usize..10 {
        let mut value = [0u8; 13];
        value[0] = (first).to_le_bytes()[0];
        let (mut cost, mut max, mut gap) = (0f32, 0f32, f32::INFINITY);
        for j in 0..12 {
            let d = if j < 6 {
                left[j][usize::from(ean::parity_side(first, j) == b'G')]
            } else {
                right[j - 6]
            };
            value[j + 1] = d.value;
            cost += d.cost;
            max = max.max(d.cost);
            gap = gap.min(d.gap);
        }
        cost /= 12.;
        if cost < best_cost {
            best_cost = cost;
            best = Some((value, max, gap));
            tied = false;
        } else if cost == best_cost {
            tied = true;
        }
    }
    let (value, max, gap) = best?;
    (!tied && best_cost <= 0.12 && max <= 0.35 && gap >= 0.05).then_some(Evidence {
        digits: value,
        cost: best_cost,
        gap,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[expect(
        clippy::float_cmp,
        reason = "Encoded samples are binary and equal decoder costs are exact ambiguity ties; epsilon matching would change accepted identities."
    )]
    fn runs(d: &[u8; 13]) -> Vec<f32> {
        let p = ean::encode(d);
        let mut out = Vec::new();
        let mut color = p[0];
        let mut n = 0.;
        for b in p {
            if b != color {
                out.push(n);
                n = 0.;
                color = b;
            }
            n += 3.;
        }
        out.push(n);
        out
    }
    #[test]
    fn exact_patterns_and_checksum_rejection() {
        for first in 0u8..10 {
            let mut d = [first, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 0];
            for c in 0..10 {
                d[12] = c;
                if ean::checksum(&d) {
                    break;
                }
            }
            assert_eq!(decode(&runs(&d)), Some(d));
            d[12] = (d[12] + 1) % 10;
            assert_eq!(decode(&runs(&d)), None);
        }
    }
    #[test]
    fn guard_bias_is_observed_bounded_and_never_repairs_checksum() {
        let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        for bias in [-1., 1.] {
            let r: Vec<_> = runs(&d)
                .iter()
                .enumerate()
                .map(|(i, v)| v + if i % 2 == 0 { bias } else { -bias })
                .collect();
            assert_eq!(decode_guard_bias(&r).unwrap().digits, d);
            let mut bad = d;
            bad[12] = 8;
            let r: Vec<_> = runs(&bad)
                .iter()
                .enumerate()
                .map(|(i, v)| v + if i % 2 == 0 { bias } else { -bias })
                .collect();
            assert!(decode_guard_bias(&r).is_none());
        }
        assert!(decode_guard_bias(&[]).is_none());
        assert!(decode_guard_bias(&[f32::NAN; 59]).is_none());
        assert!(decode_guard_bias(&runs(&d)).is_none());
        let mut r = runs(&d);
        for (i, v) in r.iter_mut().enumerate() {
            *v += if i % 2 == 0 { 1. } else { -1. }
        }
        r[0] += 2.;
        assert!(decode_guard_bias(&r).is_none());
    }
    #[test]
    fn validates_and_keeps_soft_width_evidence() {
        let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let mut r = runs(&d);
        r[3] += 0.9;
        r[4] -= 0.9;
        assert_eq!(decode(&r), Some(d));
        assert!(decode(&r[..58]).is_none());
        r[0] = f32::NAN;
        assert!(decode(&r).is_none());
        assert!(decode(&[1.; 59]).is_none());
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn lookup_costs_are_bitexact_to_reference() {
        for a in 1..=16 {
            for b in 1..=16 {
                for c in 1..=16 {
                    for d in 1..=16 {
                        let width = [
                            crate::numeric::f64_f32(f64::from(a)),
                            crate::numeric::f64_f32(f64::from(b)),
                            crate::numeric::f64_f32(f64::from(c)),
                            crate::numeric::f64_f32(f64::from(d)),
                        ];
                        for side in [b'L', b'G', b'R'] {
                            assert_eq!(
                                digit(&width, side).value,
                                digit_reference(&width, side).value
                            );
                            assert_eq!(
                                digit(&width, side).cost.to_bits(),
                                digit_reference(&width, side).cost.to_bits()
                            );
                            assert_eq!(
                                digit(&width, side).gap.to_bits(),
                                digit_reference(&width, side).gap.to_bits()
                            );
                        }
                    }
                }
            }
        }
        let mut seed = 11u32;
        for count in 0..2048 {
            let mut width = [0.; 4];
            for x in &mut width {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *x = 0.1 + (crate::numeric::f64_f32(f64::from(seed >> 8)) / 16_777_215.) * 7.;
            }
            for side in [b'L', b'G', b'R'] {
                let a = digit(&width, side);
                let b = digit_reference(&width, side);
                assert_eq!(
                    (a.value, a.cost.to_bits(), a.gap.to_bits()),
                    (b.value, b.cost.to_bits(), b.gap.to_bits()),
                    "noise {count} {side}"
                );
            }
        }
        for first in 0u8..10 {
            let mut e = [first, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 0];
            for c in 0..10 {
                e[12] = c;
                if ean::checksum(&e) {
                    break;
                }
            }
            let bits = ean::encode(&e);
            let mut ws = Vec::new();
            let (mut prev, mut count) = (bits[0], 0.);
            for x in bits {
                if x != prev {
                    ws.push(count);
                    count = 0.;
                    prev = x;
                }
                count += 3.;
            }
            ws.push(count);
            for j in 0..12 {
                let s = if j < 6 { 3 + j * 4 } else { 32 + (j - 6) * 4 };
                let side = if j < 6 {
                    ean::parity_side(first as usize, j)
                } else {
                    b'R'
                };
                let a = digit(&ws[s..s + 4], side);
                let b = digit_reference(&ws[s..s + 4], side);
                assert_eq!(
                    (a.value, a.cost.to_bits(), a.gap.to_bits()),
                    (b.value, b.cost.to_bits(), b.gap.to_bits())
                );
            }
        }
    }
}

/// Diagnostic observed bar/space bias from guard widths only. Never selected by
/// expected digits or checksum. Missing transitions/guards are not synthesized.
#[must_use]
pub fn decode_guard_bias(widths: &[f32]) -> Option<Evidence> {
    decode_evidence(&guard_bias_widths(widths)?)
}
pub(crate) fn guard_bias_widths(widths: &[f32]) -> Option<[f32; 59]> {
    guard_bias_widths_global(widths).or_else(|| guard_bias_widths_spatial(widths))
}
fn guard_bias_widths_global(widths: &[f32]) -> Option<[f32; 59]> {
    if widths.len() != 59 || widths.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return None;
    }
    let module = widths.iter().sum::<f32>() / 95.;
    let mut black = [
        widths[0], widths[2], widths[28], widths[30], widths[56], widths[58],
    ];
    let mut white = [widths[1], widths[27], widths[29], widths[31], widths[57]];
    black.sort_by(f32::total_cmp);
    white.sort_by(f32::total_cmp);
    let b = (black[2] + black[3]) * 0.5;
    let w = white[2];
    let bias = (b - w) * 0.5;
    let pitch = (b + w) * 0.5;
    if !module.is_finite()
        || module < 0.8
        || bias.abs() < 0.04 * module
        || bias.abs() > 0.4 * module
    {
        return None;
    }
    if !(0.7 * module..=1.3 * module).contains(&pitch) {
        return None;
    }
    if black
        .iter()
        .any(|v| (v - bias - pitch).abs() > 0.35 * module)
        || white
            .iter()
            .any(|v| (v + bias - pitch).abs() > 0.35 * module)
    {
        return None;
    }
    let corrected: [f32; 59] =
        std::array::from_fn(|i| widths[i] - if i % 2 == 0 { bias } else { -bias });
    Some(corrected)
}

/// Diagnostic visual rankings before checksum rejection. Never used to select
/// an alternative checksum-valid value in the runtime decoder.
#[derive(Clone, Debug)]
pub struct DiagnosticParity {
    pub digits: [u8; 13],
    pub cost: f32,
    pub maximum_digit_cost: f32,
    pub minimum_digit_gap: f32,
    pub digit_costs: [f32; 12],
    pub checksum_valid: bool,
}
#[must_use]
pub fn diagnostic_parities(widths: &[f32]) -> Option<Vec<DiagnosticParity>> {
    if widths.len() != 59 || widths.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return None;
    }
    let mut result = Vec::with_capacity(10);
    for first in 0usize..10 {
        let mut digits = [0; 13];
        digits[0] = (first).to_le_bytes()[0];
        let mut costs = [0.; 12];
        let mut maximum = 0f32;
        let mut gap = f32::INFINITY;
        for j in 0..12 {
            let start = if j < 6 { 3 + j * 4 } else { 32 + (j - 6) * 4 };
            let side = if j < 6 {
                ean::parity_side(first, j)
            } else {
                b'R'
            };
            let d = digit(&widths[start..start + 4], side);
            digits[j + 1] = d.value;
            costs[j] = d.cost;
            maximum = maximum.max(d.cost);
            gap = gap.min(d.gap);
        }
        result.push(DiagnosticParity {
            digits,
            cost: costs.iter().sum::<f32>() / 12.,
            maximum_digit_cost: maximum,
            minimum_digit_gap: gap,
            digit_costs: costs,
            checksum_valid: ean::checksum(&digits),
        });
    }
    result.sort_by(|a, b| a.cost.total_cmp(&b.cost));
    Some(result)
}
#[cfg(test)]
mod parity_diagnostic_tests {
    use super::*;
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn rankings_reproduce_best_visual_evidence_without_checksum_repair() {
        for first in 0u8..10 {
            let mut d = [first, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 0];
            for c in 0..10 {
                d[12] = c;
                if ean::checksum(&d) {
                    break;
                }
            }
            for bad in [false, true] {
                let mut value = d;
                if bad {
                    value[12] = (value[12] + 1) % 10;
                }
                let bits = ean::encode(&value);
                let mut widths = Vec::new();
                let mut previous = bits[0];
                let mut n = 0.;
                for b in bits {
                    if b != previous {
                        widths.push(n);
                        n = 0.;
                        previous = b;
                    }
                    n += 3.;
                }
                widths.push(n);
                let r = diagnostic_parities(&widths).unwrap();
                assert_eq!(r.len(), 10);
                assert_eq!(r[0].digits, value);
                assert_eq!(r[0].checksum_valid, !bad);
                assert_eq!(r[0].cost, 0.);
                assert_eq!(decode(&widths), (!bad).then_some(value));
            }
        }
        assert!(diagnostic_parities(&[]).is_none());
        assert!(diagnostic_parities(&[f32::NAN; 59]).is_none());
    }
}

/// Estimate slowly varying bar/space growth from the three observed guard groups.
/// No payload hypotheses determine correction, positions, or interpolation.
fn guard_bias_widths_spatial(widths: &[f32]) -> Option<[f32; 59]> {
    if widths.len() != 59 || widths.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return None;
    }
    let total = widths.iter().sum::<f32>();
    let module = total / 95.;
    if module < 0.8 {
        return None;
    }
    let groups: [&[usize]; 3] = [&[0, 1, 2], &[27, 28, 29, 30, 31], &[56, 57, 58]];
    let mut bias = [0.; 3];
    let mut pitch = [0.; 3];
    let mut anchors = [0.; 3];
    let mut centers = [0.; 59];
    let mut offset = 0.;
    for (i, &w) in widths.iter().enumerate() {
        centers[i] = offset + w * 0.5;
        offset += w;
    }
    for (k, ids) in groups.iter().enumerate() {
        let (mut black, mut white, mut nb, mut nw) = (0., 0., 0., 0.);
        for &i in *ids {
            if i % 2 == 0 {
                black += widths[i];
                nb += 1.;
            } else {
                white += widths[i];
                nw += 1.;
            }
        }
        black /= nb;
        white /= nw;
        bias[k] = (black - white) * 0.5;
        pitch[k] = (black + white) * 0.5;
        if !(0.55 * module..=1.8 * module).contains(&pitch[k]) || bias[k].abs() > 0.4 * pitch[k] {
            return None;
        }
        for &i in *ids {
            let corrected = widths[i] - if i % 2 == 0 { bias[k] } else { -bias[k] };
            if (corrected - pitch[k]).abs() > 0.35 * pitch[k] {
                return None;
            }
        }
        anchors[k] =
            ids.iter().map(|&i| centers[i]).sum::<f32>() / crate::numeric::usize_f32(ids.len());
    }
    if bias.iter().zip(pitch).all(|(&b, p)| b.abs() < 0.04 * p) {
        return None;
    }
    let corrected = std::array::from_fn(|i| {
        let k = usize::from(centers[i] > anchors[1]);
        let t = ((centers[i] - anchors[k]) / (anchors[k + 1] - anchors[k])).clamp(0., 1.);
        let b = bias[k] + t * (bias[k + 1] - bias[k]);
        widths[i] - if i % 2 == 0 { b } else { -b }
    });
    Some(corrected)
}

#[cfg(test)]
mod spatial_guard_tests {
    use super::*;
    #[expect(
        clippy::float_cmp,
        reason = "Encoded samples are binary and equal decoder costs are exact ambiguity ties; epsilon matching would change accepted identities."
    )]
    fn observed(d: [u8; 13], v_shape: bool) -> Vec<f32> {
        let bits = ean::encode(&d);
        let mut widths = Vec::new();
        let mut previous = bits[0];
        let mut n = 0.;
        for b in bits {
            if b != previous {
                widths.push(n);
                n = 0.;
                previous = b;
            }
            n += 3.;
        }
        widths.push(n);
        let total = widths.iter().sum::<f32>();
        let mut offset = 0.;
        for (i, w) in widths.iter_mut().enumerate() {
            let t = (offset + *w * 0.5) / total;
            offset += *w;
            let edge_offset = if v_shape {
                1.05 * (4. * (t - 0.5).abs() - 1.)
            } else {
                1.05 * (1. - 2. * t)
            };
            *w += if i % 2 == 0 {
                edge_offset
            } else {
                -edge_offset
            };
        }
        widths
    }
    #[test]
    fn guard_observations_correct_varying_growth_without_checksum_repair() {
        let good = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let mut bad = good;
        bad[12] = 8;
        for v in [false, true] {
            let widths = observed(good, v);
            assert!(guard_bias_widths_spatial(&widths).is_some());
            assert_eq!(decode_guard_bias(&widths).unwrap().digits, good);
            assert!(decode_guard_bias(&observed(bad, v)).is_none());
        }
        assert!(guard_bias_widths_spatial(&[1.; 59]).is_none());
        assert!(guard_bias_widths_spatial(&[f32::NAN; 59]).is_none());
        let mut broken = observed(good, false);
        broken[1] *= 4.;
        assert!(guard_bias_widths_spatial(&broken).is_none());
    }
}

#[cfg(test)]
mod pair_reuse_tests {
    use super::*;
    #[test]
    fn shared_errors_match_independent_sides_bit_for_bit() {
        let mut seed = 27u32;
        for _ in 0..20000 {
            let w: [f32; 4] = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                0.1 + crate::numeric::f64_f32(f64::from(seed >> 8)) / 838_860.8
            });
            let got = digit_pair(&w);
            for (i, side) in [b'L', b'G'].into_iter().enumerate() {
                let want = digit_reference(&w, side);
                assert_eq!(
                    (got[i].value, got[i].cost.to_bits(), got[i].gap.to_bits()),
                    (want.value, want.cost.to_bits(), want.gap.to_bits())
                );
            }
        }
    }
}

//! EAN-13 structure from GS1 General Specifications 5.2.1 (reference in README).
//! Research decoder: least-squares module evidence with parity/checksum rejection.
//! It deliberately does not force checksum repair or guarantee calibrated scores.
const L: [&[u8; 7]; 10] = [
    b"0001101", b"0011001", b"0010011", b"0111101", b"0100011", b"0110001", b"0101111", b"0111011",
    b"0110111", b"0001011",
];
const PARITY: [&[u8; 6]; 10] = [
    b"LLLLLL", b"LLGLGG", b"LLGGLG", b"LLGGGL", b"LGLLGG", b"LGGLLG", b"LGGGLL", b"LGLGLG",
    b"LGLGGL", b"LGGLGL",
];
const fn bit(d: usize, side: u8, i: usize) -> f32 {
    let b = if side == b'G' {
        L[d][6 - i] - b'0'
    } else {
        L[d][i] - b'0'
    };
    if side == b'L' {
        b as f32
    } else {
        (1 - b) as f32
    }
}
pub(crate) fn parity_side(first: usize, position: usize) -> u8 {
    PARITY[first][position]
}
#[expect(
    clippy::float_cmp,
    reason = "Encoded samples are binary and equal decoder costs are exact ambiguity ties; epsilon matching would change accepted identities."
)]
pub(crate) const fn digit_runs(d: usize, side: u8) -> [u8; 4] {
    let mut runs = [0; 4];
    let mut at = 0;
    let mut previous = bit(d, side, 0);
    let mut i = 0;
    while i < 7 {
        let b = bit(d, side, i);
        if b != previous {
            at += 1;
            previous = b;
        }
        runs[at] += 1;
        i += 1;
    }
    runs
}
#[must_use]
pub fn checksum(d: &[u8; 13]) -> bool {
    d.iter()
        .enumerate()
        .map(|(i, v)| *v as usize * if i % 2 == 0 { 1 } else { 3 })
        .sum::<usize>()
        % 10
        == 0
}
#[must_use]
pub fn encode(d: &[u8; 13]) -> [f32; 95] {
    let mut p = [0.; 95];
    for i in [0, 2, 46, 48, 92, 94] {
        p[i] = 1.;
    }
    for j in 0..12 {
        let start = if j < 6 { 3 + j * 7 } else { 50 + (j - 6) * 7 };
        let side = if j < 6 {
            PARITY[d[0] as usize][j]
        } else {
            b'R'
        };
        for i in 0..7 {
            p[start + i] = bit(d[j + 1] as usize, side, i);
        }
    }
    p
}
#[derive(Clone, Copy, Debug)]
pub struct Result {
    pub digits: [u8; 13],
    pub cost: f32,
    pub gap: f32,
    pub guard: f32,
}
#[derive(Clone, Copy)]
struct Digit {
    value: u8,
    cost: f32,
    gap: f32,
}
const fn bit_patterns() -> [[[usize; 7]; 10]; 3] {
    let mut a = [[[0; 7]; 10]; 3];
    let sides = [b'L', b'G', b'R'];
    let mut s = 0;
    while s < 3 {
        let mut d = 0;
        while d < 10 {
            let mut i = 0;
            while i < 7 {
                a[s][d][i] = crate::numeric::f32_usize(bit(d, sides[s], i));
                i += 1;
            }
            d += 1;
        }
        s += 1;
    }
    a
}
const BIT_PATTERNS: [[[usize; 7]; 10]; 3] = bit_patterns();
fn errors(p: &[f32]) -> [[f32; 2]; 7] {
    std::array::from_fn(|i| {
        let zero = p[i] - 0.;
        let one = p[i] - 1.;
        [zero * zero, one * one]
    })
}
fn digit_from_errors(e: &[[f32; 2]; 7], side: usize) -> Digit {
    let (mut best, mut second) = ((f32::INFINITY, 0u8), f32::INFINITY);
    for (d, pat) in BIT_PATTERNS[side].iter().enumerate() {
        let c = (0..7).map(|i| e[i][pat[i]]).sum::<f32>() / 7.;
        if c < best.0 {
            second = best.0;
            best = (c, (d).to_le_bytes()[0]);
        } else if c < second {
            second = c;
        }
    }
    Digit {
        value: best.1,
        cost: best.0,
        gap: second - best.0,
    }
}
fn digit(p: &[f32], side: u8) -> Digit {
    digit_from_errors(
        &errors(p),
        match side {
            b'L' => 0,
            b'G' => 1,
            _ => 2,
        },
    )
}
fn digit_pair(p: &[f32]) -> [Digit; 2] {
    let e = errors(p);
    [digit_from_errors(&e, 0), digit_from_errors(&e, 1)]
}
#[cfg(test)]
fn digit_reference(p: &[f32], side: u8) -> Digit {
    let mut smallest = (f32::INFINITY, 0u8);
    let mut second = f32::INFINITY;
    for d in 0..10 {
        let c = (0..7)
            .map(|i| {
                let z = p[i] - bit(d, side, i);
                z * z
            })
            .sum::<f32>()
            / 7.;
        if c < smallest.0 {
            second = smallest.0;
            smallest = (c, (d).to_le_bytes()[0]);
        } else if c < second {
            second = c;
        }
    }
    Digit {
        value: smallest.1,
        cost: smallest.0,
        gap: second - smallest.0,
    }
}
/// Select the strongest visual parity hypothesis BEFORE checking checksum.
/// Checksum rejects; it must not promote a weaker visual leading digit.
/// Digit evidence is computed once per position/alphabet and reused by parity.
/// Cost/gap are research evidence, not calibrated confidence.
#[must_use]
#[expect(
    clippy::float_cmp,
    reason = "Encoded samples are binary and equal decoder costs are exact ambiguity ties; epsilon matching would change accepted identities."
)]
pub fn decode(p: &[f32], max_cost: f32, min_gap: f32) -> Option<Result> {
    if p.len() != 95
        || p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        || !max_cost.is_finite()
        || !min_gap.is_finite()
        || max_cost < 0.
        || min_gap < 0.
    {
        return None;
    }
    let mut guard = 0.;
    for (i, b) in [
        (0, 1.),
        (1, 0.),
        (2, 1.),
        (45, 0.),
        (46, 1.),
        (47, 0.),
        (48, 1.),
        (49, 0.),
        (92, 1.),
        (93, 0.),
        (94, 1.),
    ] {
        guard += (p[i] - b) * (p[i] - b);
    }
    guard /= 11.;
    if guard > max_cost {
        return None;
    }
    let empty = Digit {
        value: 0,
        cost: 0.,
        gap: 0.,
    };
    let mut left = [[empty; 2]; 6];
    let mut right = [empty; 6];
    for j in 0..6 {
        left[j] = digit_pair(&p[3 + j * 7..10 + j * 7]);
        right[j] = digit(&p[50 + j * 7..57 + j * 7], b'R');
    }
    let mut best: Option<Result> = None;
    let mut tied = false;
    for first in 0usize..10 {
        let mut digits = [0u8; 13];
        digits[0] = (first).to_le_bytes()[0];
        let mut cost = 0.;
        let mut gap = f32::INFINITY;
        for j in 0..12 {
            let d = if j < 6 {
                left[j][usize::from(PARITY[first][j] == b'G')]
            } else {
                right[j - 6]
            };
            digits[j + 1] = d.value;
            cost += d.cost;
            gap = gap.min(d.gap);
        }
        cost /= 12.;
        if best.is_none_or(|b| cost < b.cost) {
            best = Some(Result {
                digits,
                cost,
                gap,
                guard,
            });
            tied = false;
        } else if best.is_some_and(|b| cost == b.cost) {
            tied = true;
        }
    }
    best.filter(|r| !tied && r.cost <= max_cost && r.gap >= min_gap && checksum(&r.digits))
}
#[cfg(test)]
fn legacy_decode(p: &[f32], max_cost: f32, min_gap: f32) -> Option<Result> {
    if p.len() != 95 || p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return None;
    }
    let mut guard = 0.;
    for (i, b) in [
        (0, 1.),
        (1, 0.),
        (2, 1.),
        (45, 0.),
        (46, 1.),
        (47, 0.),
        (48, 1.),
        (49, 0.),
        (92, 1.),
        (93, 0.),
        (94, 1.),
    ] {
        guard += (p[i] - b) * (p[i] - b);
    }
    guard /= 11.;
    if guard > max_cost {
        return None;
    }
    let mut best: Option<Result> = None;
    for (first, parity_entry) in PARITY.iter().enumerate() {
        let mut digits = [0u8; 13];
        digits[0] = (first).to_le_bytes()[0];
        let mut cost = 0.;
        let mut gap = f32::INFINITY;
        for j in 0..12 {
            let side = if j < 6 { (*parity_entry)[j] } else { b'R' };
            let start = if j < 6 { 3 + j * 7 } else { 50 + (j - 6) * 7 };
            let mut smallest = (f32::INFINITY, 0usize);
            let mut second = f32::INFINITY;
            for d in 0..10 {
                let c = (0..7)
                    .map(|i| {
                        let z = p[start + i] - bit(d, side, i);
                        z * z
                    })
                    .sum::<f32>()
                    / 7.;
                if c < smallest.0 {
                    second = smallest.0;
                    smallest = (c, d);
                } else if c < second {
                    second = c;
                }
            }
            digits[j + 1] = (smallest.1).to_le_bytes()[0];
            cost += smallest.0;
            gap = gap.min(second - smallest.0);
        }
        cost /= 12.;
        if checksum(&digits)
            && cost <= max_cost
            && gap >= min_gap
            && best.is_none_or(|b| cost < b.cost)
        {
            best = Some(Result {
                digits,
                cost,
                gap,
                guard,
            });
        }
    }
    best
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encodings_roundtrip_all_leading_digits() {
        for first in 0u8..10 {
            let mut d = [0u8; 13];
            d[0] = first;
            for (i, d_entry) in d.iter_mut().enumerate().take(12).skip(1) {
                (*d_entry) = ((i * 7) % 10).to_le_bytes()[0];
            }
            let sum = d[..12]
                .iter()
                .enumerate()
                .map(|(i, v)| *v as usize * if i % 2 == 0 { 1 } else { 3 })
                .sum::<usize>();
            d[12] = ((10 - sum % 10) % 10).to_le_bytes()[0];
            assert_eq!(decode(&encode(&d), 0.1, 0.01).unwrap().digits, d);
        }
    }
    #[test]
    fn rejects_blank_and_ambiguous() {
        for v in [0., 0.5, 1.] {
            assert!(decode(&[v; 95], 0.1, 0.01).is_none());
        }
    }
    #[test]
    fn damaged_checksum_is_not_repaired() {
        let d = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 8];
        assert!(!checksum(&d));
        assert!(decode(&encode(&d), 0.01, 0.01).is_none());
    }

    #[test]
    fn checksum_cannot_select_a_weaker_visual_parity() {
        // Deterministic noisy invalid-checksum fixture. The old decoder selected
        // a worse parity/digit combination solely because its checksum passed.
        let mut p = encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 8]);
        let mut state = 23u32;
        for v in &mut p {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = crate::numeric::f64_f32(f64::from(state >> 24)) / 255. * 0.4;
            *v = if *v == 0. { noise } else { 1. - noise };
        }
        assert_eq!(
            legacy_decode(&p, 0.1, 0.02).unwrap().digits,
            [9, 9, 0, 1, 2, 5, 3, 1, 2, 3, 4, 5, 8]
        );
        assert!(decode(&p, 0.1, 0.02).is_none());
    }
}

#[path = "ean_blur.rs"]
mod blurred;
pub use blurred::{blurred_guard_possible, decode_blurred, BlurredResult, BLUR_SIGMAS};

#[cfg(test)]
mod module_error_reuse_tests {
    use super::*;
    #[test]
    fn all_binary_patterns_and_noisy_values_preserve_cost_bits() {
        let check = |p: [f32; 7]| {
            for side in [b'L', b'G', b'R'] {
                let a = digit(&p, side);
                let b = digit_reference(&p, side);
                assert_eq!(
                    (a.value, a.cost.to_bits(), a.gap.to_bits()),
                    (b.value, b.cost.to_bits(), b.gap.to_bits())
                );
            }
            let pair = digit_pair(&p);
            for (i, side) in [b'L', b'G'].into_iter().enumerate() {
                let b = digit_reference(&p, side);
                let a = pair[i];
                assert_eq!(
                    (a.value, a.cost.to_bits(), a.gap.to_bits()),
                    (b.value, b.cost.to_bits(), b.gap.to_bits())
                );
            }
        };
        for bits in 0..128 {
            check(std::array::from_fn(|i| {
                crate::numeric::f64_f32(f64::from((bits >> i) & 1))
            }));
        }
        check([-0., 0., 1., 0.5, 0.25, 0.75, -0.]);
        let mut seed = 51u32;
        for _ in 0..20000 {
            check(std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                crate::numeric::f64_f32(f64::from(seed >> 8)) / 16_777_215.
            }));
        }
    }
}

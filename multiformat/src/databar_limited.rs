//! Independent GS1 `DataBar` Limited decoder (ISO/IEC 24724).
//!
//! A symbol is a one-module bar guard, a left data character, a check character, a
//! right data character and a space/bar guard pair: 73 modules from the first bar.
//! Data characters are seven odd and seven even elements (26 modules) whose width
//! ranks give a value below 2,013,571; the GTIN-13 value without its check digit is
//! `left * 2013571 + right`, so its indicator digit is 0 or 1. The check character
//! (18 modules) is one of 89 patterns selected by the weighted width sum mod 89.
//!
//! The group parameters and weights are the standard's; the 89 check patterns were
//! read from Zint 2.16 output (BSD-licensed encoder; 1,505 generated symbols gave
//! each check sum one pattern). The tests decode one Zint symbol per character
//! group. Width ranking and decoding here are project-owned.
use crate::linear::{Read, pattern_error};

/// (value base, even-value count, odd modules, odd widest, even modules, even widest)
/// for the seven character groups.
const GROUPS: [(u64, u64, usize, u8, usize, u8); 7] = [
    (0, 28, 17, 6, 9, 3),
    (183_064, 728, 13, 5, 13, 4),
    (820_064, 6454, 9, 3, 17, 6),
    (1_000_776, 203, 15, 5, 11, 4),
    (1_491_021, 2408, 11, 4, 15, 5),
    (1_979_845, 1, 19, 8, 7, 1),
    (1_996_939, 16632, 7, 1, 19, 8),
];
const CHARACTER_VALUES: u64 = 2_013_571;
/// Check-sum weights of the 28 data-character elements: powers of 3 mod 89.
const WEIGHTS: [usize; 28] = {
    let mut w = [1; 28];
    let mut i = 1;
    while i < 28 {
        w[i] = w[i - 1] * 3 % 89;
        i += 1;
    }
    w
};
/// Check character widths for check sums 0-88, one element per nibble, first element highest.
const CHECKS: [u64; 89] = [
    0x11_1111_1111_3311,
    0x11_1111_1112_3211,
    0x11_1111_1113_3111,
    0x11_1111_1211_3211,
    0x11_1111_1212_3111,
    0x11_1111_1311_3111,
    0x11_1112_1111_3211,
    0x11_1112_1112_3111,
    0x11_1112_1211_3111,
    0x11_1113_1111_3111,
    0x11_1211_1111_3211,
    0x11_1211_1112_3111,
    0x11_1211_1211_3111,
    0x11_1212_1111_3111,
    0x11_1311_1111_3111,
    0x12_1111_1111_3211,
    0x12_1111_1112_3111,
    0x12_1111_1211_3111,
    0x12_1112_1111_3111,
    0x12_1211_1111_3111,
    0x13_1111_1111_3111,
    0x11_1111_1121_2311,
    0x11_1111_1122_2211,
    0x11_1111_1123_2111,
    0x11_1111_1221_2211,
    0x11_1111_1222_2111,
    0x11_1111_1321_2111,
    0x11_1112_1121_2211,
    0x11_1112_1122_2111,
    0x11_1112_1221_2111,
    0x11_1113_1121_2111,
    0x11_1211_1121_2211,
    0x11_1211_1122_2111,
    0x11_1211_1221_2111,
    0x11_1212_1121_2111,
    0x11_1311_1121_2111,
    0x12_1111_1121_2211,
    0x12_1111_1122_2111,
    0x12_1111_1221_2111,
    0x12_1112_1121_2111,
    0x12_1211_1121_2111,
    0x13_1111_1121_2111,
    0x11_1111_1131_1311,
    0x11_1111_1132_1211,
    0x11_1111_1231_1211,
    0x11_1211_1131_1211,
    0x12_1111_1131_1211,
    0x11_1111_2111_2311,
    0x11_1111_2112_2211,
    0x11_1111_2113_2111,
    0x11_1111_2211_2211,
    0x11_1211_2111_2211,
    0x11_1211_2112_2111,
    0x11_1211_2211_2111,
    0x11_1212_2111_2111,
    0x11_1311_2111_2111,
    0x12_1111_2111_2211,
    0x12_1111_2112_2111,
    0x12_1211_2111_2111,
    0x11_1121_1111_2311,
    0x11_1121_1112_2211,
    0x11_1121_1113_2111,
    0x11_1121_1211_2211,
    0x11_1121_1212_2111,
    0x11_1122_1111_2211,
    0x12_1121_1111_2211,
    0x12_1121_1112_2111,
    0x12_1121_1211_2111,
    0x12_1122_1111_2111,
    0x12_1221_1111_2111,
    0x13_1121_1111_2111,
    0x11_2111_1111_2311,
    0x11_2111_1112_2211,
    0x11_2111_1113_2111,
    0x11_2111_1211_2211,
    0x11_2111_1212_2111,
    0x11_2111_1311_2111,
    0x11_2112_1111_2211,
    0x11_2112_1112_2111,
    0x11_2211_1111_2211,
    0x21_1111_1112_2211,
    0x21_1111_1113_2111,
    0x21_1111_1211_2211,
    0x21_1111_1212_2111,
    0x21_1111_1311_2111,
    0x21_1112_1112_2111,
    0x21_1112_1211_2111,
    0x21_1211_1112_2111,
    0x21_1111_1122_1211,
];

fn nibbles(packed: u64) -> [u8; 14] {
    std::array::from_fn(|i| u8::try_from((packed >> (4 * (13 - i))) & 15).unwrap_or(0))
}

fn combinations(n: usize, r: usize) -> u64 {
    if r > n {
        return 0;
    }
    let r = r.min(n - r);
    (0..r).fold(1_u64, |acc, i| acc * (n - i) as u64 / (i + 1) as u64)
}

/// Rank of `widths` among the combinations of its element count and module sum
/// whose elements are at most `widest` (ISO/IEC 24724 width enumeration). Without
/// `no_narrow`, only combinations with a one-module element are counted; the
/// caller checks that `widths` has one.
fn width_rank(widths: [u8; 7], widest: u8, no_narrow: bool) -> u64 {
    let elements = widths.len();
    let widest = usize::from(widest);
    let mut n: usize = widths.iter().map(|&w| usize::from(w)).sum();
    let mut value = 0_u64;
    let mut narrow_mask = 0_u32;
    for (bar, &target) in widths.iter().enumerate().take(elements - 1) {
        let mut width = 1;
        narrow_mask |= 1 << bar;
        while width < usize::from(target) {
            let mut sub = combinations(n - width - 1, elements - bar - 2);
            if !no_narrow
                && narrow_mask == 0
                && n - width - (elements - bar - 1) >= elements - bar - 1
            {
                sub -= combinations(n - width - (elements - bar), elements - bar - 2);
            }
            if elements - bar - 1 > 1 {
                let mut less = 0;
                let mut largest = n - width - (elements - bar - 2);
                while largest > widest {
                    less += combinations(n - width - largest - 1, elements - bar - 3);
                    largest -= 1;
                }
                sub -= less * (elements - 1 - bar) as u64;
            } else if n - width > widest {
                sub -= 1;
            }
            value += sub;
            width += 1;
            narrow_mask &= !(1 << bar);
        }
        n -= width;
    }
    value
}

/// Integer element widths summing to `units` from measured runs, adjusting the
/// elements furthest from their rounded width.
fn widths<const N: usize>(r: &[f32], units: usize, widest: u8) -> Option<[u8; N]> {
    let module = r[..N].iter().sum::<f32>() / crate::numeric::usize_f32(units);
    if module < crate::linear::MIN_MODULE {
        return None;
    }
    let mut out: [u8; N] = std::array::from_fn(|i| {
        crate::numeric::f32_u8((r[i] / module).round().clamp(1., f32::from(widest)))
    });
    let sum = |w: &[u8; N]| w.iter().map(|&v| usize::from(v)).sum::<usize>();
    if sum(&out).abs_diff(units) > 2 {
        return None;
    }
    while sum(&out) != units {
        let add = sum(&out) < units;
        let i = (0..N)
            .filter(|&i| if add { out[i] < widest } else { out[i] > 1 })
            .max_by(|&a, &b| {
                let e = |i: usize| {
                    let d = r[i] / module - f32::from(out[i]);
                    if add { d } else { -d }
                };
                e(a).total_cmp(&e(b))
            })?;
        if add {
            out[i] += 1;
        } else {
            out[i] -= 1;
        }
    }
    Some(out)
}

/// Value, widths and fit error of the 14-element data character starting at `r[0]`.
fn character(r: &[f32]) -> Option<(u64, [u8; 14], f32)> {
    let w: [u8; 14] = widths(r, 26, 8)?;
    let odd: [u8; 7] = std::array::from_fn(|i| w[2 * i]);
    let even: [u8; 7] = std::array::from_fn(|i| w[2 * i + 1]);
    let odd_sum: usize = odd.iter().map(|&v| usize::from(v)).sum();
    let &(base, even_count, _, odd_widest, _, even_widest) = GROUPS
        .iter()
        .find(|g| g.2 == odd_sum && g.4 == 26 - odd_sum)?;
    // Even elements need a one-module element; without it a combination would
    // alias the rank of a valid one.
    if odd.iter().any(|&v| v > odd_widest)
        || even.iter().any(|&v| v > even_widest)
        || !even.contains(&1)
    {
        return None;
    }
    let odd_value = width_rank(odd, odd_widest, true);
    let even_value = width_rank(even, even_widest, false);
    if even_value >= even_count {
        return None;
    }
    let value = base + odd_value * even_count + even_value;
    let end = GROUPS
        .iter()
        .find(|g| g.0 > base)
        .map_or(CHARACTER_VALUES, |g| g.0);
    if value >= end {
        return None;
    }
    let error = pattern_error(r, &w);
    (error < 0.22).then_some((value, w, error))
}

/// `DataBar` Limited runs starting at the guard bar `r[s]`.
#[must_use]
pub fn decode(r: &[f32], s: usize) -> Option<Read> {
    if s + 45 > r.len() {
        return None;
    }
    // The check character ends with two unit elements, followed by the right
    // data character; the end guard is a unit space and bar. Those five unit
    // elements must agree before the symbol width is summed.
    let units = [r[s], r[s + 27], r[s + 28], r[s + 43], r[s + 44]];
    let (low, high) = units
        .iter()
        .fold((f32::INFINITY, 0_f32), |(l, h), &u| (l.min(u), h.max(u)));
    // Every unit element lies within 0.35 and 1.65 modules (checked below).
    if high > low * 4.75 {
        return None;
    }
    let module = r[s..s + 45].iter().sum::<f32>() / 73.;
    let unit = |v: f32| (v - module).abs() <= module * 0.65;
    if !unit(r[s]) || !unit(r[s + 43]) || !unit(r[s + 44]) || !unit(r[s + 27]) || !unit(r[s + 28]) {
        return None;
    }
    let check: [u8; 14] = widths(&r[s + 15..], 18, 3)?;
    let packed = check.iter().fold(0_u64, |v, &w| (v << 4) | u64::from(w));
    let check_value = CHECKS.iter().position(|&c| c == packed)?;
    let check_error = pattern_error(&r[s + 15..], &nibbles(CHECKS[check_value]));
    if check_error >= 0.22 {
        return None;
    }
    let left = character(&r[s + 1..])?;
    let right = character(&r[s + 29..])?;
    let sum = left
        .1
        .iter()
        .chain(&right.1)
        .zip(WEIGHTS)
        .map(|(&w, weight)| usize::from(w) * weight)
        .sum::<usize>()
        % 89;
    if sum != check_value {
        return None;
    }
    // The three characters share one module width.
    let modules = [
        r[s + 1..s + 15].iter().sum::<f32>() / 26.,
        r[s + 15..s + 29].iter().sum::<f32>() / 18.,
        r[s + 29..s + 43].iter().sum::<f32>() / 26.,
    ];
    let (low, high) = modules
        .iter()
        .fold((f32::INFINITY, 0_f32), |(l, h), &m| (l.min(m), h.max(m)));
    if high > low * 1.3 {
        return None;
    }
    let value = left.0 * CHARACTER_VALUES + right.0;
    if value >= 2_000_000_000_000 {
        return None;
    }
    let digits = format!("{value:013}");
    let check = crate::databar::gtin_check(&digits);
    Some(Read {
        decoded: true,
        addon: None,
        format: "DataBarLimited",
        text: format!("01{digits}{check}"),
        start: s,
        end: s + 45,
        error: (left.2 + right.2 + check_error) / 3.,
        codabar_start_stop: None,
        gs1: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zint 2.16 `-b 30` symbols: the 13 digits and the element widths from the
    /// first bar, one per character group of the right data character.
    const SYMBOLS: [(&str, &str); 7] = [
        (
            "0000000091532",
            "111111111116163111111211123112131113121511311",
        ),
        (
            "0368622930963",
            "111111111133452121111121131112211111332312311",
        ),
        (
            "1999476913420",
            "124211212121115121111113112111111311111161611",
        ),
        (
            "0000001245898",
            "111111111116163111111123112112123111111424211",
        ),
        (
            "0368624164832",
            "111111111133452111211211122111231151312212111",
        ),
        (
            "1999477991392",
            "124211212121115111311211121112141312111116111",
        ),
        (
            "0000002005255",
            "111111111116163111111211123111214131113111511",
        ),
    ];

    fn runs(widths: &str) -> Vec<u8> {
        widths.bytes().map(|b| b - b'0').collect()
    }

    fn read(widths: &[u8], scale: f32) -> Option<Read> {
        let mut r = vec![12.];
        r.extend(widths.iter().map(|&w| f32::from(w) * scale));
        r.push(12.);
        decode(&r, 1)
    }

    #[test]
    fn reads_zint_symbols_of_every_group() {
        for (digits, widths) in SYMBOLS {
            let widths = runs(widths);
            assert_eq!(widths.len(), 45);
            for scale in [1., 2.5, 3.] {
                let found = read(&widths, scale).expect(digits);
                let check = crate::databar::gtin_check(digits);
                assert_eq!(found.text, format!("01{digits}{check}"));
                assert_eq!(found.format, "DataBarLimited");
            }
        }
    }

    #[test]
    fn rejects_damaged_check_sums() {
        for (_, widths) in SYMBOLS {
            let widths = runs(widths);
            for i in 1..43 {
                // Moving a module between neighbours inside a character keeps the sums.
                if widths[i] < 2 || i == 14 || i == 28 {
                    continue;
                }
                let mut damaged = widths.clone();
                damaged[i] -= 1;
                damaged[i + 1] += 1;
                if let Some(found) = read(&damaged, 2.) {
                    panic!("element {i} read as {}", found.text);
                }
            }
        }
    }

    #[test]
    fn check_patterns_are_unique_and_eighteen_modules() {
        for (i, &a) in CHECKS.iter().enumerate() {
            assert_eq!(
                nibbles(a).iter().map(|&w| usize::from(w)).sum::<usize>(),
                18
            );
            assert!(CHECKS[i + 1..].iter().all(|&b| b != a));
        }
    }

    /// Ranks of every composition of `modules` into seven elements of at most
    /// `widest` modules; without `no_narrow`, only those with a one-module element.
    fn ranks(modules: usize, widest: u8, no_narrow: bool) -> Vec<u64> {
        let mut out = Vec::new();
        let mut w = [1_u8; 7];
        loop {
            if w.iter().map(|&v| usize::from(v)).sum::<usize>() == modules
                && (no_narrow || w.contains(&1))
            {
                out.push(width_rank(w, widest, no_narrow));
            }
            let mut i = 0;
            while i < 7 {
                w[i] += 1;
                if w[i] <= widest {
                    break;
                }
                w[i] = 1;
                i += 1;
            }
            if i == 7 {
                break;
            }
        }
        out.sort_unstable();
        out
    }

    #[test]
    fn width_ranks_are_exact_in_every_group() {
        for (k, &(base, even_count, odd_modules, odd_widest, even_modules, even_widest)) in
            GROUPS.iter().enumerate()
        {
            let end = GROUPS.get(k + 1).map_or(CHARACTER_VALUES, |g| g.0);
            let odd_count = (end - base) / even_count;
            assert_eq!(
                ranks(odd_modules, odd_widest, true),
                (0..odd_count).collect::<Vec<_>>(),
                "odd, group {k}"
            );
            assert_eq!(
                ranks(even_modules, even_widest, false),
                (0..even_count).collect::<Vec<_>>(),
                "even, group {k}"
            );
        }
    }
}

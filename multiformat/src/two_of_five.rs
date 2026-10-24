//! IATA 2 of 5 and Matrix 2 of 5: the 2 of 5 digit patterns (five elements, two of
//! them wide) with their own start and stop. Neither has a mandatory check digit,
//! so the digits are returned as printed.
//!
//! - IATA 2 of 5 encodes digits in the bars only, like Industrial 2 of 5; all
//!   spaces are narrow. Start: two narrow bars; stop: a wide and a narrow bar.
//! - Matrix 2 of 5 (Zint calls it Standard 2 of 5) encodes digits in bar, space,
//!   bar, space, bar, each digit followed by a narrow space. Start: a four-module
//!   bar and five narrow elements; stop: a four-module bar and four narrow elements.
//!
//! As for Industrial 2 of 5, the whole symbol must fit one physical model and show
//! quiet zones of at least seven narrow modules on both sides.

use crate::linear::{ITF_DIGITS, Read, wide_bits};

/// Element classes of the physical model.
#[derive(Clone, Copy, PartialEq)]
enum Class {
    Narrow,
    Wide,
    /// The four-module start/stop bar of Matrix 2 of 5.
    Long,
}

fn digit(pattern: u16) -> Option<u8> {
    ITF_DIGITS
        .iter()
        .position(|&x| u16::from(x) == pattern)
        .map(|d| d.to_le_bytes()[0])
}

/// Check `elements` (the runs from bar `s`, with their class) against one
/// narrow/wide model and both quiet zones, and build the read of `digits`. Ink
/// spread widens bars and narrows spaces by the same amount; it is measured on
/// the narrow elements and removed first.
fn finish(
    r: &[f32],
    s: usize,
    end: usize,
    elements: &[(f32, Class)],
    digits: &[u8],
    format: &'static str,
) -> Option<Read> {
    let narrow_of = |bar: bool| {
        let (sum, count) = elements
            .iter()
            .enumerate()
            .filter(|(i, e)| e.1 == Class::Narrow && (i % 2 == 0) == bar)
            .fold((0f32, 0f32), |(s, c), (_, e)| (s + e.0, c + 1.));
        sum / count
    };
    let spread = (narrow_of(true) - narrow_of(false)) / 2.;
    let elements: Vec<(f32, Class)> = elements
        .iter()
        .enumerate()
        .map(|(i, &(w, class))| (if i % 2 == 0 { w - spread } else { w + spread }, class))
        .collect();
    let elements = elements.as_slice();
    let mean = |class: Class| -> Option<f32> {
        let (sum, count) = elements
            .iter()
            .filter(|e| e.1 == class)
            .fold((0f32, 0f32), |(s, c), e| (s + e.0, c + 1.));
        (count > 0.).then(|| sum / count)
    };
    let narrow = mean(Class::Narrow)?;
    let wide = mean(Class::Wide)?;
    if narrow < 0.65 || !(1.8..=4.).contains(&(wide / narrow)) {
        return None;
    }
    let long = mean(Class::Long);
    if long.is_some_and(|l| l < wide * 1.05 || l > narrow * 6.) {
        return None;
    }
    if r[s - 1] < narrow * 7. || r.get(end).is_none_or(|&q| q < narrow * 7.) {
        return None;
    }
    // Every element within 0.4 of the wide-narrow difference of its class.
    let delta = wide - narrow;
    let mut total = 0f32;
    for &(w, class) in elements {
        let expected = match class {
            Class::Narrow => narrow,
            Class::Wide => wide,
            Class::Long => long.unwrap_or(wide),
        };
        let deviation = (w - expected).abs();
        if deviation > delta * 0.4 {
            return None;
        }
        total += deviation;
    }
    let error = total / (narrow * crate::numeric::usize_f32(elements.len()));
    if error > 0.2 {
        return None;
    }
    Some(Read {
        decoded: true,
        addon: None,
        format,
        text: digits.iter().map(|&d| char::from(b'0' + d)).collect(),
        start: s,
        end,
        error,
        codabar_start_stop: None,
        gs1: false,
    })
}

/// IATA 2 of 5 runs starting at bar `s`; `r[s - 1]` must be the left quiet zone.
pub(crate) fn iata(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    if s == 0 || s + 13 > r.len() {
        return None;
    }
    // Start: two narrow bars and their spaces, all of one width.
    let unit = r[s..s + 4].iter().sum::<f32>() / 4.;
    if r[s..s + 4].iter().any(|&w| (w - unit).abs() > unit * 0.5) {
        return None;
    }
    let mut digits = Vec::new();
    let mut at = s + 4;
    while digits.len() < 64 {
        // The stop (wide bar, narrow space, narrow bar) is followed by the right
        // quiet zone; inside the symbol the run after it is a narrow space.
        if !digits.is_empty()
            && r.get(at + 3).is_some_and(|&q| q >= unit * 5.)
            && r[at] > unit * 1.5
        {
            let mut elements: Vec<(f32, Class)> =
                r[s..s + 4].iter().map(|&w| (w, Class::Narrow)).collect();
            for (k, &d) in digits.iter().enumerate() {
                for i in 0..5 {
                    let wide = ITF_DIGITS[usize::from(d)] & (16 >> i) != 0;
                    let bar = s + 4 + k * 10 + 2 * i;
                    elements.push((r[bar], if wide { Class::Wide } else { Class::Narrow }));
                    elements.push((r[bar + 1], Class::Narrow));
                }
            }
            elements.extend([
                (r[at], Class::Wide),
                (r[at + 1], Class::Narrow),
                (r[at + 2], Class::Narrow),
            ]);
            if let Some(read) = finish(r, s, at + 3, &elements, &digits, "IATA25") {
                return Some(read);
            }
        }
        if at + 10 > r.len() {
            return None;
        }
        let bars: [f32; 5] = std::array::from_fn(|i| r[at + 2 * i]);
        let (pattern, _) = wide_bits(&bars, 5, 2)?;
        digits.push(digit(pattern)?);
        at += 10;
    }
    *limited |= at + 3 <= r.len();
    None
}

/// Matrix 2 of 5 runs starting at bar `s`; `r[s - 1]` must be the left quiet zone.
pub(crate) fn matrix(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    if s == 0 || s + 18 > r.len() {
        return None;
    }
    // Start: a four-module bar, then five narrow elements.
    let unit = r[s + 1..s + 6].iter().sum::<f32>() / 5.;
    if r[s] < unit * 2.8
        || r[s + 1..s + 6]
            .iter()
            .any(|&w| (w - unit).abs() > unit * 0.5)
    {
        return None;
    }
    // Ink spread from the start's narrow bars and spaces, removed before the
    // digit elements (bars and spaces alike) are classified.
    let spread = (f32::midpoint(r[s + 2], r[s + 4]) - (r[s + 1] + r[s + 3] + r[s + 5]) / 3.) / 2.;
    let mut digits = Vec::new();
    let mut at = s + 6;
    while digits.len() < 64 {
        // The stop (long bar and four narrow elements) is followed by the quiet
        // zone; a digit's fifth element is followed by a narrow space.
        if !digits.is_empty()
            && r.get(at + 5).is_some_and(|&q| q >= unit * 5.)
            && r[at] > unit * 2.8
        {
            let mut elements: Vec<(f32, Class)> = vec![(r[s], Class::Long)];
            elements.extend(r[s + 1..s + 6].iter().map(|&w| (w, Class::Narrow)));
            for (k, &d) in digits.iter().enumerate() {
                let first = s + 6 + k * 6;
                for i in 0..5 {
                    let wide = ITF_DIGITS[usize::from(d)] & (16 >> i) != 0;
                    elements.push((r[first + i], if wide { Class::Wide } else { Class::Narrow }));
                }
                elements.push((r[first + 5], Class::Narrow));
            }
            elements.push((r[at], Class::Long));
            elements.extend(r[at + 1..at + 5].iter().map(|&w| (w, Class::Narrow)));
            if let Some(read) = finish(r, s, at + 5, &elements, &digits, "Matrix25") {
                return Some(read);
            }
        }
        if at + 6 > r.len() {
            return None;
        }
        let elements: [f32; 5] =
            std::array::from_fn(|i| r[at + i] + if i % 2 == 0 { -spread } else { spread });
        let (pattern, _) = wide_bits(&elements, 5, 2)?;
        if r[at + 5] > unit * 1.8 {
            return None;
        }
        digits.push(digit(pattern)?);
        at += 6;
    }
    *limited |= at + 5 <= r.len();
    None
}

#[cfg(test)]
mod tests {
    use crate::linear::{IATA25, INDUSTRIAL25, ITF_DIGITS, MATRIX25, decode};

    /// Module widths of a symbol, bars first, without quiet zones.
    fn modules(text: &str, matrix: bool) -> Vec<u8> {
        let digits = text.bytes().map(|b| usize::from(b - b'0'));
        let element = |d: usize, i: usize| if ITF_DIGITS[d] & (16 >> i) != 0 { 3 } else { 1 };
        let mut out: Vec<u8> = if matrix {
            vec![4, 1, 1, 1, 1, 1]
        } else {
            vec![1, 1, 1, 1]
        };
        for d in digits {
            for i in 0..5 {
                out.push(element(d, i));
                if !matrix {
                    out.push(1);
                }
            }
            if matrix {
                out.push(1);
            }
        }
        out.extend::<&[u8]>(if matrix { &[4, 1, 1, 1, 1] } else { &[3, 1, 1] });
        out
    }

    fn runs(text: &str, matrix: bool, module: f32, spread: f32) -> Vec<f32> {
        let mut out = vec![12. * module];
        for (i, &w) in modules(text, matrix).iter().enumerate() {
            let s = if i % 2 == 0 { spread } else { -spread };
            out.push(f32::from(w) * module + s);
        }
        out.push(12. * module);
        out
    }

    #[test]
    fn reads_zint_runs() {
        // Zint 2.16 `-b 4 -d 4711` and `-b 2 -d 4711`.
        assert_eq!(
            modules("4711", false),
            [
                1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 3, 1, 1, 1,
                1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1
            ]
        );
        assert_eq!(
            modules("4711", true),
            [
                4, 1, 1, 1, 1, 1, 1, 1, 3, 1, 3, 1, 1, 1, 1, 3, 3, 1, 3, 1, 1, 1, 3, 1, 3, 1, 1, 1,
                3, 1, 4, 1, 1, 1, 1
            ]
        );
    }

    #[test]
    fn reads_both_variants_only_when_selected() {
        for (text, module, spread) in [
            ("1", 2., 0.),
            ("47110", 2.5, 0.4),
            ("0099887766554433", 1.6, -0.3),
        ] {
            for (matrix, mask, format) in [(false, IATA25, "IATA25"), (true, MATRIX25, "Matrix25")]
            {
                let symbol = runs(text, matrix, module, spread);
                let reads = decode(&symbol, false, mask);
                assert_eq!(reads.len(), 1, "{format} {text}");
                assert_eq!((reads[0].format, reads[0].text.as_str()), (format, text));
                let other = if matrix { IATA25 } else { MATRIX25 };
                assert!(decode(&symbol, false, other | INDUSTRIAL25).is_empty());
                assert!(decode(&symbol, false, 511).is_empty());
                let mut cut = symbol.clone();
                cut[0] = 3.;
                assert!(decode(&cut, false, mask).is_empty(), "{format} cut {text}");
            }
        }
    }

    #[test]
    fn rejects_noise() {
        let mut state = 0x25_u32;
        for _ in 0..20_000 {
            let mut runs = vec![40.];
            runs.extend((0..61).map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                2. + f32::from(state.to_be_bytes()[0]) * 6. / 255.
            }));
            runs.push(40.);
            let d = decode(&runs, false, IATA25 | MATRIX25);
            assert!(d.is_empty(), "{d:?} {runs:?}");
        }
    }
}

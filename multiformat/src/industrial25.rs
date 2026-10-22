//! Industrial 2 of 5 (also called Standard 2 of 5 or Code 2 of 5 Industrial): every
//! digit is five bars, two of them wide; all spaces are narrow and carry no data.
//! The start is wide, wide, narrow bars, the stop wide, narrow, wide bars. The check
//! digit is optional, so the digits are returned as printed.
//!
//! Only bars carry information and the symbology has no check: the reader requires
//! one narrow/wide bar model and one space width across the whole symbol, and quiet
//! zones of at least seven narrow modules on both sides (a symbol cut by the image
//! edge is not read).

use crate::linear::{ITF_DIGITS, Read, wide_bits};

/// Industrial 2 of 5 runs starting at bar `s`; `r[s - 1]` must be the left quiet zone.
pub(crate) fn decode(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    if s == 0 || s + 11 > r.len() {
        return None;
    }
    // Start: wide, wide, narrow bars; the narrow bar is the shortest of the three.
    let narrow_start = r[s + 4];
    if r[s] < narrow_start * 1.5 || r[s + 2] < narrow_start * 1.5 {
        return None;
    }
    let mut digits = Vec::new();
    let mut at = s + 6;
    while at + 5 <= r.len() && digits.len() < 64 {
        // The stop's last bar is followed by the right quiet zone; inside the
        // symbol the run after the third bar is a narrow space.
        if !digits.is_empty()
            && r.get(at + 5).is_some_and(|&q| q >= narrow_start * 5.)
            && let Some(read) = finish(r, s, at, &digits)
        {
            return Some(read);
        }
        if at + 10 > r.len() {
            return None;
        }
        let bars: [f32; 5] = std::array::from_fn(|i| r[at + 2 * i]);
        let (pattern, _) = wide_bits(&bars, 5, 2)?;
        digits.push(
            ITF_DIGITS
                .iter()
                .position(|&x| u16::from(x) == pattern)?
                .to_le_bytes()[0],
        );
        at += 10;
    }
    *limited |= at + 5 <= r.len() && digits.len() >= 64;
    None
}

/// Validate the complete symbol (start at `s`, stop at `stop`) against one physical model.
fn finish(r: &[f32], s: usize, stop: usize, digits: &[u8]) -> Option<Read> {
    let end = stop + 5;
    // Bars: start W W N, data, stop W N W; spaces: every space between them.
    let mut bars: Vec<(f32, bool)> = vec![(r[s], true), (r[s + 2], true), (r[s + 4], false)];
    for (k, &d) in digits.iter().enumerate() {
        for i in 0..5 {
            bars.push((
                r[s + 6 + k * 10 + 2 * i],
                ITF_DIGITS[usize::from(d)] & (16 >> i) != 0,
            ));
        }
    }
    bars.extend([(r[stop], true), (r[stop + 2], false), (r[stop + 4], true)]);
    let spaces: Vec<f32> = (s + 1..end).step_by(2).map(|i| r[i]).collect();
    let mean = |wide: bool| -> f32 {
        let (sum, count) = bars
            .iter()
            .filter(|b| b.1 == wide)
            .fold((0f32, 0f32), |(s, c), b| (s + b.0, c + 1.));
        sum / count
    };
    let (narrow, wide) = (mean(false), mean(true));
    let space = spaces.iter().sum::<f32>() / crate::numeric::usize_f32(spaces.len());
    // Ink spread moves bars and spaces in opposite directions; their average is the module.
    let module = f32::midpoint(narrow, space);
    if module < 0.65
        || !(1.8..=4.).contains(&(wide / narrow))
        || space > f32::midpoint(narrow, wide)
    {
        return None;
    }
    if r[s - 1] < module * 7. || r.get(end).is_none_or(|&q| q < module * 7.) {
        return None;
    }
    // Bars must cluster: each within 0.4 of the wide-narrow difference of its class
    // (so the classes stay apart); spaces within half a module of their mean.
    let delta = wide - narrow;
    let mut total = 0f32;
    for &(w, is_wide) in &bars {
        let deviation = (w - if is_wide { wide } else { narrow }).abs();
        if deviation > delta * 0.4 {
            return None;
        }
        total += deviation;
    }
    for &w in &spaces {
        let deviation = (w - space).abs();
        if deviation > module * 0.5 {
            return None;
        }
        total += deviation;
    }
    let count = bars.len() + spaces.len();
    let error = total / (module * crate::numeric::usize_f32(count));
    if error > 0.2 {
        return None;
    }
    Some(Read {
        decoded: true,
        addon: None,
        format: "Industrial25",
        text: digits.iter().map(|&d| char::from(b'0' + d)).collect(),
        start: s,
        end,
        error,
        gs1: false,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::linear::{INDUSTRIAL25, ITF_DIGITS, decode};

    /// Runs of an Industrial 2 of 5 symbol with quiet zones, wide ratio and ink spread.
    pub(crate) fn runs(text: &str, module: f32, ratio: f32, spread: f32) -> Vec<f32> {
        let mut widths: Vec<bool> = vec![true, true, false];
        for d in text.bytes().map(|b| usize::from(b - b'0')) {
            widths.extend((0..5).map(|i| ITF_DIGITS[d] & (16 >> i) != 0));
        }
        widths.extend([true, false, true]);
        let mut out = vec![12. * module];
        for (i, &wide) in widths.iter().enumerate() {
            if i > 0 {
                out.push(module - spread);
            }
            out.push(if wide { ratio * module } else { module } + spread);
        }
        out.push(12. * module);
        out
    }

    #[test]
    fn reads_industrial_2_of_5_in_both_directions() {
        for (text, module, ratio, spread) in [
            ("1", 2., 3., 0.),
            ("12345670", 2.5, 2.5, 0.4),
            ("0099887766554433", 1.6, 2.2, -0.3),
        ] {
            let forward = runs(text, module, ratio, spread);
            let reads = decode(&forward, false, INDUSTRIAL25);
            assert_eq!(reads.len(), 1, "{text}");
            assert_eq!(
                (reads[0].format, reads[0].text.as_str()),
                ("Industrial25", text)
            );
            let mut backward = forward;
            backward.reverse();
            let reads = decode(&backward, false, INDUSTRIAL25);
            assert_eq!(reads.len(), 0, "the stop does not start a symbol: {text}");
        }
        // Unselected, cut by the edge, or with an uneven space: nothing.
        let symbol = runs("1234", 2., 3., 0.);
        assert!(decode(&symbol, false, 511).is_empty());
        let mut cut = symbol.clone();
        cut[0] = 6.;
        assert!(decode(&cut, false, INDUSTRIAL25).is_empty());
        let mut uneven = symbol;
        uneven[10] = 6.;
        assert!(decode(&uneven, false, INDUSTRIAL25).is_empty());
    }

    #[test]
    fn rejects_noise_and_other_bar_codes() {
        let mut state = 0x2f5_u32;
        for _ in 0..20_000 {
            let mut runs = vec![40.];
            runs.extend((0..61).map(|i| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let [a, b, ..] = state.to_be_bytes();
                if i % 2 == 0 {
                    // Bars of any width between one and four modules.
                    2. + f32::from(a) * 6. / 255.
                } else {
                    2. + f32::from(b % 8) * 0.05
                }
            }));
            runs.push(40.);
            // Bar widths that do not cluster into narrow and wide do not fit one model.
            let d = decode(&runs, false, INDUSTRIAL25);
            assert!(d.is_empty(), "{d:?} {runs:?}");
        }
    }
}

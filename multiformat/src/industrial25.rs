//! Industrial 2 of 5 (also called Standard 2 of 5 or Code 2 of 5 Industrial): every
//! digit is five bars, two of them wide; all spaces are narrow and carry no data.
//! The start is wide, wide, narrow bars, the stop wide, narrow, wide bars. The check
//! digit is optional, so the digits are returned as printed.
//!
//! Only bars carry information and the symbology has no check: the reader requires
//! one narrow/wide bar model and one space width across the whole symbol, and quiet
//! zones of at least seven narrow modules on both sides (a symbol cut by the image
//! edge is not read).

use crate::linear::{ITF_DIGITS, MIN_MODULE, Read, full_quiet_zones, wide_bits};

/// Start bars: wide, wide, narrow.
const START: [bool; 3] = [true, true, false];
/// Stop bars: wide, narrow, wide.
const STOP: [bool; 3] = [true, false, true];
/// Smallest width of the start's wide bars relative to its narrow bar, before the
/// whole-symbol fit.
const START_WIDE_RATIO: f32 = 1.5;
/// A run after the stop's last bar this many start narrow bars wide may be the
/// right quiet zone (inside the symbol it is a narrow space); [`finish`] checks it.
const STOP_QUIET_RATIO: f32 = 5.;
/// Digits read before the work limit.
const MAX_DIGITS: usize = 64;
/// Accepted ratio of the mean wide to the mean narrow bar (printed 2 to 3).
const WIDE_RATIO: std::ops::RangeInclusive<f32> = 1.8..=4.;
/// Quiet zone required on both sides, in modules.
const QUIET_MODULES: f32 = 7.;
/// Largest distance of a bar from its class mean, as a fraction of the wide-narrow
/// difference: the classes stay apart.
const BAR_DEVIATION: f32 = 0.4;
/// Largest distance of a space from the mean space, in modules.
const SPACE_DEVIATION: f32 = 0.5;
/// Largest mean deviation of all elements from the model, in modules.
const MAX_ERROR: f32 = 0.2;

/// Whether wide/narrow bars spell a complete symbol: start, digits and stop.
pub(crate) fn bar_pattern(wide: &[bool]) -> bool {
    let n = wide.len();
    n >= 11
        && (n - 6).is_multiple_of(5)
        && wide[..3] == START
        && wide[n - 3..] == STOP
        && wide[3..n - 3].chunks(5).all(|digit| {
            let bits = digit.iter().fold(0u8, |v, &w| v * 2 + u8::from(w));
            ITF_DIGITS.contains(&bits)
        })
}

/// Industrial 2 of 5 runs starting at bar `s`; `r[s - 1]` must be the left quiet zone.
pub(crate) fn decode(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    if s == 0 || s + 11 > r.len() {
        return None;
    }
    // Start: wide, wide, narrow bars; the narrow bar is the shortest of the three.
    let narrow_start = r[s + 4];
    if r[s] < narrow_start * START_WIDE_RATIO || r[s + 2] < narrow_start * START_WIDE_RATIO {
        return None;
    }
    let mut digits = Vec::new();
    let mut at = s + 6;
    while at + 5 <= r.len() && digits.len() < MAX_DIGITS {
        // The stop's last bar is followed by the right quiet zone; inside the
        // symbol the run after the third bar is a narrow space.
        if !digits.is_empty()
            && r.get(at + 5)
                .is_some_and(|&q| q >= narrow_start * STOP_QUIET_RATIO)
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
    *limited |= at + 5 <= r.len() && digits.len() >= MAX_DIGITS;
    None
}

/// Validate the complete symbol (start at `s`, stop at `stop`) against one physical model.
fn finish(r: &[f32], s: usize, stop: usize, digits: &[u8]) -> Option<Read> {
    let end = stop + 5;
    // Bars: start W W N, data, stop W N W; spaces: every space between them.
    let mut bars: Vec<(f32, bool)> = START
        .iter()
        .enumerate()
        .map(|(i, &wide)| (r[s + 2 * i], wide))
        .collect();
    for (k, &d) in digits.iter().enumerate() {
        for i in 0..5 {
            bars.push((
                r[s + 6 + k * 10 + 2 * i],
                ITF_DIGITS[usize::from(d)] & (16 >> i) != 0,
            ));
        }
    }
    bars.extend(
        STOP.iter()
            .enumerate()
            .map(|(i, &wide)| (r[stop + 2 * i], wide)),
    );
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
    if module < MIN_MODULE
        || !WIDE_RATIO.contains(&(wide / narrow))
        || space > f32::midpoint(narrow, wide)
    {
        return None;
    }
    if !full_quiet_zones(r, s, end, module, QUIET_MODULES) {
        return None;
    }
    // Bars must cluster around their class means, spaces around theirs.
    let delta = wide - narrow;
    let mut total = 0f32;
    for &(w, is_wide) in &bars {
        let deviation = (w - if is_wide { wide } else { narrow }).abs();
        if deviation > delta * BAR_DEVIATION {
            return None;
        }
        total += deviation;
    }
    for &w in &spaces {
        let deviation = (w - space).abs();
        if deviation > module * SPACE_DEVIATION {
            return None;
        }
        total += deviation;
    }
    let count = bars.len() + spaces.len();
    let error = total / (module * crate::numeric::usize_f32(count));
    if error > MAX_ERROR {
        return None;
    }
    Some(Read {
        decoded: true,
        addon: None,
        codabar_start_stop: None,
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
    use super::{START, STOP};
    use crate::linear::{INDUSTRIAL25, ITF_DIGITS, decode};

    /// Runs of an Industrial 2 of 5 symbol with quiet zones, wide ratio and ink spread.
    pub(crate) fn runs(text: &str, module: f32, ratio: f32, spread: f32) -> Vec<f32> {
        let mut widths: Vec<bool> = START.to_vec();
        for d in text.bytes().map(|b| usize::from(b - b'0')) {
            widths.extend((0..5).map(|i| ITF_DIGITS[d] & (16 >> i) != 0));
        }
        widths.extend(STOP);
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
    fn reads_industrial_2_of_5_from_its_start() {
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

    /// Deterministic uniform value in 0..1.
    pub(crate) fn uniform(state: &mut u32) -> f32 {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f32::from(state.to_be_bytes()[0]) / 256. + f32::from(state.to_be_bytes()[1]) / 65_536.
    }

    /// `bars` bars of two widths (wide with probability 0.4, ratio 2 to 3.5, 10 %
    /// jitter) between 12-module quiet zones. Spaces are one module (±10 %), as in
    /// Industrial 2 of 5, or with `uneven` anywhere from 0.8 to 2.4 modules, like
    /// strokes of printed text.
    pub(crate) fn stripes(state: &mut u32, bars: usize, uneven: bool) -> Vec<f32> {
        let module = 2. + uniform(state) * 2.;
        let ratio = 2. + uniform(state) * 1.5;
        let mut out = vec![12. * module];
        for i in 0..bars {
            if i > 0 {
                let space = if uneven {
                    0.8 + uniform(state) * 1.6
                } else {
                    0.9 + uniform(state) * 0.2
                };
                out.push(space * module);
            }
            let width = if uniform(state) < 0.4 { ratio } else { 1. };
            out.push(width * module * (0.9 + uniform(state) * 0.2));
        }
        out.push(12. * module);
        out
    }

    #[test]
    fn evenly_spaced_two_width_stripes_rarely_read() {
        // Bars of two widths with even narrow spaces are Industrial 2 of 5 whenever
        // they happen to spell a start, digits and a stop: nothing else in the
        // symbology tells them apart. Groups of 11, 16 and 21 bars hold one to three
        // digits; at most 1 in 400 such groups read.
        let mut state = 0x1234_u32;
        let mut reads = 0;
        for trial in 0..6_000 {
            let runs = stripes(&mut state, [11, 16, 21][trial % 3], false);
            reads += decode(&runs, false, INDUSTRIAL25).len();
        }
        assert!(reads <= 15, "{reads} reads");
        // Text-like strokes with uneven spaces: at most 1 in 3,000.
        let mut reads = 0;
        for trial in 0..6_000 {
            let runs = stripes(&mut state, [11, 16, 21][trial % 3], true);
            reads += decode(&runs, false, INDUSTRIAL25).len();
        }
        assert!(reads <= 2, "{reads} text-like reads");
    }
}

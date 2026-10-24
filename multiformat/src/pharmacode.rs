//! Pharmacode (Laetus, one track): 2 to 16 narrow or wide bars with equal spaces and
//! no start, stop or check. Read from the right, bar `i` is worth `2^i` narrow or
//! `2^(i+1)` wide, so values run from 3 to 131,070.
//!
//! Nothing in the symbol shows where it starts, so the caller decides the reading
//! direction: runs are passed left to right as the symbol appears in the image. Any
//! group of bars in two widths with even spacing fits the format, so the reader is
//! strict: at least five bars, both widths present, one narrow and one wide bar width
//! and one space width across the symbol, spaces wider than the narrow bars (nominally
//! twice), and quiet zones of at least five space widths on both sides that the image
//! shows in full (a symbol cut by the edge is not read). A bar sequence that is a
//! complete Industrial 2 of 5 symbol is left to that format. The image scan needs
//! three agreeing lines.

use crate::linear::{MIN_MODULE, Read, full_quiet_zones};

/// Fewest bars read: shorter groups of two bar widths are too common in other prints.
pub(crate) const MIN_BARS: usize = 5;
const MAX_BARS: usize = 16;
/// A space wider than this multiple of the mean space so far ends the symbol.
const END_SPACE_RATIO: f32 = 2.5;
/// A first space wider than this multiple of the first bar: no symbol starts there.
const FIRST_SPACE_RATIO: f32 = 3.;
/// Quiet zone required on both sides, in mean space widths.
const QUIET_SPACES: f32 = 5.;
/// Accepted ratio of the mean wide to the mean narrow bar (printed 3).
const WIDE_RATIO: std::ops::RangeInclusive<f32> = 1.8..=5.;
/// Accepted mean space relative to the wide-narrow bar difference (printed 1).
const SPACE_TO_DELTA: std::ops::RangeInclusive<f32> = 0.5..=1.5;
/// Largest distance of a bar from its class mean, as a fraction of the wide-narrow
/// difference.
const BAR_DEVIATION: f32 = 0.3;
/// Largest distance of a space from the mean space, as a fraction of it.
const SPACE_DEVIATION: f32 = 0.35;
/// Largest total deviation from the model, as a fraction of the symbol width.
const MAX_ERROR: f32 = 0.06;
/// Smallest mean space relative to the narrow bar (printed 2).
const MIN_SPACE_TO_NARROW: f32 = 1.2;

/// Pharmacode runs starting at bar `s`; `r[s - 1]` must be the left quiet zone.
pub(crate) fn decode(r: &[f32], s: usize) -> Option<Read> {
    if s == 0 {
        return None;
    }
    // The symbol ends at the first space much wider than the spaces before it.
    let mut end = s + 1;
    let mut space_sum = 0f32;
    loop {
        if end >= r.len() {
            // The image ends inside the symbol or without a visible quiet zone.
            return None;
        }
        let bars = (end - s).div_ceil(2);
        let spaces = bars - 1;
        if spaces > 0 && r[end] > END_SPACE_RATIO * space_sum / crate::numeric::usize_f32(spaces) {
            break;
        }
        if spaces == 0 && r[end] > FIRST_SPACE_RATIO * r[s] {
            return None;
        }
        if bars > MAX_BARS {
            return None;
        }
        space_sum += r[end];
        end += 2;
    }
    let bars: Vec<f32> = (s..end).step_by(2).map(|i| r[i]).collect();
    if !(MIN_BARS..=MAX_BARS).contains(&bars.len()) {
        return None;
    }
    let spaces: Vec<f32> = (s + 1..end).step_by(2).map(|i| r[i]).collect();
    let space = spaces.iter().sum::<f32>() / crate::numeric::usize_f32(spaces.len());
    if space < MIN_MODULE || !full_quiet_zones(r, s, end, space, QUIET_SPACES) {
        return None;
    }
    // Two bar classes split at the largest gap between sorted widths.
    let mut sorted = bars.clone();
    sorted.sort_by(f32::total_cmp);
    let (cut, _) = sorted
        .windows(2)
        .enumerate()
        .map(|(i, w)| (i, w[1] - w[0]))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let narrow = sorted[..=cut].iter().sum::<f32>() / crate::numeric::usize_f32(cut + 1);
    let wide =
        sorted[cut + 1..].iter().sum::<f32>() / crate::numeric::usize_f32(sorted.len() - cut - 1);
    let threshold = f32::midpoint(narrow, wide);
    // Nominal widths are 1:3 with spaces of two narrow bars; ink spread changes bars
    // and spaces in opposite directions but leaves the bar width difference unchanged.
    let delta = wide - narrow;
    if !WIDE_RATIO.contains(&(wide / narrow)) || !SPACE_TO_DELTA.contains(&(space / delta)) {
        return None;
    }
    let mut total = 0f32;
    for &w in &bars {
        let deviation = (w - if w > threshold { wide } else { narrow }).abs();
        if deviation > delta * BAR_DEVIATION {
            return None;
        }
        total += deviation;
    }
    for &w in &spaces {
        let deviation = (w - space).abs();
        if deviation > space * SPACE_DEVIATION {
            return None;
        }
        total += deviation;
    }
    let pattern: Vec<bool> = bars.iter().map(|&w| w > threshold).collect();
    if crate::industrial25::bar_pattern(&pattern)
        || crate::industrial25::bar_pattern(&pattern.iter().rev().copied().collect::<Vec<_>>())
    {
        return None;
    }
    let value = pattern
        .iter()
        .fold(0u32, |v, &wide| v * 2 + if wide { 2 } else { 1 });
    let error = total / (bars.iter().sum::<f32>() + spaces.iter().sum::<f32>());
    // Spaces are nominally twice the narrow bar (1 mm and 0.5 mm); Industrial 2 of 5
    // and text strokes have spaces about as wide as their bars.
    if error > MAX_ERROR || space < narrow * MIN_SPACE_TO_NARROW {
        return None;
    }
    Some(Read {
        decoded: true,
        addon: None,
        codabar_start_stop: None,
        format: "Pharmacode",
        text: value.to_string(),
        start: s,
        end,
        error,
        gs1: false,
    })
}

#[cfg(test)]
mod tests {
    use crate::linear::{INDUSTRIAL25, PHARMACODE, decode};

    /// Runs of a Pharmacode symbol (narrow 1, wide 3, space 2 modules) with ink spread.
    fn runs(value: u32, module: f32, spread: f32) -> Vec<f32> {
        let mut bars = Vec::new();
        let mut v = value;
        while v > 0 {
            let wide = v.is_multiple_of(2);
            bars.push(wide);
            v = (v - if wide { 2 } else { 1 }) / 2;
        }
        bars.reverse();
        let mut out = vec![15. * module];
        for (i, &wide) in bars.iter().enumerate() {
            if i > 0 {
                out.push(2. * module - spread);
            }
            out.push(if wide { 3. } else { 1. } * module + spread);
        }
        out.push(15. * module);
        out
    }

    #[test]
    fn reads_values_from_left_to_right() {
        for (value, module, spread) in [
            (1234, 2., 0.),
            (131_069, 1.5, 0.3),
            (45, 3., -0.5),
            (65_533, 2.2, 0.4),
        ] {
            let reads = decode(&runs(value, module, spread), false, PHARMACODE);
            assert_eq!(reads.len(), 1, "{value}");
            assert_eq!(
                (reads[0].format, reads[0].text.as_str()),
                ("Pharmacode", value.to_string().as_str())
            );
        }
        // Too few bars, one width only (values 2^n - 1 and 2^(n+1) - 2 are not read:
        // nothing tells narrow from wide), unselected, or cut by the edge: nothing.
        for (runs, mask) in [
            (runs(10, 2., 0.), PHARMACODE),
            (runs(15, 2., 0.), PHARMACODE),
            (runs(1234, 2., 0.), 511),
        ] {
            assert!(decode(&runs, false, mask).is_empty());
        }
        let mut cut = runs(1234, 2., 0.);
        cut[0] = 4.;
        assert!(decode(&cut, false, PHARMACODE).is_empty());
    }

    #[test]
    fn rejects_noise_and_industrial_2_of_5() {
        let mut state = 0x9a4_u32;
        for _ in 0..20_000 {
            let mut runs = vec![40.];
            runs.extend((0..15).map(|i| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let [a, b, ..] = state.to_be_bytes();
                if i % 2 == 0 {
                    2. + f32::from(a % 4) * 2.
                } else if i % 4 == 1 {
                    // Spaces of two widths, as in another symbology: not even spacing.
                    2. + f32::from(b % 2) * 0.2
                } else {
                    5.
                }
            }));
            runs.push(40.);
            let d = decode(&runs, false, PHARMACODE);
            assert!(d.is_empty(), "{d:?} {runs:?}");
        }
        let industrial = crate::industrial25::tests::runs("1234", 2., 3., 0.);
        assert!(decode(&industrial, false, PHARMACODE).is_empty());
        assert_eq!(
            decode(&industrial, false, PHARMACODE | INDUSTRIAL25).len(),
            1
        );
    }

    #[test]
    fn two_width_stripes_rarely_read() {
        use crate::industrial25::tests::stripes;
        // Spaces as wide as the narrow bars (Industrial 2 of 5, most bar codes)
        // never read: Pharmacode spaces are wider than its narrow bars.
        let mut state = 0x5eed_u32;
        for trial in 0..3_000 {
            let runs = stripes(&mut state, 5 + trial % 12, false);
            assert!(decode(&runs, false, PHARMACODE).is_empty(), "{runs:?}");
        }
        // Any evenly spaced group of two bar widths with wider spaces is a
        // Pharmacode. Text-like strokes with uneven spaces sometimes are: at most
        // 1 in 75 groups of 5 to 16 strokes read on one line (the image scan
        // needs three agreeing lines).
        let mut reads = 0;
        for trial in 0..6_000 {
            let runs = stripes(&mut state, 5 + trial % 12, true);
            reads += decode(&runs, false, PHARMACODE).len();
        }
        assert!(reads <= 80, "{reads} reads");
    }
}

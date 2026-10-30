//! Telepen's even-parity, least-significant-bit-first codewords and modulo-127 check.
//!
//! Independent implementation of the published SB Electronic Systems description:
//! <https://advanova.co.uk/wp-content/uploads/2022/05/Barcode-Symbology-information-and-History.pdf>.
//! Narrow/narrow encodes 1, wide/narrow 00, wide/wide 010, and narrow/wide
//! alternates 01 and 10. Each codeword occupies sixteen modules. AIM start/stop
//! variants select ASCII, numeric-to-ASCII, or ASCII-to-numeric respectively.
//! Legacy numeric labels reuse the ASCII start; that interpretation is opt-in,
//! never guessed from printable payloads. No reference decoder is called.

use crate::linear::{MIN_MODULE, Read, full_quiet_zones};

const STARTS: [[u8; 12]; 3] = [
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3],
    [1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 3],
    [1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1, 3],
];
// Allow isolated sampled-edge errors, but keep narrow/wide acceptance bands
// disjoint and retain the stricter mean error, parity and checksum gates.
const ELEMENT_ERROR: f32 = 0.75;
const MEAN_ERROR: f32 = 0.25;
const QUIET: f32 = 7.;
const MAX_CODEWORDS: usize = 128;

#[derive(Clone, Copy)]
struct Scale {
    module: f32,
    spread: f32,
}
impl Scale {
    fn adjusted(self, width: f32, bar: bool) -> f32 {
        if bar {
            width - self.spread
        } else {
            width + self.spread
        }
    }
    fn element(self, width: f32, bar: bool) -> Option<u8> {
        let width = self.adjusted(width, bar);
        let size = if width < 2. * self.module { 1 } else { 3 };
        ((width - f32::from(size) * self.module).abs() <= self.module).then_some(size)
    }
}

/// Return one eight-bit even-parity word, its end run and the local module width.
/// Only bounded work is spent; a character has at most sixteen runs.
fn character(runs: &[f32], start: usize, scale: Scale) -> Option<(u8, usize, f32, f32)> {
    let mut word = 0u8;
    let mut bits = 0u32;
    let mut open = false;
    let mut at = start;
    let mut classes = [0u8; 16];
    while bits < 8 {
        let a = scale.element(*runs.get(at)?, true)?;
        let b = scale.element(*runs.get(at + 1)?, false)?;
        let (value, count) = match (a, b, open) {
            (1, 1, _) => (1, 1),
            (3, 1, false) => (0, 2),
            (3, 3, false) => (2, 3),
            (1, 3, false) => {
                open = true;
                (2, 2)
            }
            (1, 3, true) => {
                open = false;
                (1, 2)
            }
            _ => return None,
        };
        if bits + count > 8 || at + 2 - start > classes.len() {
            return None;
        }
        word |= value << bits;
        bits += count;
        classes[at - start] = a;
        classes[at + 1 - start] = b;
        at += 2;
    }
    if open || !word.count_ones().is_multiple_of(2) {
        return None;
    }
    let module = runs[start..at].iter().sum::<f32>() / 16.;
    if module < MIN_MODULE || (module / scale.module - 1.).abs() > 0.25 {
        return None;
    }
    // Estimate ink spread per character: photographed/antialiased bars can
    // change thickness along a label. Provisional classes are checked below
    // against the fitted model, parity and the whole-symbol checksum.
    let mut residual = 0.;
    for (i, &class) in classes[..at - start].iter().enumerate() {
        let delta = runs[start + i] - f32::from(class) * module;
        residual += if i % 2 == 0 { delta } else { -delta };
    }
    let spread = residual / crate::numeric::usize_f32(at - start);
    if spread.abs() > 0.5 * module {
        return None;
    }
    let scale = Scale { module, spread };
    let mut deviation = 0.;
    for (i, &class) in classes[..at - start].iter().enumerate() {
        let error = (scale.adjusted(runs[start + i], i % 2 == 0) - f32::from(class) * module).abs();
        if error > ELEMENT_ERROR * module {
            return None;
        }
        deviation += error;
    }
    if deviation > 16. * module * MEAN_ERROR {
        return None;
    }
    Some((word & 127, at, module, deviation / (16. * module)))
}

fn stop(runs: &[f32], at: usize, variant: usize, scale: Scale) -> bool {
    // The final one-module space merges into the right quiet zone. Only eleven
    // elements belong to the visible stop; do not consume the following symbol.
    let Some(elements) = runs.get(at..at + 11) else {
        return false;
    };
    elements.iter().enumerate().all(|(i, &w)| {
        (scale.adjusted(w, i % 2 == 0) - f32::from(STARTS[variant][11 - i]) * scale.module).abs()
            <= ELEMENT_ERROR * scale.module
    }) && runs
        .get(at + 11)
        .is_some_and(|&q| q >= QUIET * scale.module)
}

fn payload(words: &[u8], variant: usize, legacy_numeric: bool) -> Option<String> {
    let mut numeric = variant == 1 || (variant == 0 && legacy_numeric);
    let may_switch = variant != 0 || legacy_numeric;
    let mut switched = false;
    let mut out = String::with_capacity(words.len() * 2);
    for &w in words {
        if w == 16 && may_switch {
            if switched {
                return None;
            }
            numeric = !numeric;
            switched = true;
        } else if !numeric || w < 16 || w == 127 {
            out.push(char::from(w));
        } else if w >= 27 {
            let n = w - 27;
            out.push(char::from(b'0' + n / 10));
            out.push(char::from(b'0' + n % 10));
        } else if w >= 17 {
            out.push(char::from(b'0' + w - 17));
            out.push('X');
        } else {
            return None;
        }
    }
    Some(out)
}

pub(crate) fn decode(
    runs: &[f32],
    start: usize,
    legacy_numeric: bool,
    limited: &mut bool,
) -> Option<Read> {
    let header = runs.get(start..start + 12)?;
    // All three starts begin with six narrow runs. Estimate ink spread before
    // classifying: a cheap prefix rejection avoids allocation on other formats.
    let black = (header[0] + header[2] + header[4]) / 3.;
    let white = (header[1] + header[3] + header[5]) / 3.;
    let module = black.midpoint(white);
    if !module.is_finite() || module < MIN_MODULE || (black - white).abs() > module {
        return None;
    }
    // Every start has eight black and eight white modules. Fit all twelve
    // elements after the cheap prefix gate; a few rounded narrow pixels must
    // not bias the scale/spread estimate for the entire payload.
    let black_sum: f32 = header.iter().step_by(2).sum();
    let white_sum: f32 = header.iter().skip(1).step_by(2).sum();
    let module = (black_sum + white_sum) / 16.;
    if !module.is_finite() || module < MIN_MODULE {
        return None;
    }
    let mut scale = Scale {
        module,
        spread: (black_sum - white_sum) / 12.,
    };
    if start == 0 || runs[start - 1] < QUIET * module {
        return None;
    }
    let variant = STARTS.iter().position(|pattern| {
        header.iter().enumerate().all(|(i, &w)| {
            (scale.adjusted(w, i % 2 == 0) - f32::from(pattern[i]) * module).abs()
                <= ELEMENT_ERROR * module
        })
    })?;
    let mut words = Vec::new();
    let mut at = start + 12;
    let mut checksum = 0u32;
    let mut max_error = 0f32;
    loop {
        if words.len() >= 2 && stop(runs, at, variant, scale) {
            if !checksum.is_multiple_of(127)
                || !full_quiet_zones(runs, start, at + 11, module.min(scale.module), QUIET)
            {
                return None;
            }
            words.pop(); // Last word is the check character, never payload.
            let text = payload(&words, variant, legacy_numeric)?;
            return Some(Read {
                decoded: true,
                addon: None,
                codabar_start_stop: None,
                format: if legacy_numeric {
                    "TelepenNumeric"
                } else {
                    "Telepen"
                },
                text,
                start,
                end: at + 11,
                error: max_error,
                gs1: false,
            });
        }
        if words.len() >= MAX_CODEWORDS {
            *limited = true;
            return None;
        }
        let (word, next, local_module, error) = character(runs, at, scale)?;
        checksum += u32::from(word);
        words.push(word);
        at = next;
        scale.module = local_module;
        max_error = max_error.max(error);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod dispatch_tests {
    use super::tests::symbol;
    use crate::{formats, linear};

    #[test]
    fn format_masks_and_mixed_profiles() {
        let first = symbol(b"LIBRARY-0042", 0);
        assert!(linear::decode(&first, false, formats::RETAIL_MASK).is_empty());
        for mask in [
            formats::TELEPEN,
            formats::ENGINE_LINEAR_MASK & !formats::TELEPEN_NUMERIC,
        ] {
            let reads = linear::decode(&first, false, mask);
            assert_eq!(reads.len(), 1);
            assert_eq!(reads[0].format, "Telepen");
            assert_eq!(reads[0].text, "LIBRARY-0042");
        }
        let numeric = symbol(&[27, 39, 61, 83], 0);
        let reads = linear::decode(&numeric, false, formats::TELEPEN | formats::TELEPEN_NUMERIC);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].format, "TelepenNumeric");
        assert_eq!(reads[0].text, "00123456");
        let mut both = first;
        both.extend_from_slice(&symbol(b"SECOND", 0)[1..]);
        let reads = linear::decode(&both, false, formats::TELEPEN);
        assert_eq!(
            reads.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["LIBRARY-0042", "SECOND"]
        );
    }
}

#[cfg(test)]
mod noise_tests {
    #[test]
    fn rejects_unstructured_stripe_profiles() {
        let mut seed = 0xA5B7_193Du32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            u8::try_from(seed >> 24).unwrap()
        };
        for _ in 0..10_000 {
            let length = 32 + usize::from(next());
            let mut runs = Vec::with_capacity(length + 2);
            runs.push(20.);
            for _ in 0..length {
                runs.push(0.5 + f32::from(next()) / 64.);
            }
            runs.push(20.);
            assert!(crate::linear::decode(&runs, false, crate::formats::TELEPEN).is_empty());
        }
    }
}

//! Japan Post 4-State Customer Code.
//!
//! 67 bars: a start pair, 20 characters of three bars, a check character and a
//! stop pair. The 19 characters are the digits, `-` and control characters
//! CC1 to CC8: CC1, CC2 and CC3 followed by a digit spell `A`-`J`, `K`-`T` and
//! `U`-`Z`, and CC4 pads the data to 20 characters. The check character makes
//! the sum of all 21 values divisible by 19. The reported text omits the check
//! character and the padding.
use crate::four_state::Bar;

const LENGTH: usize = 67;
const START: [Bar; 2] = [Bar::Full, Bar::Descender];
const STOP: [Bar; 2] = [Bar::Descender, Bar::Full];

/// Bars of each character by check value: `0`-`9`, `-`, CC1-CC8.
const TABLE: [[Bar; 3]; 19] = {
    use Bar::{Ascender as A, Descender as D, Full as F, Tracker as T};
    [
        [F, T, T],
        [F, F, T],
        [F, D, A],
        [D, F, A],
        [F, A, D],
        [F, T, F],
        [D, A, F],
        [A, F, D],
        [A, D, F],
        [T, F, F],
        [T, F, T],
        [D, A, T],
        [D, T, A],
        [A, D, T],
        [T, D, A],
        [A, T, D],
        [T, A, D],
        [T, T, F],
        [F, F, F],
    ]
};

/// Value of the code `-` and the first control character.
const DASH: usize = 10;
const CC1: usize = 11;
const PAD: usize = 14;

/// The data of `bars` read in the given order, or `None` when they are not a
/// valid symbol.
#[must_use]
pub fn decode(bars: &[Bar]) -> Option<String> {
    if !framed(bars) {
        return None;
    }
    let values = bars[2..LENGTH - 2]
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| TABLE.iter().position(|t| t == c))
        .collect::<Option<Vec<_>>>()?;
    if !values.iter().sum::<usize>().is_multiple_of(19) {
        return None;
    }
    let data = &values[..20];
    let length = data.iter().position(|&v| v == PAD).unwrap_or(20);
    if data[length..].iter().any(|&v| v != PAD) {
        return None;
    }
    let mut text = String::new();
    let mut values = data[..length].iter();
    while let Some(&v) = values.next() {
        let c = match v {
            0..=9 => b'0' + u8::try_from(v).ok()?,
            DASH => b'-',
            CC1..=13 => {
                let digit = *values.next().filter(|&&d| d <= 9)?;
                let letter = (v - CC1) * 10 + digit;
                if letter >= 26 {
                    return None;
                }
                b'A' + u8::try_from(letter).ok()?
            }
            _ => return None,
        };
        text.push(char::from(c));
    }
    (!text.is_empty()).then_some(text)
}

/// Whether `bars` have the length, start and stop of a symbol.
#[must_use]
pub fn framed(bars: &[Bar]) -> bool {
    bars.len() == LENGTH && bars[..2] == START && bars[LENGTH - 2..] == STOP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::four_state::testing::{from_codes, render, zint_bars};

    const DUMP: [&str; 3] = [
        "8A 22 A2 08 22 28 A2 28 28 8A 08 8A 08 82 08 22 88",
        "AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA A8",
        "AA 22 8A 08 28 A2 82 28 88 88 88 A2 08 88 20 88 A8",
    ];

    #[test]
    fn table_matches_zint() {
        // Zint's JapanTable in the order "1234567890-abcdefgh".
        let zint: [[u8; 3]; 19] = [
            [0, 0, 3],
            [0, 2, 1],
            [2, 0, 1],
            [0, 1, 2],
            [0, 3, 0],
            [2, 1, 0],
            [1, 0, 2],
            [1, 2, 0],
            [3, 0, 0],
            [0, 3, 3],
            [3, 0, 3],
            [2, 1, 3],
            [2, 3, 1],
            [1, 2, 3],
            [3, 2, 1],
            [1, 3, 2],
            [3, 1, 2],
            [3, 3, 0],
            [0, 0, 0],
        ];
        for (k, codes) in zint.iter().enumerate() {
            // "1".."9" have values 1..9, "0" value 0, the rest keep their index.
            let value = match k {
                0..=8 => k + 1,
                9 => 0,
                _ => k,
            };
            assert_eq!(from_codes(codes), TABLE[value], "entry {k}");
        }
    }

    #[test]
    fn decodes_zint_symbols() {
        // zint --dump --barcode=JAPANPOST -d 15400233-16-4-205
        assert_eq!(
            decode(&zint_bars(DUMP)).as_deref(),
            Some("15400233-16-4-205")
        );
        // zint --dump --barcode=JAPANPOST -d 1060032ABZ
        let bars = zint_bars([
            "8A 20 2A 08 0A 88 88 08 A2 08 82 08 20 82 08 2A 08",
            "AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA A8",
            "AA 20 8A 08 28 A2 08 20 A0 88 88 20 82 08 20 8A 28",
        ]);
        assert_eq!(decode(&bars).as_deref(), Some("1060032ABZ"));
        let flipped: Vec<Bar> = bars.iter().rev().map(|b| b.flipped()).collect();
        assert!(decode(&flipped).is_none());
    }

    #[test]
    fn rejects_damage() {
        let bars = zint_bars(DUMP);
        let mut wrong = bars.clone();
        // "1" becomes "2": a valid character, but the check fails.
        wrong[2..5].copy_from_slice(&TABLE[2]);
        assert!(decode(&wrong).is_none());
        assert!(decode(&bars[..LENGTH - 1]).is_none());
    }

    #[test]
    fn reads_rendered_symbols() {
        let bars = zint_bars(DUMP);
        for (module, degrees, blur, effort) in [(3., 0., 0, 0), (3., 160., 0, 0), (3., 0., 2, 2)] {
            let (gray, width, height) = render(&bars, module, degrees, blur);
            assert_eq!(
                crate::postal::scan(&gray, width, height, effort, crate::formats::JAPAN_POST),
                ["15400233-16-4-205"]
            );
        }
    }
}

//! Royal Mail 4-State Customer Code (RM4SCC, also CBC).
//!
//! A start bar (ascender), four bars per character, a check character and a stop
//! bar (full). Each character is a row and a column of a 6x6 table of `0-9A-Z`:
//! two of its four bars extend upwards (the row) and two downwards (the column).
//! The check character's row and column are the sums of the data rows and
//! columns modulo 6. The reported text omits the check character.
use crate::four_state::Bar;

pub const CHARSET: &[u8; 36] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Row or column (1 to 6) of four bars, the first bar as the highest bit; `None`
/// unless exactly two are set.
const fn half(bits: u8) -> Option<usize> {
    match bits {
        0b0011 => Some(1),
        0b0101 => Some(2),
        0b0110 => Some(3),
        0b1001 => Some(4),
        0b1010 => Some(5),
        0b1100 => Some(6),
        _ => None,
    }
}

/// Table index of four bars, or `None` when they break the two-up/two-down rule.
#[must_use]
pub fn character(bars: &[Bar]) -> Option<usize> {
    let (up, down) = bars.iter().fold((0, 0), |(up, down), bar| {
        (
            up << 1 | u8::from(bar.up()),
            down << 1 | u8::from(bar.down()),
        )
    });
    Some((half(up)? - 1) * 6 + half(down)? - 1)
}

/// Check character of table indices `values`.
fn check(values: &[usize]) -> usize {
    let row = values.iter().map(|v| v / 6 + 1).sum::<usize>() % 6;
    let column = values.iter().map(|v| v % 6 + 1).sum::<usize>() % 6;
    // A sum divisible by six selects the sixth row or column.
    (row + 5) % 6 * 6 + (column + 5) % 6
}

/// Whether `bars` have RM4SCC's start and stop bars and a whole number of
/// characters, including the check character.
#[must_use]
pub fn framed(bars: &[Bar]) -> bool {
    bars.len() >= crate::four_state::MIN_BARS
        && (bars.len() - 2).is_multiple_of(4)
        && bars[0] == Bar::Ascender
        && bars[bars.len() - 1] == Bar::Full
}

/// The data characters of `bars` read in the given order, without the check
/// character, or `None` when the bars are not a valid symbol.
#[must_use]
pub fn decode(bars: &[Bar]) -> Option<String> {
    if !framed(bars) {
        return None;
    }
    let values = bars[1..bars.len() - 1]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| character(c))
        .collect::<Option<Vec<_>>>()?;
    let (&checked, data) = values.split_last()?;
    (check(data) == checked).then(|| data.iter().map(|&v| char::from(CHARSET[v])).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::four_state::testing::{render, zint_bars};

    fn encode(text: &str) -> Vec<Bar> {
        const PATTERNS: [u8; 6] = [0b0011, 0b0101, 0b0110, 0b1001, 0b1010, 0b1100];
        let mut values: Vec<usize> = text
            .bytes()
            .map(|b| CHARSET.iter().position(|&c| c == b).unwrap())
            .collect();
        values.push(check(&values));
        let mut bars = vec![Bar::Ascender];
        for v in values {
            let (up, down) = (PATTERNS[v / 6], PATTERNS[v % 6]);
            bars.extend(
                (0..4)
                    .rev()
                    .map(|b| Bar::new(up >> b & 1 == 1, down >> b & 1 == 1)),
            );
        }
        bars.push(Bar::Full);
        bars
    }

    fn scan(gray: &[u8], width: usize, height: usize, effort: usize) -> Vec<String> {
        crate::postal::scan(gray, width, height, effort, crate::formats::RM4SCC)
    }

    #[test]
    fn reads_rendered_symbols() {
        let bars = encode("SN34RD1A");
        for (module, degrees, blur, effort) in [
            (3., 0., 0, 0),
            (3., 15., 0, 0),
            (3., 30., 0, 1),
            (3., 90., 0, 0),
            (3., 180., 0, 0),
            (2., 0., 0, 1),
            (4., 20., 1, 1),
            (3., 0., 2, 2),
        ] {
            let (gray, width, height) = render(&bars, module, degrees, blur);
            assert_eq!(
                scan(&gray, width, height, effort),
                ["SN34RD1A"],
                "module {module} angle {degrees} blur {blur} effort {effort}"
            );
        }
    }

    #[test]
    fn ignores_plain_bars() {
        // Evenly spaced full bars frame no symbol.
        let bars = vec![Bar::Full; 38];
        let (gray, width, height) = render(&bars, 3., 10., 0);
        assert_eq!(scan(&gray, width, height, 2), Vec::<String>::new());
    }

    #[test]
    fn decodes_zint_symbol() {
        // zint --dump --barcode=RM4SCC -d W1J0TR01
        let bars = zint_bars([
            "A8 02 A0 82 A2 22 02 82 A8 2",
            "AA AA AA AA AA AA AA AA AA A",
            "0A 08 88 82 A8 20 82 88 8A 2",
        ]);
        assert_eq!(bars.len(), 38);
        assert_eq!(decode(&bars).as_deref(), Some("W1J0TR01"));
        assert_eq!(bars, encode("W1J0TR01"));
    }

    #[test]
    fn table_and_check() {
        // Every character's bars have two ascenders and two descenders.
        for (i, &c) in CHARSET.iter().enumerate() {
            let bars = encode(&char::from(c).to_string());
            assert_eq!(character(&bars[1..5]), Some(i));
        }
        assert_eq!(check(&[0]), 0);
        // Rows 1 + 6 = 7 -> 1, columns 1 + 6 = 7 -> 1.
        assert_eq!(check(&[0, 35]), 0);
        // Rows 6 + 6 = 12 -> 6 (Z's row), columns 6 + 6 -> 6.
        assert_eq!(check(&[35, 35]), 35);
        assert_eq!(char::from(CHARSET[check(&[34, 1])]), '0');
    }

    #[test]
    fn rejects_damage() {
        let bars = encode("SN34RD1A");
        assert_eq!(decode(&bars).as_deref(), Some("SN34RD1A"));
        // A wrong check character.
        let mut wrong = bars.clone();
        let n = wrong.len();
        let other = character(&bars[n - 5..n - 1]).unwrap() + 1;
        let replacement = encode(&char::from(CHARSET[other % 36]).to_string());
        wrong[n - 5..n - 1].copy_from_slice(&replacement[1..5]);
        assert!(decode(&wrong).is_none());
        // A character with three ascenders.
        let mut broken = bars.clone();
        broken[1] = Bar::Full;
        broken[2] = Bar::Full;
        broken[3] = Bar::Full;
        assert!(decode(&broken).is_none());
        // Missing stop bar, and the upside-down order.
        assert!(decode(&bars[..bars.len() - 1]).is_none());
        let flipped: Vec<Bar> = bars.iter().rev().map(|b| b.flipped()).collect();
        assert!(decode(&flipped).is_none());
    }
}

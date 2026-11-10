//! Australia Post 4-State Customer Barcode.
//!
//! 37, 52 or 67 bars: an ascender-tracker start pair, the two-digit Format
//! Control Code, the eight-digit Delivery Point Identifier, customer
//! information (52 and 67 bars only), filler trackers, four Reed-Solomon
//! parity symbols over GF(64) and a stop pair equal to the start. Digits take
//! two bars and customer letters three bars. The reported text is the FCC,
//! the DPID and the customer information, without parity or filler.
//!
//! The bars do not say whether customer information holds digits (N table) or
//! characters (C table). It is read as digits when every bar pair is a digit
//! up to trailing filler, and as characters otherwise; a trailing `z` (three
//! trackers, the same as filler) is not reported.
use crate::four_state::Bar;
use crate::reed_solomon;

const LENGTHS: [usize; 3] = [37, 52, 67];
const FRAME: [Bar; 2] = [Bar::Ascender, Bar::Tracker];
/// Parity symbols of three bars each.
const PARITY: usize = 4;
/// Filler and the bar code of a tracker.
const FILLER: u8 = 3;

/// Characters of the C table, in the order of [`C_TABLE`].
const C_CHARSET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz #";
/// Bar codes (0 full, 1 ascender, 2 descender, 3 tracker) of each C character.
const C_TABLE: [[u8; 3]; 64] = [
    [2, 2, 2],
    [3, 0, 0],
    [3, 0, 1],
    [3, 0, 2],
    [3, 1, 0],
    [3, 1, 1],
    [3, 1, 2],
    [3, 2, 0],
    [3, 2, 1],
    [3, 2, 2],
    [0, 0, 0],
    [0, 0, 1],
    [0, 0, 2],
    [0, 1, 0],
    [0, 1, 1],
    [0, 1, 2],
    [0, 2, 0],
    [0, 2, 1],
    [0, 2, 2],
    [1, 0, 0],
    [1, 0, 1],
    [1, 0, 2],
    [1, 1, 0],
    [1, 1, 1],
    [1, 1, 2],
    [1, 2, 0],
    [1, 2, 1],
    [1, 2, 2],
    [2, 0, 0],
    [2, 0, 1],
    [2, 0, 2],
    [2, 1, 0],
    [2, 1, 1],
    [2, 1, 2],
    [2, 2, 0],
    [2, 2, 1],
    [0, 2, 3],
    [0, 3, 0],
    [0, 3, 1],
    [0, 3, 2],
    [0, 3, 3],
    [1, 0, 3],
    [1, 1, 3],
    [1, 2, 3],
    [1, 3, 0],
    [1, 3, 1],
    [1, 3, 2],
    [1, 3, 3],
    [2, 0, 3],
    [2, 1, 3],
    [2, 2, 3],
    [2, 3, 0],
    [2, 3, 1],
    [2, 3, 2],
    [2, 3, 3],
    [3, 0, 3],
    [3, 1, 3],
    [3, 2, 3],
    [3, 3, 0],
    [3, 3, 1],
    [3, 3, 2],
    [3, 3, 3],
    [0, 0, 3],
    [0, 1, 3],
];

const fn code(bar: Bar) -> u8 {
    match bar {
        Bar::Full => 0,
        Bar::Ascender => 1,
        Bar::Descender => 2,
        Bar::Tracker => 3,
    }
}

/// The digit of an N-table bar pair: `a * 3 + b` for 00 to 30.
fn digit(pair: &[u8]) -> Option<u8> {
    let value = pair[0] * 3 + pair[1];
    (pair[1] < 3 && value < 10).then_some(value)
}

/// Customer information as digits followed only by filler.
fn numeric(codes: &[u8]) -> Option<String> {
    let mut text = String::new();
    let mut at = 0;
    while let Some(d) = codes.get(at..at + 2).and_then(digit) {
        text.push(char::from(b'0' + d));
        at += 2;
    }
    codes[at..].iter().all(|&c| c == FILLER).then_some(text)
}

/// Customer information as C-table characters followed only by filler.
fn characters(codes: &[u8]) -> Option<String> {
    let (triples, rest) = codes.as_chunks::<3>();
    if rest.iter().any(|&c| c != FILLER) {
        return None;
    }
    let length = triples.len()
        - triples
            .iter()
            .rev()
            .take_while(|t| **t == [FILLER; 3])
            .count();
    triples[..length]
        .iter()
        .map(|t| {
            C_TABLE
                .iter()
                .position(|c| c == t)
                .map(|i| char::from(C_CHARSET[i]))
        })
        .collect()
}

/// The FCC, DPID and customer information of `bars` read in the given order,
/// or `None` when they are not a valid symbol. At most one damaged parity
/// symbol is corrected.
#[must_use]
pub fn decode(bars: &[Bar]) -> Option<String> {
    if !framed(bars) {
        return None;
    }
    let codes: Vec<u8> = bars[2..bars.len() - 2].iter().map(|&b| code(b)).collect();
    let mut words: Vec<u16> = codes
        .as_chunks::<3>()
        .0
        .iter()
        .map(|&[a, b, c]| u16::from(a << 4 | b << 2 | c))
        .collect();
    // Four parity symbols correct two errors; one keeps a margin against
    // misread rows that land near a codeword.
    if reed_solomon::words(6, 0x43).correct(&mut words, PARITY, 1)? > 1 {
        return None;
    }
    let data: Vec<u8> = words[..words.len() - PARITY]
        .iter()
        .flat_map(|&w| [w >> 4, w >> 2 & 3, w & 3].map(|c| u8::try_from(c).unwrap_or(FILLER)))
        .collect();
    let digits = data[..20]
        .chunks(2)
        .map(|pair| digit(pair).map(|d| char::from(b'0' + d)))
        .collect::<Option<String>>()?;
    let fcc = &digits[..2];
    let customer = &data[20..];
    let info = match (fcc, bars.len()) {
        ("00" | "11" | "45" | "87" | "92", 37) => (customer == [FILLER]).then(String::new)?,
        ("59", 52) | ("62", 67) => numeric(customer).or_else(|| characters(customer))?,
        _ => return None,
    };
    Some(digits + &info)
}

/// Whether `bars` have the length, start and stop of a symbol.
#[must_use]
pub fn framed(bars: &[Bar]) -> bool {
    LENGTHS.contains(&bars.len()) && bars[..2] == FRAME && bars[bars.len() - 2..] == FRAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::four_state::testing::{render, zint_bars};

    fn symbols() -> [([&'static str; 3], &'static str); 5] {
        [
            // zint --dump --barcode=AUSPOST -d 39987520
            (
                [
                    "8A AA 22 02 88 A0 88 8A 22 0",
                    "AA AA AA AA AA AA AA AA AA 8",
                    "08 82 22 A8 2A A0 A2 02 28 0",
                ],
                "1139987520",
            ),
            // zint --dump --barcode=AUSPOST -d 3998752011223344
            (
                [
                    "88 2A 22 02 88 AA A8 8A AA A0 2A 02 08",
                    "AA AA AA AA AA AA AA AA AA AA AA AA AA",
                    "02 22 22 A8 2A A8 8A A2 20 00 22 A8 A0",
                ],
                "593998752011223344",
            ),
            // zint --dump --barcode=AUSPOST -d 39987520123456789012345
            (
                [
                    "82 8A 22 02 88 AA 8A A8 22 02 AA 8A A8 20 A2 82 20",
                    "AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA A8",
                    "0A A2 22 A8 2A A8 A2 02 A8 A2 A8 A2 02 08 00 8A 00",
                ],
                "6239987520123456789012345",
            ),
            // zint --dump --barcode=AUSPOST -d 3998752012345ABCDE
            (
                [
                    "82 8A 22 02 88 A2 8A 20 A2 AA AA 8A AA 28 28 0A 20",
                    "AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA A8",
                    "0A A2 22 A8 2A A2 88 28 20 2A A2 A8 A0 28 A0 82 00",
                ],
                "623998752012345ABCDE",
            ),
            // zint --dump --barcode=AUSREPLY -d 12345678
            (
                [
                    "8A 8A 8A A8 22 00 82 00 8A 0",
                    "AA AA AA AA AA AA AA AA AA 8",
                    "00 28 A2 02 A8 A0 08 A8 A0 0",
                ],
                "4512345678",
            ),
        ]
    }

    #[test]
    fn decodes_zint_symbols() {
        for (dump, text) in symbols() {
            let bars = zint_bars(dump);
            assert_eq!(decode(&bars).as_deref(), Some(text));
            let flipped: Vec<Bar> = bars.iter().rev().map(|b| b.flipped()).collect();
            assert!(decode(&flipped).is_none(), "{text}");
        }
    }

    #[test]
    fn corrects_one_symbol_only() {
        let (dump, text) = symbols()[1];
        let bars = zint_bars(dump);
        let mut damaged = bars.clone();
        damaged[10] = damaged[10].flipped();
        damaged[11] = Bar::new(!damaged[11].up(), damaged[11].down());
        assert_eq!(decode(&damaged).as_deref(), Some(text));
        // A second damaged symbol is beyond the accepted correction.
        damaged[30] = Bar::new(!damaged[30].up(), damaged[30].down());
        assert!(decode(&damaged).is_none());
    }

    #[test]
    fn reads_rendered_symbols() {
        let (dump, text) = symbols()[3];
        let bars = zint_bars(dump);
        for (module, degrees, blur, effort) in [(3., 0., 0, 0), (3., 170., 0, 0), (3., 0., 2, 2)] {
            let (gray, width, height) = render(&bars, module, degrees, blur);
            assert_eq!(
                crate::postal::scan(&gray, width, height, effort, crate::formats::AUSTRALIA_POST),
                [text]
            );
        }
    }
}

//! USPS Intelligent Mail Barcode (USPS-B-3200).
//!
//! 65 bars carry ten 13-bit characters spread over the ascenders and
//! descenders. Each character is a codeword from table I (five set bits) or
//! table II (two set bits), inverted when the matching bit of the 11-bit
//! frame check sequence is set; the FCS is a CRC over the 102-bit binary value
//! of the data. The reported text is the 20-digit tracking code followed by
//! the 0, 5, 9 or 11-digit routing code, without separator.
use crate::four_state::Bar;
use std::sync::OnceLock;

const LENGTH: usize = 65;

/// Bar of each character bit: entry `13 * i + j` is the bar map position
/// (1-based) of bit `j` of character `i`. Positions 1-65 are the descenders of
/// bars 1-65, positions 66-130 their ascenders.
const BIT_POSITIONS: [u8; 130] = [
    67, 6, 78, 16, 86, 95, 34, 40, 45, 113, 117, 121, 62, 87, 18, 104, 41, 76, 57, 119, 115, 72,
    97, 2, 127, 26, 105, 35, 122, 52, 114, 7, 24, 82, 68, 63, 94, 44, 77, 112, 70, 100, 39, 30,
    107, 15, 125, 85, 10, 65, 54, 88, 20, 106, 46, 66, 8, 116, 29, 61, 99, 80, 90, 37, 123, 51, 25,
    84, 129, 56, 4, 109, 96, 28, 36, 47, 11, 71, 33, 102, 21, 9, 17, 49, 124, 79, 64, 91, 42, 69,
    53, 60, 14, 1, 27, 103, 126, 75, 89, 50, 120, 19, 32, 110, 92, 111, 130, 59, 31, 12, 81, 43,
    55, 5, 74, 22, 101, 128, 58, 118, 48, 108, 38, 98, 93, 23, 83, 13, 73, 3,
];

/// Characters of table I (`n` = 5, 1287 entries) or table II (`n` = 2, 78
/// entries) by codeword, built as in USPS-B-3200 Appendix D.
fn table(n: u32, length: usize) -> Vec<u16> {
    let mut table = vec![0; length];
    let (mut low, mut high) = (0, length);
    for count in 0u16..1 << 13 {
        if count.count_ones() != n {
            continue;
        }
        let reverse = count.reverse_bits() >> 3;
        if reverse < count {
            continue;
        }
        if reverse == count {
            high -= 1;
            table[high] = count;
        } else {
            table[low] = count;
            table[low + 1] = reverse;
            low += 2;
        }
    }
    table
}

/// Codeword of every 13-bit character with its FCS bit, or `None`.
fn codewords() -> &'static [Option<(u16, bool)>] {
    static CODEWORDS: OnceLock<Vec<Option<(u16, bool)>>> = OnceLock::new();
    CODEWORDS.get_or_init(|| {
        let mut codewords = vec![None; 1 << 13];
        for (offset, table) in [(0, table(5, 1287)), (1287, table(2, 78))] {
            for (i, &c) in table.iter().enumerate() {
                let word = u16::try_from(offset + i).unwrap_or(u16::MAX);
                codewords[usize::from(c)] = Some((word, false));
                codewords[usize::from(c ^ 0x1FFF)] = Some((word, true));
            }
        }
        codewords
    })
}

/// The 11-bit CRC of USPS-B-3200 over the 102-bit value in 13 bytes.
fn frame_check(bytes: &[u8; 13]) -> u16 {
    const POLYNOMIAL: u16 = 0x0F35;
    let mut fcs: u16 = 0x07FF;
    let mut step = |data: &mut u16| {
        fcs = if (fcs ^ *data) & 0x400 == 0 {
            fcs << 1
        } else {
            fcs << 1 ^ POLYNOMIAL
        } & 0x07FF;
        *data <<= 1;
    };
    // The first byte holds only the value's top six bits.
    let mut data = u16::from(bytes[0]) << 5;
    for _ in 0..6 {
        step(&mut data);
    }
    for &byte in &bytes[1..] {
        let mut data = u16::from(byte) << 3;
        for _ in 0..8 {
            step(&mut data);
        }
    }
    fcs
}

/// The tracking and routing digits of `bars` read in the given order, or
/// `None` when they are not a valid symbol.
#[must_use]
pub fn decode(bars: &[Bar]) -> Option<String> {
    if !framed(bars) {
        return None;
    }
    let bit = |position: u8| {
        let p = usize::from(position) - 1;
        if p < LENGTH {
            bars[p].down()
        } else {
            bars[p - LENGTH].up()
        }
    };
    let mut words = [0u16; 10];
    let mut fcs = 0u16;
    for (i, positions) in BIT_POSITIONS.as_chunks::<13>().0.iter().enumerate() {
        let character = positions
            .iter()
            .enumerate()
            .fold(0u16, |c, (j, &p)| c | u16::from(bit(p)) << j);
        let (word, inverted) = codewords()[usize::from(character)]?;
        words[i] = word;
        fcs |= u16::from(inverted) << i;
    }
    if words[9] % 2 != 0 {
        return None;
    }
    words[9] /= 2;
    if words[0] >= 659 {
        words[0] -= 659;
        fcs |= 1 << 10;
    }
    if words[0] >= 659 || words[1..9].iter().any(|&w| w >= 1365) || words[9] >= 636 {
        return None;
    }
    let mut value = u128::from(words[0]);
    for &w in &words[1..9] {
        value = value * 1365 + u128::from(w);
    }
    value = value * 636 + u128::from(words[9]);
    let bytes: [u8; 13] = value.to_be_bytes()[3..].try_into().ok()?;
    if frame_check(&bytes) != fcs {
        return None;
    }
    let mut tracking = [0u8; 20];
    for d in tracking[2..].iter_mut().rev() {
        *d = u8::try_from(value % 10).ok()?;
        value /= 10;
    }
    tracking[1] = u8::try_from(value % 5).ok()?;
    value /= 5;
    tracking[0] = u8::try_from(value % 10).ok()?;
    value /= 10;
    let routing = match value {
        0 => String::new(),
        1..=100_000 => format!("{:05}", value - 1),
        100_001..=1_000_100_000 => format!("{:09}", value - 100_001),
        _ if value < 1_000_100_001 + 100_000_000_000 => format!("{:011}", value - 1_000_100_001),
        _ => return None,
    };
    let mut text: String = tracking.iter().map(|&d| char::from(b'0' + d)).collect();
    text.push_str(&routing);
    Some(text)
}

/// Whether `bars` have the length of a symbol. Intelligent Mail has no start
/// or stop pattern.
#[must_use]
pub fn framed(bars: &[Bar]) -> bool {
    bars.len() == LENGTH
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::four_state::testing::{render, zint_bars};

    const TRACKER: &str = "AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA AA 8";

    fn symbols() -> [([&'static str; 3], &'static str); 4] {
        [
            // zint --dump --barcode=USPS_IMAIL -d 01234567094987654321-01234567891
            (
                [
                    "A0 A2 08 28 A2 00 A8 00 0A 88 02 A0 02 2A 88 28 8",
                    TRACKER,
                    "08 AA 22 02 02 8A 02 88 22 22 A2 AA 00 A0 28 82 0",
                ],
                "0123456709498765432101234567891",
            ),
            // zint --dump --barcode=USPS_IMAIL -d 00040123456200800001
            (
                [
                    "20 00 A8 8A 2A AA AA 08 A2 AA A0 20 88 88 AA 0A 8",
                    TRACKER,
                    "08 20 2A AA A0 20 80 02 0A 80 88 0A AA 28 0A 80 8",
                ],
                "00040123456200800001",
            ),
            // zint --dump --barcode=USPS_IMAIL -d 01234567094987654321-012345678
            (
                [
                    "88 28 00 A0 82 A2 22 0A A0 88 80 00 80 88 80 A2 8",
                    TRACKER,
                    "28 0A 00 80 20 02 20 20 2A 20 08 82 A2 02 22 A2 0",
                ],
                "01234567094987654321012345678",
            ),
            // zint --dump --barcode=USPS_IMAIL -d 01234567094987654321-01234
            (
                [
                    "02 A0 08 08 80 22 2A 28 08 AA 02 2A 82 02 80 0A 0",
                    TRACKER,
                    "80 8A 08 88 A2 88 88 A0 A8 02 80 A0 0A 8A 2A 2A 0",
                ],
                "0123456709498765432101234",
            ),
        ]
    }

    #[test]
    fn tables_match_the_specification() {
        assert_eq!(table(5, 1287)[..4], [0x001F, 0x1F00, 0x002F, 0x1E80]);
        assert_eq!(table(2, 78)[..4], [0x0003, 0x1800, 0x0005, 0x1400]);
        assert!(table(5, 1287).iter().chain(&table(2, 78)).all(|&c| c != 0));
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
    fn rejects_single_bar_damage() {
        let bars = zint_bars(symbols()[0].0);
        for i in 0..LENGTH {
            for damaged in [
                Bar::new(!bars[i].up(), bars[i].down()),
                Bar::new(bars[i].up(), !bars[i].down()),
            ] {
                let mut copy = bars.clone();
                copy[i] = damaged;
                assert!(decode(&copy).is_none(), "bar {i}");
            }
        }
    }

    #[test]
    fn reads_rendered_symbols() {
        let (dump, text) = symbols()[0];
        let bars = zint_bars(dump);
        for (module, degrees, blur, effort) in [(3., 0., 0, 0), (3., 190., 0, 0), (3., 0., 2, 2)] {
            let (gray, width, height) = render(&bars, module, degrees, blur);
            assert_eq!(
                crate::postal::scan(
                    &gray,
                    width,
                    height,
                    effort,
                    crate::formats::INTELLIGENT_MAIL
                ),
                [text]
            );
        }
    }
}

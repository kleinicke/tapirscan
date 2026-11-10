//! Dutch KIX code (`PostNL` Klantenindex): RM4SCC's characters without start,
//! stop or check character.
//!
//! Every character has two ascenders and two descenders, so the same bars read
//! upside down are again valid characters, and nothing in the symbol checks the
//! data. KIX therefore accepts only text shaped like a Dutch address, a
//! postcode, house number and optional `X` with suffix
//! (`[1-9][0-9]{3}[A-Z]{2}[0-9]{1,5}(X[0-9A-Z]{1,6})?`), and only when exactly
//! one of the two reading directions has that shape.
use crate::four_state::{Bar, Row};
use crate::rm4scc::{CHARSET, character};

/// Characters of the shortest address (postcode and one house-number digit).
const MIN_CHARS: usize = 7;
/// The Netherlands post's maximum KIX length.
const MAX_CHARS: usize = 18;

/// Whether `text` is a postcode, house number and optional `X` and suffix.
fn address(text: &[u8]) -> bool {
    let Some((postcode, rest)) = text.split_at_checked(6) else {
        return false;
    };
    if !matches!(postcode[0], b'1'..=b'9')
        || !postcode[1..4].iter().all(u8::is_ascii_digit)
        || !postcode[4..].iter().all(u8::is_ascii_uppercase)
    {
        return false;
    }
    let number = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    match &rest[number..] {
        [] => (1..=5).contains(&number),
        [b'X', suffix @ ..] => {
            (1..=5).contains(&number)
                && (1..=6).contains(&suffix.len())
                && suffix
                    .iter()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
        }
        _ => false,
    }
}

/// The address encoded by `bars` read in the given order, or `None`.
#[must_use]
pub fn decode(bars: &[Bar]) -> Option<String> {
    if !bars.len().is_multiple_of(4) || !(MIN_CHARS..=MAX_CHARS).contains(&(bars.len() / 4)) {
        return None;
    }
    let text = bars
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| character(c).map(|v| CHARSET[v]))
        .collect::<Option<Vec<u8>>>()?;
    if !address(&text) {
        return None;
    }
    String::from_utf8(text).ok()
}

/// The address and the row in its reading direction, when exactly one
/// direction reads.
#[must_use]
pub fn read(row: &Row) -> Option<(String, Row)> {
    // KIX has no check character and a part of an address can be an address.
    if row.open {
        return None;
    }
    let rotated = row.rotated();
    match (decode(&row.bars), decode(&rotated.bars)) {
        (Some(text), None) => Some((text, row.clone())),
        (None, Some(text)) => Some((text, rotated)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::four_state::testing::{render, zint_bars};

    fn scan(gray: &[u8], width: usize, height: usize, effort: usize) -> Vec<String> {
        crate::postal::scan(gray, width, height, effort, crate::formats::KIX)
    }

    #[test]
    fn decodes_zint_symbols() {
        // zint --dump --barcode=KIX -d 2500GG30250
        let bars = zint_bars([
            "0A 0A 0A 0A 28 28 0A 0A 0A 0A 0A",
            "AA AA AA AA AA AA AA AA AA AA AA",
            "28 A0 0A 0A 88 88 82 0A 28 A0 0A",
        ]);
        assert_eq!(decode(&bars).as_deref(), Some("2500GG30250"));
        // zint --dump --barcode=KIX -d 1231FZ13XHS
        let bars = zint_bars([
            "0A 0A 0A 0A 28 A0 0A 0A A0 28 88",
            "AA AA AA AA AA AA AA AA AA AA AA",
            "22 28 82 22 82 A0 22 82 82 A0 88",
        ]);
        assert_eq!(decode(&bars).as_deref(), Some("1231FZ13XHS"));
        let row = Row {
            bars: bars.iter().rev().map(|b| b.flipped()).collect(),
            polygon: [[0.; 2]; 4],
            open: false,
        };
        assert_eq!(read(&row).map(|r| r.0).as_deref(), Some("1231FZ13XHS"));
    }

    #[test]
    fn address_shape() {
        for good in [
            "2500GG30250",
            "1231FZ13XHS",
            "9999ZZ1",
            "1000AA12345X123456",
        ] {
            assert!(address(good.as_bytes()), "{good}");
        }
        for bad in [
            "0500GG30250",
            "2500G330250",
            "2500GG",
            "2500GG123456",
            "2500GG12X",
            "2500GGX1",
            "2500GG12XABCDEFG",
            "2500GG12Y1",
        ] {
            assert!(!address(bad.as_bytes()), "{bad}");
        }
    }

    #[test]
    fn reads_rendered_symbols() {
        let bars = zint_bars([
            "0A 0A 0A 0A 28 A0 0A 0A A0 28 88",
            "AA AA AA AA AA AA AA AA AA AA AA",
            "22 28 82 22 82 A0 22 82 82 A0 88",
        ]);
        for (module, degrees, blur, effort) in [(3., 0., 0, 0), (3., 200., 0, 0), (3., 0., 2, 2)] {
            let (gray, width, height) = render(&bars, module, degrees, blur);
            assert_eq!(scan(&gray, width, height, effort), ["1231FZ13XHS"]);
        }
    }

    #[test]
    fn rejects_parts_of_a_longer_row() {
        // "1231FZ13" alone is an address: a row that ends where more ink
        // follows must not be read as one.
        let bars = zint_bars([
            "0A 0A 0A 0A 28 A0 0A 0A A0 28 88",
            "AA AA AA AA AA AA AA AA AA AA AA",
            "22 28 82 22 82 A0 22 82 82 A0 88",
        ]);
        let (gray, width, height) = render(&bars, 3., 0., 0);
        let column = |bar: usize| width / 2 - 3 * bars.len() + 6 * bar;
        // The image ends after the eighth character.
        let cut = column(32) - 1;
        let cropped: Vec<u8> = gray
            .chunks(width)
            .flat_map(|row| &row[..cut])
            .copied()
            .collect();
        assert_eq!(scan(&cropped, cut, height, 2), [] as [String; 0]);
        // The first bar of the ninth character is missing.
        let mut gap = gray.clone();
        for row in gap.chunks_mut(width) {
            row[column(32) - 1..column(33) - 1].fill(230);
        }
        assert_eq!(scan(&gap, width, height, 2), [] as [String; 0]);
        // The same eight characters with a quiet zone read.
        let (gray, width, height) = render(&bars[..32], 3., 0., 0);
        assert_eq!(scan(&gray, width, height, 2), ["1231FZ13"]);
    }
}

//! Opt-in pharmacy-number interpretations of Code 39 reads (Code 32, PZN).
//!
//! Both are ordinary Code 39 symbols. Selecting `Code32` or `PZN` runs the
//! Code 39 reader; a read whose content is a valid number of that scheme is
//! reported with that format and the number as text. The Code 39 content stays
//! available as payload bytes. Other Code 39 reads are kept only when Code 39
//! itself is selected.

use crate::{Barcode, Format, format::SUPPORTED_MASK};

/// The opt-in formats read by the Code 39 reader. Other opt-in formats have
/// their own readers.
const VARIANT_MASK: u32 = Format::Code32 as u32 | Format::Pzn as u32;

/// Code 32 base-32 alphabet: digits and consonants (no A, E, I, O).
const CODE32_ALPHABET: &[u8; 32] = b"0123456789BCDFGHJKLMNPQRSTUVWXYZ";

/// Format bits for the engine: opt-in formats are read by the Code 39 reader.
pub(crate) const fn engine_bits(bits: u32) -> u32 {
    let base = bits & !VARIANT_MASK;
    if bits & VARIANT_MASK == 0 {
        base
    } else {
        base | Format::Code39 as u32
    }
}

/// Reinterpret Code 39 reads for the selected opt-in formats.
pub(crate) fn apply(bits: u32, barcodes: &mut Vec<Barcode>) {
    debug_assert_eq!(bits & !SUPPORTED_MASK, 0);
    if bits & VARIANT_MASK == 0 {
        return;
    }
    let code39 = Format::Code39 as u32;
    for barcode in barcodes.iter_mut().filter(|b| b.format == Format::Code39) {
        let converted = [
            (Format::Code32, code32_number as fn(&str) -> Option<String>),
            (Format::Pzn, pzn_number),
        ]
        .into_iter()
        .filter(|(format, _)| bits & *format as u32 != 0)
        .find_map(|(format, convert)| convert(&barcode.text).map(|text| (format, text)));
        if let Some((format, text)) = converted {
            let raw = std::mem::replace(&mut barcode.text, text);
            barcode
                .payload_bytes
                .get_or_insert_with(|| raw.into_bytes());
            barcode.format = format;
        }
    }
    if bits & code39 == 0 {
        barcodes.retain(|b| b.format != Format::Code39);
    }
}

/// Italian pharmacy code: six base-32 characters encoding 8 digits plus a
/// check digit, returned as the 9-digit number with leading zeros.
fn code32_number(content: &str) -> Option<String> {
    if content.len() != 6 {
        return None;
    }
    let mut value: u32 = 0;
    for byte in content.bytes() {
        let digit = CODE32_ALPHABET.iter().position(|&c| c == byte)?;
        value = value * 32 + u32::try_from(digit).ok()?;
    }
    if value >= 1_000_000_000 {
        return None;
    }
    let number = format!("{value:09}");
    let digits: Vec<u32> = number.bytes().map(|b| u32::from(b - b'0')).collect();
    // Odd positions add the digit; even positions add the digit sum of twice it.
    let sum: u32 = digits[..8]
        .iter()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                (2 * d) / 10 + (2 * d) % 10
            } else {
                d
            }
        })
        .sum();
    (sum % 10 == digits[8]).then_some(number)
}

/// German pharmacy number: Code 39 content `-` plus 8 digits (PZN8) or 7
/// digits (older PZN7), the last digit a modulo-11 check. Returned without `-`.
fn pzn_number(content: &str) -> Option<String> {
    let number = content.strip_prefix('-')?;
    if !matches!(number.len(), 7 | 8) || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: Vec<u32> = number.bytes().map(|b| u32::from(b - b'0')).collect();
    let (body, check) = digits.split_at(digits.len() - 1);
    // PZN8 weights the seven body digits 1..7, PZN7 weights its six 2..7.
    let first_weight = if body.len() == 7 { 1 } else { 2 };
    let sum: u32 = body.iter().zip(first_weight..).map(|(&d, w)| d * w).sum();
    (sum % 11 == check[0]).then(|| number.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn barcode(text: &str) -> Barcode {
        serde_json::from_value(serde_json::json!({
            "text": text, "format": "Code39", "support": 1,
            "polygon": [[0, 0], [10, 0], [10, 5], [0, 5]]
        }))
        .unwrap()
    }

    #[test]
    fn code32_decodes_base32_with_check_digit() {
        assert_eq!(code32_number("0CSSBD").as_deref(), Some("012345676"));
        // Wrong check digit (012345677 encodes as 0CSSBF) and non-alphabet letters.
        assert_eq!(code32_number("0CSSBF"), None);
        assert_eq!(code32_number("0CSSBA"), None);
        assert_eq!(code32_number("0CSSB"), None);
        assert_eq!(code32_number("ZZZZZZ"), None);
    }

    #[test]
    fn pzn_checks_modulo_11_for_both_lengths() {
        assert_eq!(pzn_number("-02532876").as_deref(), Some("02532876"));
        assert_eq!(pzn_number("-02532877"), None);
        assert_eq!(pzn_number("02532876"), None);
        // PZN7 4908802: 4*2 + 9*3 + 0*4 + 8*5 + 8*6 + 0*7 = 123, 123 % 11 = 2.
        assert_eq!(pzn_number("-4908802").as_deref(), Some("4908802"));
        assert_eq!(pzn_number("-4908803"), None);
    }

    #[test]
    fn apply_converts_selected_variants_and_keeps_raw_bytes() {
        let code32 = Format::Code32 as u32;
        let pzn = Format::Pzn as u32;
        let code39 = Format::Code39 as u32;
        let mut reads = vec![barcode("0CSSBD"), barcode("-02532876"), barcode("HELLO")];
        apply(code32 | pzn, &mut reads);
        let got: Vec<_> = reads.iter().map(|b| (b.format, b.text.as_str())).collect();
        assert_eq!(
            got,
            [(Format::Code32, "012345676"), (Format::Pzn, "02532876")]
        );
        assert_eq!(reads[0].payload_bytes.as_deref(), Some(&b"0CSSBD"[..]));

        let mut reads = vec![barcode("0CSSBD"), barcode("-02532876"), barcode("HELLO")];
        apply(pzn | code39, &mut reads);
        let got: Vec<_> = reads.iter().map(|b| (b.format, b.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (Format::Code39, "0CSSBD"),
                (Format::Pzn, "02532876"),
                (Format::Code39, "HELLO")
            ]
        );

        let mut reads = vec![barcode("-02532876")];
        apply(code39, &mut reads);
        assert_eq!(reads[0].format, Format::Code39);
        assert_eq!(engine_bits(code32), code39);
        assert_eq!(engine_bits(code39 | 1), code39 | 1);
    }
}

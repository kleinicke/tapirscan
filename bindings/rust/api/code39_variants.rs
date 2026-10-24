//! Opt-in interpretations of Code 39 reads: the pharmacy numbers Code 32 and PZN,
//! and Extended (full-ASCII) Code 39.
//!
//! All are ordinary Code 39 symbols. Selecting `Code32`, `PZN` or `Code39Extended`
//! runs the Code 39 reader; a read whose content is valid in that scheme is
//! reported with that format and the interpreted text (the number, or the
//! content with its shift pairs expanded). The Code 39 content stays available as
//! payload bytes. With Code 39 also selected, content without shift characters
//! stays a Code 39 read. Other Code 39 reads are kept only when Code 39
//! itself is selected; otherwise they and unread Code 39 regions are undecoded
//! regions of the selected variant, or of unknown format when both are selected.

use crate::{Barcode, Format, UndecodedRegion, format::SUPPORTED_MASK};

/// The opt-in formats read by the Code 39 reader. Other opt-in formats have
/// their own readers.
const VARIANT_MASK: u32 =
    Format::Code32 as u32 | Format::Pzn as u32 | Format::Code39Extended as u32;

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

/// Reinterpret Code 39 reads and regions for the selected opt-in formats.
pub(crate) fn apply(bits: u32, barcodes: &mut Vec<Barcode>, undecoded: &mut Vec<UndecodedRegion>) {
    debug_assert_eq!(bits & !SUPPORTED_MASK, 0);
    if bits & VARIANT_MASK == 0 {
        return;
    }
    let code39 = Format::Code39 as u32;
    for barcode in barcodes.iter_mut().filter(|b| b.format == Format::Code39) {
        let shifted = barcode.text.contains(['$', '%', '/', '+']);
        let converted = [
            (Format::Code32, code32_number as fn(&str) -> Option<String>),
            (Format::Pzn, pzn_number),
            (Format::Code39Extended, full_ascii),
        ]
        .into_iter()
        .filter(|(format, _)| {
            bits & *format as u32 != 0
                && (*format != Format::Code39Extended || shifted || bits & code39 == 0)
        })
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
        // Report only selected formats: a Code 39 symbol that is no valid number
        // of the selected scheme is an unread candidate for it.
        let variant = [Format::Code32, Format::Pzn, Format::Code39Extended]
            .into_iter()
            .find(|format| bits & VARIANT_MASK == *format as u32);
        for region in undecoded.iter_mut() {
            if region.format == Some(Format::Code39) {
                region.format = variant;
            }
        }
        barcodes.retain(|barcode| {
            let keep = barcode.format != Format::Code39;
            if !keep {
                undecoded.push(UndecodedRegion {
                    format: variant,
                    polygon: barcode.polygon,
                });
            }
            keep
        });
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

/// Extended Code 39: each `$`, `%`, `/` or `+` with the following letter stands for
/// one ASCII character (ISO/IEC 16388 full-ASCII table). `None` when a shift
/// character is not followed by a letter of its pair set.
fn full_ascii(content: &str) -> Option<String> {
    let mut out = String::with_capacity(content.len());
    let mut chars = content.bytes();
    while let Some(c) = chars.next() {
        let code = match c {
            b'$' | b'%' | b'/' | b'+' => {
                let letter = chars.next().filter(u8::is_ascii_uppercase)?;
                let k = letter - b'A';
                match c {
                    // $A-$Z: control characters 1-26.
                    b'$' => k + 1,
                    // +A-+Z: lower-case letters.
                    b'+' => b'a' + k,
                    // /A-/O: ! " # $ % & ' ( ) * + , - . /; /Z: colon.
                    b'/' if k < 15 => b'!' + k,
                    b'/' if letter == b'Z' => b':',
                    b'/' => return None,
                    // %A-%E: control characters 27-31; %F-%J: ; < = > ?;
                    // %K-%O: [ \ ] ^ _; %P-%T: { | } ~ DEL; %U: NUL; %V: @;
                    // %W: `; %X-%Z: DEL.
                    _ => match letter {
                        b'A'..=b'E' => 27 + k,
                        b'F'..=b'J' => b';' + k - 5,
                        b'K'..=b'O' => b'[' + k - 10,
                        b'P'..=b'T' => b'{' + k - 15,
                        b'U' => 0,
                        b'V' => b'@',
                        b'W' => b'`',
                        _ => 127,
                    },
                }
            }
            other => other,
        };
        out.push(char::from(code));
    }
    Some(out)
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
    fn full_ascii_expands_every_pair_set() {
        assert_eq!(full_ascii("+H+E+L+L+O").as_deref(), Some("hello"));
        assert_eq!(full_ascii("$I%U%V%W%T").as_deref(), Some("\t\0@`\x7f"));
        assert_eq!(
            full_ascii("/A/O/Z%F%J%K%O%P%S").as_deref(),
            Some("!/:;?[_{~")
        );
        assert_eq!(full_ascii("%A%E").as_deref(), Some("\x1b\x1f"));
        assert_eq!(full_ascii("AB-12. ").as_deref(), Some("AB-12. "));
        for invalid in ["$", "+1", "/P", "%-", "A$"] {
            assert_eq!(full_ascii(invalid), None, "{invalid}");
        }
        // Every ASCII character is a plain Code 39 character or has a pair (`-`, `.`
        // and DEL have two encodings).
        let plain = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. ";
        let mut seen = std::collections::HashSet::new();
        for shift in ['$', '%', '/', '+'] {
            for letter in 'A'..='Z' {
                if let Some(text) = full_ascii(&format!("{shift}{letter}")) {
                    let c = text.chars().next().unwrap();
                    assert!(
                        !plain.contains(c) || c == '-' || c == '.',
                        "{shift}{letter}"
                    );
                    seen.insert(c);
                }
            }
        }
        assert!(
            (0..128_u8).all(|b| plain.contains(char::from(b)) || seen.contains(&char::from(b)))
        );
    }

    #[test]
    fn extended_code39_reads_need_shifts_when_code39_is_selected() {
        let ext = Format::Code39Extended as u32;
        let code39 = Format::Code39 as u32;
        let mut reads = vec![barcode("+H+I"), barcode("HI"), barcode("$1")];
        apply(ext, &mut reads, &mut Vec::new());
        let got: Vec<_> = reads.iter().map(|b| (b.format, b.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (Format::Code39Extended, "hi"),
                (Format::Code39Extended, "HI")
            ]
        );
        assert_eq!(reads[0].payload_bytes.as_deref(), Some(&b"+H+I"[..]));
        let mut reads = vec![barcode("+H+I"), barcode("HI"), barcode("$1")];
        apply(ext | code39, &mut reads, &mut Vec::new());
        let got: Vec<_> = reads.iter().map(|b| (b.format, b.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (Format::Code39Extended, "hi"),
                (Format::Code39, "HI"),
                (Format::Code39, "$1")
            ]
        );
        assert_eq!(engine_bits(ext), code39);
    }

    #[test]
    fn apply_converts_selected_variants_and_keeps_raw_bytes() {
        let code32 = Format::Code32 as u32;
        let pzn = Format::Pzn as u32;
        let code39 = Format::Code39 as u32;
        let mut reads = vec![barcode("0CSSBD"), barcode("-02532876"), barcode("HELLO")];
        apply(code32 | pzn, &mut reads, &mut Vec::new());
        let got: Vec<_> = reads.iter().map(|b| (b.format, b.text.as_str())).collect();
        assert_eq!(
            got,
            [(Format::Code32, "012345676"), (Format::Pzn, "02532876")]
        );
        assert_eq!(reads[0].payload_bytes.as_deref(), Some(&b"0CSSBD"[..]));

        let mut reads = vec![barcode("0CSSBD"), barcode("-02532876"), barcode("HELLO")];
        apply(pzn | code39, &mut reads, &mut Vec::new());
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
        apply(code39, &mut reads, &mut Vec::new());
        assert_eq!(reads[0].format, Format::Code39);
        assert_eq!(engine_bits(code32), code39);
        assert_eq!(engine_bits(code39 | 1), code39 | 1);
    }

    #[test]
    fn undecoded_regions_name_only_selected_formats() {
        let code32 = Format::Code32 as u32;
        let pzn = Format::Pzn as u32;
        let code39 = Format::Code39 as u32;
        let region = |format| UndecodedRegion {
            format: Some(format),
            polygon: [[0., 0.], [10., 0.], [10., 5.], [0., 5.]],
        };
        let formats = |regions: &[UndecodedRegion]| -> Vec<_> {
            regions.iter().map(|region| region.format).collect()
        };
        // A rejected Code 39 read and an unread Code 39 region become regions of
        // the one selected variant; other formats keep theirs.
        let mut reads = vec![barcode("HELLO")];
        let mut regions = vec![region(Format::Code39), region(Format::Code128)];
        apply(pzn | Format::Code128 as u32, &mut reads, &mut regions);
        assert_eq!(reads.len(), 0);
        assert_eq!(
            formats(&regions),
            [Some(Format::Pzn), Some(Format::Code128), Some(Format::Pzn)]
        );
        // With both variants it is unknown which one a region would have been.
        let mut reads = vec![barcode("HELLO")];
        let mut regions = vec![region(Format::Code39)];
        apply(code32 | pzn, &mut reads, &mut regions);
        assert_eq!(formats(&regions), [None, None]);
        // Selecting Code 39 keeps its reads and regions.
        let mut reads = vec![barcode("HELLO")];
        let mut regions = vec![region(Format::Code39)];
        apply(pzn | code39, &mut reads, &mut regions);
        assert_eq!(reads.len(), 1);
        assert_eq!(formats(&regions), [Some(Format::Code39)]);
    }
}

//! ECI character-set conversion only; `encoding_rs` supplies text codecs, not
//! barcode decoding, localization, bit parsing or error correction.
pub fn decode(bytes: &[u8], eci: Option<usize>) -> Option<String> {
    let encoding = match eci {
        Some(0 | 2) => {
            return bytes
                .iter()
                .map(|&b| {
                    char::from_u32(if b < 128 {
                        u32::from(b)
                    } else {
                        u32::from(crate::encoding_tables::CP437[b as usize - 128])
                    })
                })
                .collect()
        }
        Some(1 | 3 | 899) => return Some(bytes.iter().map(|&b| b as char).collect()),
        Some(4) => encoding_rs::ISO_8859_2,
        Some(5) => encoding_rs::ISO_8859_3,
        Some(6) => encoding_rs::ISO_8859_4,
        Some(7) => encoding_rs::ISO_8859_5,
        Some(8) => encoding_rs::ISO_8859_6,
        Some(9) => encoding_rs::ISO_8859_7,
        Some(10) => encoding_rs::ISO_8859_8,
        Some(11) => encoding_rs::WINDOWS_1254,
        Some(12) => encoding_rs::ISO_8859_10,
        Some(13) => encoding_rs::WINDOWS_874,
        Some(15) => encoding_rs::ISO_8859_13,
        Some(16) => encoding_rs::ISO_8859_14,
        Some(17) => encoding_rs::ISO_8859_15,
        Some(18) => encoding_rs::ISO_8859_16,
        Some(20) => encoding_rs::SHIFT_JIS,
        Some(21) => encoding_rs::WINDOWS_1250,
        Some(22) => encoding_rs::WINDOWS_1251,
        Some(23) => encoding_rs::WINDOWS_1252,
        Some(24) => encoding_rs::WINDOWS_1256,
        Some(25) => encoding_rs::UTF_16BE,
        Some(26) => return String::from_utf8(bytes.to_vec()).ok(),
        Some(27 | 170) => {
            return bytes
                .iter()
                .all(u8::is_ascii)
                .then(|| bytes.iter().map(|&b| b as char).collect())
        }
        Some(28) => encoding_rs::BIG5,
        Some(29 | 31) => encoding_rs::GBK,
        Some(30) => encoding_rs::EUC_KR,
        Some(32) => encoding_rs::GB18030,
        Some(33) => encoding_rs::UTF_16LE,
        Some(34 | 35) => {
            if !bytes.len().is_multiple_of(4) {
                return None;
            }
            return bytes
                .chunks_exact(4)
                .map(|b| {
                    char::from_u32(if eci == Some(34) {
                        u32::from_be_bytes(b.try_into().ok()?)
                    } else {
                        u32::from_le_bytes(b.try_into().ok()?)
                    })
                })
                .collect();
        }
        None => {
            if let Ok(text) = std::str::from_utf8(bytes) {
                return Some(text.into());
            }
            let (text, _, errors) = encoding_rs::SHIFT_JIS.decode(bytes);
            if !errors {
                return Some(text.into_owned());
            }
            return Some(bytes.iter().map(|&b| b as char).collect());
        }
        _ => return None,
    };
    // ISO 8859-9/11 use their Windows superset codec above, but retain the
    // ISO C1 controls instead of Windows punctuation in 0x80..0x9f.
    if matches!(eci, Some(11 | 13)) {
        let mut text = String::new();
        for &b in bytes {
            if (0x80..=0x9f).contains(&b) {
                text.push(b as char);
            } else {
                let one = [b];
                let (part, _, errors) = encoding.decode(&one);
                if errors {
                    return None;
                }
                text.push_str(&part);
            }
        }
        return Some(text);
    }
    let (text, _, errors) = encoding.decode(bytes);
    (!errors).then(|| text.into_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn explicit_cp437_preserves_controls_and_maps_upper_bytes() {
        for eci in [0, 2] {
            assert_eq!(
                super::decode(&[0, 0x7f, 0x80, 0xe1, 0xff], Some(eci)).as_deref(),
                Some("\0\u{7f}Çß\u{a0}")
            );
        }
        assert_eq!(super::decode(&[0xe1], Some(3)).as_deref(), Some("á"));
        assert!(super::decode(&[0xe1], Some(900)).is_none());
    }
}

//! Independent `MaxiCode` module extraction, GF(64) correction and payload parser.
//! The standard placement/character constants are attributed separately.
use crate::{
    maxicode_tables::{CHARS, GRID},
    qr::Payload,
    reed_binary::Field,
    StructuredAppend,
};

#[must_use]
pub fn decode_matrix(matrix: &[bool]) -> Option<Payload> {
    decode_words(&mut codewords(matrix)?)
}
fn codewords(matrix: &[bool]) -> Option<[u16; 144]> {
    if matrix.len() != 990 {
        return None;
    }
    let mut words = [0u16; 144];
    for (&bit, &position) in matrix.iter().zip(GRID.iter()) {
        if position != 0 && bit {
            let index = position as usize - 1;
            words[index / 6] |= 1 << (5 - index % 6);
        }
    }
    Some(words)
}

#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_words(matrix: &[bool]) -> serde_json::Value {
    let Some(mut words) = codewords(matrix) else {
        return serde_json::json!({"stage":"size"});
    };
    let field = Field::new(6, 0x43);
    let primary = field.correct(&mut words[..20], 10, 1);
    if primary.is_none() {
        return serde_json::json!({"stage":"primary"});
    }
    let mode = words[0] & 15;
    let ecc = if mode == 5 { 28 } else { 20 };
    for offset in 0..2 {
        let mut block: Vec<_> = (20 + offset..144).step_by(2).map(|i| words[i]).collect();
        if field.correct(&mut block, ecc, 1).is_none() {
            return serde_json::json!({"stage":"secondary","mode":mode,"offset":offset});
        }
        for (i, value) in (20 + offset..144).step_by(2).zip(block) {
            words[i] = value;
        }
    }
    serde_json::json!({"stage":"corrected-codewords","mode":mode,"words":words.to_vec()})
}

fn decode_words(words: &mut [u16; 144]) -> Option<Payload> {
    let field = Field::new(6, 0x43);
    let mut corrected = field.correct(&mut words[..20], 10, 1)?;
    let mode = words[0] & 15;
    if !(2..=6).contains(&mode) || (mode >= 4 && words[0] != mode) {
        return None;
    }
    let ecc = if mode == 5 { 28 } else { 20 };
    for offset in 0..2 {
        let mut block: Vec<_> = (20 + offset..144).step_by(2).map(|i| words[i]).collect();
        corrected += field.correct(&mut block, ecc, 1)?;
        for (i, value) in (20 + offset..144).step_by(2).zip(block) {
            words[i] = value;
        }
    }
    let mut data = Vec::new();
    if mode >= 4 {
        data.extend_from_slice(&words[1..10]);
    }
    data.extend_from_slice(&words[20..144 - 2 * ecc]);
    let mut result = parse(&data)?;
    if mode <= 3 {
        let postal = if mode == 2 {
            let length = ((words[5] >> 4) | ((words[6] & 15) << 2)) as usize;
            if !(1..=9).contains(&length) {
                return None;
            }
            let number = (u32::from(words[0]) >> 4)
                | (u32::from(words[1]) << 2)
                | (u32::from(words[2]) << 8)
                | (u32::from(words[3]) << 14)
                | (u32::from(words[4]) << 20)
                | (u32::from(words[5] & 15) << 26);
            let value = format!("{number:0length$}");
            if value.len() != length {
                return None;
            }
            value
        } else {
            let mut value = String::new();
            for i in (0..6).rev() {
                let symbol = (words[i] >> 4) | ((words[i + 1] & 15) << 2);
                let byte = CHARS[0][symbol as usize];
                if byte < 32 {
                    return None;
                }
                value.push((byte).to_le_bytes()[0] as char);
            }
            value.trim_end_matches(' ').to_owned()
        };
        let country = (words[6] >> 4) | (words[7] << 2) | ((words[8] & 3) << 8);
        let service = (words[8] >> 2) | (words[9] << 4);
        if country > 999 || service > 999 {
            return None;
        }
        let prefix = format!("{postal}\x1d{country:03}\x1d{service:03}\x1d");
        // Insert the structured primary after an existing ISO 15434 SCM header.
        let position = if result.bytes.starts_with(b"[)>\x1e01\x1d")
            && result.bytes.len() >= 9
            && result.bytes[7..9].iter().all(u8::is_ascii_digit)
        {
            9
        } else {
            0
        };
        result.bytes.splice(position..position, prefix.bytes());
        result.text.insert_str(position, &prefix);
    }
    result.corrected = corrected;
    result.reader_initialization = mode == 6;
    Some(result)
}

#[expect(
    clippy::too_many_lines,
    reason = "The MaxiCode table dispatcher shares shift/latch and structured-append state for a single payload cursor."
)]
fn parse(data: &[u16]) -> Option<Payload> {
    let mut bytes = Vec::new();
    let mut text = String::new();
    let mut pending = Vec::new();
    let mut eci = 3usize;
    let mut state = 0usize;
    let mut shift: Option<(usize, usize)> = None;
    let mut index = 0;
    let mut structured_append = None;
    if data.first() == Some(&33) {
        let info = *data.get(1)? as usize;
        let count = (info & 7) + 1;
        let sequence_index = (info >> 3) + 1;
        if count < 2 || sequence_index > count {
            return None;
        }
        structured_append = Some(StructuredAppend {
            index: sequence_index,
            count,
            id: None,
            parity: None,
        });
        index = 2;
    }
    while index < data.len() {
        let code = data[index] as usize;
        index += 1;
        if code >= 64 {
            return None;
        }
        let active = shift.map_or(state, |(s, _)| s);
        if let Some((s, remaining)) = shift {
            shift = if remaining > 1 {
                Some((s, remaining - 1))
            } else {
                None
            };
        }
        let value = CHARS[active][code];
        if value >= 0 {
            bytes.push((value).to_le_bytes()[0]);
            pending.push((value).to_le_bytes()[0]);
            continue;
        }
        match code {
            27 => {
                text.push_str(&crate::encoding::decode(&pending, Some(eci))?);
                pending.clear();
                let first = *data.get(index)? as usize;
                index += 1;
                let (mut value, count) = if first < 32 {
                    (first, 0)
                } else if first < 48 {
                    (first & 15, 1)
                } else if first < 56 {
                    (first & 7, 2)
                } else if first < 60 {
                    (first & 3, 3)
                } else {
                    return None;
                };
                for _ in 0..count {
                    value = (value << 6) | *data.get(index)? as usize;
                    index += 1;
                }
                if value > 999_999 {
                    return None;
                }
                eci = value;
            }
            31 => {
                let mut number = 0u32;
                for _ in 0..5 {
                    number = (number << 6) | u32::from(*data.get(index)?);
                    index += 1;
                }
                if number > 999_999_999 {
                    return None;
                }
                let digits = format!("{number:09}");
                bytes.extend_from_slice(digits.as_bytes());
                pending.extend_from_slice(digits.as_bytes());
            }
            28 | 33 => {
                // PAD is terminal; do not trim spaces or other real payload bytes.
                if data[index..].iter().any(|&c| c as usize != code) {
                    return None;
                }
                break;
            }
            56 | 57 if active == 1 => shift = Some((0, code - 54)),
            58 if active >= 2 => {
                state = 0;
                shift = None;
            }
            59 if active <= 1 => shift = Some((1 - active, 1)),
            60..=62 => {
                let target = match code {
                    60 => 3,
                    61 => 4,
                    _ => 2,
                };
                if data.get(index) == Some(&crate::numeric::usize_u16(code)) {
                    index += 1;
                    state = target;
                    shift = None;
                } else {
                    shift = Some((target, 1));
                }
            }
            63 => {
                state = usize::from(active != 1);
                shift = None;
            }
            _ => return None,
        }
    }
    if shift.is_some() {
        return None;
    }
    text.push_str(&crate::encoding::decode(&pending, Some(eci))?);
    Some(Payload {
        bytes,
        text,
        corrected: 0,
        gs1: false,
        structured_append,
        reader_initialization: false,
    })
}

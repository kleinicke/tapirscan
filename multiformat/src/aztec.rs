//! Independent Aztec compact/full matrix decoder, including mode-message ECC.
use crate::{qr::Payload, reed_binary::Field};
fn value(bits: &[bool]) -> u16 {
    bits.iter().fold(0, |v, &b| (v << 1) | u16::from(b))
}
// The twelve asymmetric corner modules establish reading orientation before
// mode-message correction. An all-white ring is not a compact descriptor.
pub(crate) fn orientation_valid(
    matrix: &[bool],
    n: usize,
    compact: bool,
    turn: usize,
    mirror: bool,
) -> bool {
    let radius = if compact { 5_isize } else { 7 };
    if n < 2 * (radius).cast_unsigned() + 1 || matrix.len() != n * n {
        return false;
    }
    let center = (n / 2).cast_signed();
    let mut errors = 0;
    for (sx, sy, expected) in [
        (-1, -1, [true, true, true]),
        (1, -1, [true, false, true]),
        (1, 1, [false, false, true]),
        (-1, 1, [false, false, false]),
    ] {
        let corner = [sx * radius, sy * radius];
        for ([x, y], expect) in [
            corner,
            [corner[0] - sx, corner[1]],
            [corner[0], corner[1] - sy],
        ]
        .into_iter()
        .zip(expected)
        {
            let (mut x, mut y) = ((center + x).cast_unsigned(), (center + y).cast_unsigned());
            if mirror {
                std::mem::swap(&mut x, &mut y);
            }
            for _ in 0..turn {
                (x, y) = (n - 1 - y, x);
            }
            errors += usize::from(matrix[y * n + x] != expect);
            if errors > 2 {
                return false;
            }
        }
    }
    true
}
fn finder_valid(matrix: &[bool], n: usize, compact: bool) -> bool {
    if !orientation_valid(matrix, n, compact, 0, false) {
        return false;
    }
    let radius = if compact { 5_isize } else { 7 };
    let center = (n / 2).cast_signed();
    let mut bull_errors = 0;
    let inner = radius - 1;
    for y in -inner..=inner {
        for x in -inner..=inner {
            bull_errors += usize::from(
                matrix[(center + y).cast_unsigned() * n + (center + x).cast_unsigned()]
                    != (x.abs().max(y.abs()) % 2 == 0),
            );
        }
    }
    bull_errors * 10 <= (2 * inner + 1).pow(2) as usize
}
fn mode_words(matrix: &[bool], dimension: usize, compact: bool) -> Option<Vec<u16>> {
    if !finder_valid(matrix, dimension, compact) {
        return None;
    }
    let center = dimension / 2;
    let radius = if compact { 5 } else { 7 };
    let count = if compact { 7 } else { 10 };
    let mut mode = Vec::new();
    for side in 0..4 {
        for i in 0..count {
            let offset = if compact {
                i as isize - 3
            } else {
                i as isize + i as isize / 5 - 5
            };
            let c = (center).cast_signed();
            let r = radius as isize;
            let (x, y) = match side {
                0 => (c + offset, c - r),
                1 => (c + r, c + offset),
                2 => (c - offset, c + r),
                _ => (c - r, c - offset),
            };
            mode.push(matrix[(y).cast_unsigned() * dimension + (x).cast_unsigned()]);
        }
    }
    Some(mode.chunks_exact(4).map(value).collect())
}
/// Aztec Rune: one byte, protected by five GF(16) parity words and the
/// alternating-bit mask specified in ISO/IEC 24778 Annex A.
#[must_use]
pub fn decode_rune(matrix: &[bool], n: usize) -> Option<Payload> {
    if n != 11 {
        return None;
    }
    let mut words = mode_words(matrix, n, true)?;
    for word in &mut words {
        *word ^= 0xA;
    }
    let corrected = Field::new(4, 0x13).correct(&mut words, 5, 1)?;
    let value = (words[0] << 4) | words[1];
    let text = format!("{value:03}");
    Some(Payload {
        structured_append: None,
        reader_initialization: false,
        bytes: text.as_bytes().to_vec(),
        text,
        corrected,
        gs1: false,
    })
}
pub struct Mode {
    pub layers: usize,
    pub data_words: usize,
    pub reader_initialization: bool,
}
#[must_use]
pub fn read_mode(matrix: &[bool], n: usize, compact: bool) -> Option<Mode> {
    let mut words = mode_words(matrix, n, compact)?;
    let data = if compact { 2 } else { 4 };
    let length = words.len();
    Field::new(4, 0x13).correct(&mut words, length - data, 1)?;
    let info = words[..data]
        .iter()
        .fold(0usize, |v, &b| (v << 4) | b as usize);
    let layers = (info >> if compact { 6 } else { 11 }) + 1;
    let mut count = info & if compact { 63 } else { 2047 };
    let word_bits = match layers {
        1..=2 => 6,
        3..=8 => 8,
        9..=22 => 10,
        _ => 12,
    };
    let capacity = layers * (16 * layers + if compact { 88 } else { 112 }) / word_bits;
    let flag = if compact { 32 } else { 1024 };
    // Reader initialization uses the high count bit only at sizes where that
    // bit cannot describe an ordinary data-word count (compact layer 1 / full
    // layers <=22). Larger symbols retain that bit as ordinary count data.
    let reader_initialization = count & flag != 0 && capacity <= flag;
    if reader_initialization {
        count &= !flag;
    }
    let data_words = count + 1;
    if layers > if compact { 4 } else { 32 } {
        return None;
    }
    Some(Mode {
        layers,
        data_words,
        reader_initialization,
    })
}
pub fn decode_matrix(matrix: &[bool], n: usize) -> Option<Payload> {
    if n == 11 {
        return decode_rune(matrix, n);
    }
    if !(15..=151).contains(&n) || matrix.len() != n * n || n.is_multiple_of(2) {
        return None;
    }
    for compact in [true, false] {
        let Some(mode) = read_mode(matrix, n, compact) else {
            continue;
        };
        let layers = mode.layers;
        let data_words = mode.data_words;
        let base = layers * 4 + if compact { 11 } else { 14 };
        let expected = if compact {
            base
        } else {
            base + 1 + 2 * ((base / 2 - 1) / 15)
        };
        if expected != n {
            continue;
        }
        let map: Vec<usize> = if compact {
            (0..base).collect()
        } else {
            let mut m = vec![0; base];
            for i in 0..base / 2 {
                let offset = i + i / 15;
                m[base / 2 - i - 1] = n / 2 - offset - 1;
                m[base / 2 + i] = n / 2 + offset + 1;
            }
            m
        };
        let mut raw = Vec::new();
        for layer in 0..layers {
            let size = (layers - layer) * 4 + if compact { 9 } else { 12 };
            let low = layer * 2;
            let high = base - 1 - low;
            let mut ring = vec![false; size * 8];
            for j in 0..size {
                for k in 0..2 {
                    ring[j * 2 + k] = matrix[map[low + j] * n + map[low + k]];
                    ring[size * 2 + j * 2 + k] = matrix[map[high - k] * n + map[low + j]];
                    ring[size * 4 + j * 2 + k] = matrix[map[high - j] * n + map[high - k]];
                    ring[size * 6 + j * 2 + k] = matrix[map[low + k] * n + map[high - j]];
                }
            }
            raw.extend(ring);
        }
        let (word_bits, poly) = match layers {
            1..=2 => (6, 0x43),
            3..=8 => (8, 0x12d),
            9..=22 => (10, 0x409),
            _ => (12, 0x1069),
        };
        let mut words: Vec<u16> = raw[raw.len() % word_bits..]
            .chunks_exact(word_bits)
            .map(value)
            .collect();
        if data_words >= words.len() {
            continue;
        }
        let ecc = words.len() - data_words;
        let Some(corrected) = Field::new(word_bits, poly).correct(&mut words, ecc, 1) else {
            continue;
        };
        let max = (1u16 << word_bits) - 1;
        let mut bits = Vec::new();
        let mut valid = true;
        for &word in &words[..data_words] {
            if word == 0 || word == max {
                valid = false;
                break;
            }
            if word == 1 || word == max - 1 {
                bits.extend(std::iter::repeat_n(word > 1, word_bits - 1));
            } else {
                for k in (0..word_bits).rev() {
                    bits.push((word >> k) & 1 != 0);
                }
            }
        }
        if !valid {
            continue;
        }
        if let Some((bytes, text, gs1, structured_append)) = parse(&bits) {
            return Some(Payload {
                structured_append,
                reader_initialization: mode.reader_initialization,
                bytes,
                text,
                corrected,
                gs1,
            });
        }
    }
    None
}
struct Bits<'a> {
    bits: &'a [bool],
    at: usize,
}
impl Bits<'_> {
    fn read(&mut self, n: usize) -> Option<usize> {
        if self.at + n > self.bits.len() {
            return None;
        }
        let v = value(&self.bits[self.at..self.at + n]) as usize;
        self.at += n;
        Some(v)
    }
}
#[expect(
    clippy::too_many_lines,
    reason = "The Aztec control-table state machine keeps latch, shift, ECI and binary transitions in one audited dispatch loop."
)]
fn parse(bits: &[bool]) -> Option<(Vec<u8>, String, bool, Option<crate::StructuredAppend>)> {
    let sequence_header = bits.len() >= 10 && value(&bits[..5]) == 29 && value(&bits[5..10]) == 29;
    let mut reader = Bits { bits, at: 0 };
    let mut table = 0;
    let mut latch = 0;
    let mut bytes = Vec::new();
    let mut gs1 = false;
    let mut eci_encoding = 3;
    let mut segment_start = 0;
    let mut text = String::new();
    while reader.at < bits.len() {
        if table == 5 {
            let Some(mut count) = reader.read(5) else {
                break;
            };
            if count == 0 {
                let Some(extended) = reader.read(11) else {
                    break;
                };
                count = extended + 31;
            }
            // The descriptor defines the data-bit boundary. Trailing binary
            // shift padding may announce bytes beyond that boundary; consume
            // only complete bytes and never manufacture missing data.
            let available = (bits.len() - reader.at) / 8;
            for _ in 0..count.min(available) {
                bytes.push((reader.read(8)?).to_le_bytes()[0]);
            }
            if count > available {
                break;
            }
            table = latch;
            continue;
        }
        let Some(c) = reader.read(if table == 4 { 4 } else { 5 }) else {
            break;
        };
        let mut control = None;
        let mut chars: Vec<u8> = Vec::new();
        match table {
            0 | 1 => match c {
                0 => control = Some((3, false)),
                1 => chars.push(b' '),
                2..=27 => chars.push((if table == 0 { b'A' } else { b'a' }) + (c - 2).to_le_bytes()[0]),
                28 => control = Some(if table == 0 { (1, true) } else { (0, false) }),
                29 => control = Some((2, true)),
                30 => control = Some((4, true)),
                31 => control = Some((5, false)),
                _ => return None,
            },
            2 => match c {
                0 => control = Some((3, false)),
                1..=27 => chars.push(b" \x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x1b\x1c\x1d\x1e\x1f@\\^_`|~\x7f"[c - 1]),
                28 => control = Some((1, true)),
                29 => control = Some((0, true)),
                30 => control = Some((3, true)),
                31 => control = Some((5, false)),
                _ => return None,
            },
            3 => match c {
                0 => {
                    let n = reader.read(3)?;
                    if n == 0 {
                        gs1 = true;
                        if !bytes.is_empty() {
                            bytes.push(29);
                        }
                    } else if n <= 6 {
                        let mut eci = 0;
                        for _ in 0..n {
                            let digit = reader.read(4)?;
                            if !(2..=11).contains(&digit) {
                                return None;
                            }
                            eci = eci * 10 + digit - 2;
                        }
                        text.push_str(&crate::encoding::decode(&bytes[segment_start..], Some(eci_encoding))?);
                        segment_start = bytes.len();
                        eci_encoding = eci;
                    } else {
                        return None;
                    }
                }
                1 => chars.push(13),
                2 => chars.extend([13, 10]),
                3 => chars.extend(b". "),
                4 => chars.extend(b", "),
                5 => chars.extend(b": "),
                6..=30 => chars.push(b"!\"#$%&'()*+,-./:;<=>?[]{}"[c - 6]),
                31 => control = Some((0, true)),
                _ => return None,
            },
            4 => match c {
                0 => control = Some((3, false)),
                1 => chars.push(b' '),
                2..=11 => chars.push(b'0' + (c - 2).to_le_bytes()[0]),
                12 => chars.push(b','),
                13 => chars.push(b'.'),
                14 => control = Some((0, true)),
                15 => control = Some((0, false)),
                _ => return None,
            },
            _ => return None,
        }
        if let Some((next, permanent)) = control {
            // A second control consumes the previous single-symbol shift.
            // In particular DIGIT -> UPPER shift -> BINARY shift returns to
            // UPPER after the bytes, rather than the earlier DIGIT latch.
            latch = if permanent { next } else { table };
            table = next;
        } else {
            bytes.extend(chars);
            table = latch;
        }
    }
    if bytes.is_empty() {
        return None;
    }
    text.push_str(&crate::encoding::decode(
        &bytes[segment_start..],
        Some(eci_encoding),
    )?);
    let structured_append = if sequence_header {
        let mut at = 0;
        let id = if bytes.first() == Some(&b' ') {
            let end = bytes[1..].iter().position(|&b| b == b' ')? + 1;
            let id = crate::encoding::decode(&bytes[1..end], Some(3))?;
            at = end + 1;
            Some(id)
        } else {
            None
        };
        let index = *bytes.get(at)?;
        let count = *bytes.get(at + 1)?;
        if !index.is_ascii_uppercase() || !(b'B'..=b'Z').contains(&count) || index > count {
            return None;
        }
        at += 2;
        let header = crate::encoding::decode(&bytes[..at], Some(3))?;
        text = text.strip_prefix(&header)?.to_owned();
        bytes.drain(..at);
        Some(crate::StructuredAppend {
            index: (index - b'A' + 1) as usize,
            count: (count - b'A' + 1) as usize,
            id,
            parity: None,
        })
    } else {
        None
    };
    Some((bytes, text, gs1, structured_append))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bits(values: &[(usize, usize)]) -> Vec<bool> {
        values
            .iter()
            .flat_map(|&(v, n)| (0..n).rev().map(move |i| (v >> i) & 1 != 0))
            .collect()
    }
    #[test]
    fn nested_binary_shift_restores_the_shifted_upper_table() {
        let input = bits(&[
            (30, 5),
            (5, 4),
            (5, 4),
            (5, 4),
            (5, 4),
            (15, 4),
            (31, 5),
            (1, 5),
            (b'h' as usize, 8),
            (10, 5),
            (21, 5),
        ]);
        assert_eq!(parse(&input).unwrap().1, "3333hIT");
    }
    #[test]
    fn trailing_binary_padding_stops_at_data_boundary() {
        let input = bits(&[(2, 5), (31, 5), (31, 5), (b'a' as usize, 8)]);
        assert_eq!(parse(&input).unwrap().1, "Aa");
        let input = bits(&[(2, 5), (31, 5), (0, 5)]);
        assert_eq!(parse(&input).unwrap().1, "A");
    }
    #[test]
    fn structured_append_header_is_metadata_not_payload() {
        let header = b" Z1.txt DG3456";
        let mut values = vec![(29, 5), (29, 5), (31, 5), (header.len(), 5)];
        values.extend(header.iter().map(|&b| (b as usize, 8)));
        let (bytes, text, _, sequence) = parse(&bits(&values)).unwrap();
        assert_eq!(bytes, b"3456");
        assert_eq!(text, "3456");
        assert_eq!(
            sequence,
            Some(crate::StructuredAppend {
                index: 4,
                count: 7,
                id: Some("Z1.txt".into()),
                parity: None
            })
        );
        assert!(parse(&bits(&[(29, 5), (29, 5), (27, 5), (3, 5), (2, 5)])).is_none());
    }
}

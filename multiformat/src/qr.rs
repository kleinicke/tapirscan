//! Independent QR Model 2 matrix decoder and finder-based localizer.
use crate::qr_tables::{BLOCKS, ECC};
use crate::reed_solomon::Field;

#[derive(Debug)]
pub struct Payload {
    pub structured_append: Option<crate::StructuredAppend>,
    pub reader_initialization: bool,
    pub bytes: Vec<u8>,
    pub text: String,
    pub corrected: usize,
    pub gs1: bool,
}
type Coordinates = [(usize, usize); 15];
fn format_coordinates(n: usize) -> (Coordinates, Coordinates) {
    let a = [
        (8, 0),
        (8, 1),
        (8, 2),
        (8, 3),
        (8, 4),
        (8, 5),
        (8, 7),
        (8, 8),
        (7, 8),
        (5, 8),
        (4, 8),
        (3, 8),
        (2, 8),
        (1, 8),
        (0, 8),
    ];
    let b = [
        (n - 1, 8),
        (n - 2, 8),
        (n - 3, 8),
        (n - 4, 8),
        (n - 5, 8),
        (n - 6, 8),
        (n - 7, 8),
        (n - 8, 8),
        (8, n - 7),
        (8, n - 6),
        (8, n - 5),
        (8, n - 4),
        (8, n - 3),
        (8, n - 2),
        (8, n - 1),
    ];
    (a, b)
}
const FORMAT_CODES: [u16; 32] = {
    let mut codes = [0; 32];
    let mut data = 0_u16;
    while data < 32 {
        let mut remainder = data;
        let mut bit = 0;
        while bit < 10 {
            remainder = (remainder << 1) ^ if remainder & 512 != 0 { 0x537 } else { 0 };
            bit += 1;
        }
        codes[data as usize] = ((data << 10) | remainder) ^ 0x5412;
        data += 1;
    }
    codes
};
fn format_code(data: u16) -> u16 {
    FORMAT_CODES[usize::from(data)]
}

/// Cheap geometric header screening before allocating and sampling a full grid.
/// Both mirror orientations remain eligible. Timing damage is tolerated, but
/// random cross-symbol triples should not consume full payload decoding work.
pub fn plausible_image_header(
    n: usize,
    mut read: impl FnMut(usize, usize) -> Option<bool>,
) -> bool {
    let (a, b) = format_coordinates(n);
    let mut formats = [0u16; 4];
    for (index, coordinates) in [a, b].iter().enumerate() {
        for (bit, &(x, y)) in coordinates.iter().enumerate() {
            let (Some(normal), Some(mirrored)) = (read(x, y), read(y, x)) else {
                return false;
            };
            formats[index] |= u16::from(normal) << bit;
            formats[index + 2] |= u16::from(mirrored) << bit;
        }
    }
    if !(0..32).any(|data| {
        let code = format_code(data);
        formats
            .iter()
            .any(|&value| (value ^ code).count_ones() <= 3)
    }) {
        return false;
    }
    let mut errors = 0;
    let mut total = 0;
    for i in 8..n - 8 {
        for (x, y) in [(6, i), (i, 6)] {
            total += 1;
            errors += usize::from(read(x, y) != Some(i % 2 == 0));
        }
    }
    errors * 5 <= total * 2
}
#[must_use]
pub fn alignment_positions(version: usize) -> Vec<usize> {
    if version == 1 {
        return vec![];
    }
    let n = version / 7 + 2;
    let step = if version == 32 {
        26
    } else {
        (version * 4 + n * 2 + 1) / (n * 2 - 2) * 2
    };
    let mut p = vec![6];
    for i in (0..n - 1).rev() {
        p.push(version * 4 + 10 - i * step);
    }
    p
}
fn masked(mask: usize, x: usize, y: usize) -> bool {
    match mask {
        0 => (x + y).is_multiple_of(2),
        1 => y.is_multiple_of(2),
        2 => x.is_multiple_of(3),
        3 => (x + y).is_multiple_of(3),
        4 => (x / 3 + y / 2).is_multiple_of(2),
        5 => (x * y) % 2 + (x * y) % 3 == 0,
        6 => ((x * y) % 2 + (x * y) % 3).is_multiple_of(2),
        _ => ((x + y) % 2 + (x * y) % 3).is_multiple_of(2),
    }
}

// At most forty immutable geometry layouts, initialized only when used.
// Packed (cell index, eight mask flags) entries occupy under 2 MiB in total.
static LAYOUTS: [std::sync::OnceLock<Vec<(u16, u8)>>; 40] =
    [const { std::sync::OnceLock::new() }; 40];
fn layout(version: usize) -> Vec<(u16, u8)> {
    let n = 17 + version * 4;
    let (a, b) = format_coordinates(n);
    let mut function = vec![false; n * n];
    for y in 0..n {
        for x in 0..n {
            if ((x <= 8 || x >= n - 8) && y <= 8) || (x <= 8 && y >= n - 8) || x == 6 || y == 6 {
                function[y * n + x] = true;
            }
        }
    }
    let positions = alignment_positions(version);
    for (i, &x) in positions.iter().enumerate() {
        for (j, &y) in positions.iter().enumerate() {
            if (i == 0 && (j == 0 || j + 1 == positions.len()))
                || (j == 0 && i + 1 == positions.len())
            {
                continue;
            }
            for yy in y - 2..=y + 2 {
                for xx in x - 2..=x + 2 {
                    function[yy * n + xx] = true;
                }
            }
        }
    }
    for (x, y) in a.into_iter().chain(b) {
        function[y * n + x] = true;
    }
    function[(n - 8) * n + 8] = true;
    if version >= 7 {
        for y in 0..6 {
            for x in n - 11..n - 8 {
                function[y * n + x] = true;
                function[x * n + y] = true;
            }
        }
    }
    let mut bits = Vec::with_capacity(n * n);
    let mut right = n - 1;
    let mut up = true;
    while right > 0 {
        if right == 6 {
            right = 5;
        }
        for i in 0..n {
            let y = if up { n - 1 - i } else { i };
            for x in [right, right - 1] {
                if !function[y * n + x] {
                    let masks = (0..8).fold(0u8, |value, mask| {
                        value | (u8::from(masked(mask, x, y)) << mask)
                    });
                    bits.push((crate::numeric::usize_u16(y * n + x), masks));
                }
            }
        }
        up = !up;
        right = right.saturating_sub(2);
    }
    bits
}

pub fn decode_matrix(matrix: &[bool], n: usize) -> Option<Payload> {
    if !(21..=177).contains(&n) || !(n - 17).is_multiple_of(4) || matrix.len() != n * n {
        return None;
    }
    let version = (n - 17) / 4;
    let (a, b) = format_coordinates(n);
    let read = |p: &[(usize, usize)]| {
        p.iter().enumerate().fold(0u16, |s, (i, &(x, y))| {
            s | (u16::from(matrix[y * n + x]) << i)
        })
    };
    let fa = read(&a);
    let fb = read(&b);
    let mut best = (0, 16);
    let mut tied = false;
    for data in 0..32 {
        let code = format_code(data);
        let e = (fa ^ code).count_ones().min((fb ^ code).count_ones());
        if e < best.1 {
            best = (data, e);
            tied = false;
        } else if e == best.1 {
            tied = true;
        }
    }
    if best.1 > 3 || tied {
        return None;
    }
    let level = match best.0 >> 3 {
        1 => 0,
        0 => 1,
        3 => 2,
        _ => 3,
    };
    let mask = (best.0 & 7) as usize;
    let layout = LAYOUTS[version - 1].get_or_init(|| layout(version));
    let code: Vec<u8> = layout
        .chunks_exact(8)
        .map(|chunk| {
            chunk.iter().fold(0, |value, &(index, masks)| {
                (value << 1) | (u8::from(matrix[index as usize]) ^ ((masks >> mask) & 1))
            })
        })
        .collect();
    let ecc = ECC[level][version];
    let count = BLOCKS[level][version];
    let short = code.len() / count;
    let longs = code.len() % count;
    let short_count = count - longs;
    let lengths: Vec<usize> = (0..count)
        .map(|i| short - ecc + usize::from(i >= short_count))
        .collect();
    let mut blocks: Vec<Vec<u8>> = lengths.iter().map(|&l| vec![0; l + ecc]).collect();
    let mut at = 0;
    for column in 0..*lengths.iter().max()? {
        for (block, &data_length) in blocks.iter_mut().zip(&lengths) {
            if let Some(value) = block[..data_length].get_mut(column) {
                *value = *code.get(at)?;
                at += 1;
            }
        }
    }
    for j in 0..ecc {
        for i in 0..count {
            blocks[i][lengths[i] + j] = *code.get(at)?;
            at += 1;
        }
    }
    if at != code.len() {
        return None;
    }
    let field = Field::new(0x11d);
    let mut corrected = 0;
    let mut data = Vec::new();
    for (i, mut block) in blocks.into_iter().enumerate() {
        corrected += field.correct(&mut block, ecc, 0)?;
        data.extend_from_slice(&block[..lengths[i]]);
    }
    let (bytes, text, gs1, structured_append) = parse(&data, version)?;
    Some(Payload {
        structured_append,
        reader_initialization: false,
        bytes,
        text,
        corrected,
        gs1,
    })
}
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn take(&mut self, n: usize) -> Option<usize> {
        if self.at + n > self.data.len() * 8 {
            return None;
        }
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | ((self.data[self.at / 8] >> (7 - self.at % 8)) & 1) as usize;
            self.at += 1;
        }
        Some(v)
    }
}
fn text_segment(bytes: &[u8], eci: Option<usize>) -> Option<String> {
    crate::encoding::decode(bytes, eci)
}
#[expect(
    clippy::too_many_lines,
    reason = "The QR segment dispatcher shares version-dependent lengths, ECI, FNC1 and structured-append state."
)]
fn parse(
    data: &[u8],
    v: usize,
) -> Option<(Vec<u8>, String, bool, Option<crate::StructuredAppend>)> {
    let mut structured_append = None;
    let mut bits = Bits { data, at: 0 };
    let mut out = Vec::new();
    let mut text = String::new();
    let mut gs1 = false;
    let mut eci = None;
    while bits.at + 4 <= data.len() * 8 {
        let mode = bits.take(4)?;
        if mode == 0 {
            break;
        }
        if mode == 5 {
            gs1 = true;
            continue;
        }
        if mode == 9 {
            gs1 = true;
            bits.take(8)?;
            continue;
        }
        if mode == 3 {
            if structured_append.is_some() || !out.is_empty() {
                return None;
            }
            let index = bits.take(4)? + 1;
            let count = bits.take(4)? + 1;
            let parity = (bits.take(8)?).to_le_bytes()[0];
            if index > count {
                return None;
            }
            structured_append = Some(crate::StructuredAppend {
                index,
                count,
                parity: Some(parity),
                id: None,
            });
            continue;
        }
        if mode == 7 {
            let b = bits.take(8)?;
            eci = Some(if b & 128 == 0 {
                b
            } else if b & 192 == 128 {
                ((b & 63) << 8) | bits.take(8)?
            } else if b & 224 == 192 {
                ((b & 31) << 16) | bits.take(16)?
            } else {
                return None;
            });
            continue;
        }
        if mode == 13 && bits.take(4)? != 1 {
            return None;
        }
        let group = if v <= 9 {
            0
        } else if v <= 26 {
            1
        } else {
            2
        };
        let size = match mode {
            1 => [10, 12, 14][group],
            2 => [9, 11, 13][group],
            4 => [8, 16, 16][group],
            8 | 13 => [8, 10, 12][group],
            _ => return None,
        };
        let count = bits.take(size)?;
        let mut segment = Vec::new();
        let mut segment_eci = eci;
        match mode {
            1 => {
                let mut left = count;
                while left > 0 {
                    let n = left.min(3);
                    let x = bits.take([0, 4, 7, 10][n])?;
                    if x >= 10usize.pow(crate::numeric::usize_u32(n)) {
                        return None;
                    }
                    segment.extend(format!("{x:0n$}").as_bytes());
                    left -= n;
                }
            }
            2 => {
                const ALPH: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
                let mut left = count;
                while left >= 2 {
                    let x = bits.take(11)?;
                    if x >= 2025 {
                        return None;
                    }
                    segment.extend([ALPH[x / 45], ALPH[x % 45]]);
                    left -= 2;
                }
                if left == 1 {
                    segment.push(*ALPH.get(bits.take(6)?)?);
                }
                if gs1 {
                    let mut transformed = Vec::new();
                    let mut i = 0;
                    while i < segment.len() {
                        if segment[i] == b'%' {
                            if segment.get(i + 1) == Some(&b'%') {
                                transformed.push(b'%');
                                i += 2;
                                continue;
                            }
                            transformed.push(29);
                        } else {
                            transformed.push(segment[i]);
                        }
                        i += 1;
                    }
                    segment = transformed;
                }
            }
            4 => {
                for _ in 0..count {
                    segment.push((bits.take(8)?).to_le_bytes()[0]);
                }
            }
            8 | 13 => {
                for _ in 0..count {
                    let x = bits.take(13)?;
                    let value = if mode == 8 {
                        let a = (x / 192) * 256 + x % 192;
                        a + if a < 0x1f00 { 0x8140 } else { 0xc140 }
                    } else {
                        let a = (x / 96) * 256 + x % 96;
                        a + if a < 0x3bf { 0xa1a1 } else { 0xa6a1 }
                    };
                    segment.extend([(value >> 8).to_le_bytes()[0], (value).to_le_bytes()[0]]);
                }
                segment_eci = Some(if mode == 8 { 20 } else { 29 });
            }
            _ => return None,
        }
        text.push_str(&text_segment(
            &segment,
            if matches!(mode, 1 | 2) {
                Some(3)
            } else {
                segment_eci
            },
        )?);
        out.extend(segment);
    }
    if out.is_empty() {
        return None;
    }
    Some((out, text, gs1, structured_append))
}

/// Conservative localization evidence after payload failure: two agreeing BCH
/// copies plus both timing tracks. This never manufactures a decoded value.
#[must_use]
pub fn localization_score(matrix: &[bool], n: usize) -> Option<f32> {
    if n < 21 || matrix.len() != n * n {
        return None;
    }
    let (a, b) = format_coordinates(n);
    let read = |p: &[(usize, usize)]| {
        p.iter().enumerate().fold(0u16, |v, (i, &(x, y))| {
            v | (u16::from(matrix[y * n + x]) << i)
        })
    };
    let (a, b) = (read(&a), read(&b));
    if !(0..32).any(|d| {
        let c = format_code(d);
        (a ^ c).count_ones() <= 2 && (b ^ c).count_ones() <= 2
    }) {
        return None;
    }
    let errors = (8..n - 8)
        .map(|i| {
            usize::from(matrix[6 * n + i] != (i % 2 == 0))
                + usize::from(matrix[i * n + 6] != (i % 2 == 0))
        })
        .sum::<usize>();
    let score = 1. - crate::numeric::usize_f32(errors) / crate::numeric::usize_f32(2 * (n - 16));
    (score >= 0.8).then_some(score)
}

#[cfg(test)]
mod structured_tests {
    use super::*;
    #[test]
    fn preserves_sequence_index_count_and_parity() {
        let values = [
            (3, 4),
            (3, 4),
            (6, 4),
            (0x5a, 8),
            (4, 4),
            (4, 8),
            (b'3' as usize, 8),
            (b'4' as usize, 8),
            (b'5' as usize, 8),
            (b'6' as usize, 8),
            (0, 4),
        ];
        let bits: Vec<_> = values
            .iter()
            .flat_map(|&(v, n)| (0..n).rev().map(move |i| ((v >> i) & 1).to_le_bytes()[0]))
            .collect();
        let bytes: Vec<_> = bits
            .chunks(8)
            .map(|chunk| {
                chunk
                    .iter()
                    .enumerate()
                    .fold(0_u8, |v, (i, &b)| v | (b << (7 - i)))
            })
            .collect();
        let (_, text, _, metadata) = parse(&bytes, 1).unwrap();
        assert_eq!(text, "3456");
        assert_eq!(
            metadata,
            Some(crate::StructuredAppend {
                index: 4,
                count: 7,
                id: None,
                parity: Some(0x5a)
            })
        );
    }
}

#[cfg(test)]
mod header_proposal_tests {
    use super::*;

    type LegacyCoordinates = Vec<(usize, usize)>;
    fn old_coordinates(n: usize) -> (LegacyCoordinates, LegacyCoordinates) {
        let mut a: Vec<_> = (0..6).map(|i| (8, i)).collect();
        a.extend([(8, 7), (8, 8), (7, 8)]);
        a.extend((9..15).map(|i| (14 - i, 8)));
        let mut b: Vec<_> = (0..8).map(|i| (n - 1 - i, 8)).collect();
        b.extend((8..15).map(|i| (8, n - 15 + i)));
        (a, b)
    }

    fn old_code(data: u16) -> u16 {
        let mut r = data;
        for _ in 0..10 {
            r = (r << 1) ^ if r & 512 != 0 { 0x537 } else { 0 };
        }
        ((data << 10) | r) ^ 0x5412
    }

    #[test]
    fn fixed_header_tables_match_original_for_all_models_and_codes() {
        for n in (21..=177).step_by(4) {
            let (new_a, new_b) = format_coordinates(n);
            let (old_a, old_b) = old_coordinates(n);
            assert_eq!(new_a.as_slice(), old_a.as_slice());
            assert_eq!(new_b.as_slice(), old_b.as_slice());
        }
        for data in 0..32 {
            assert_eq!(format_code(data), old_code(data));
        }
    }
}

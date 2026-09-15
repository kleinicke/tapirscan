//! Independent ECC200 matrix decoding. Dimensions and Utah placement follow
//! ISO/IEC 16022. No external barcode decoder is linked.
use crate::{qr::Payload, reed_solomon::Field};
#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub h: usize,
    pub w: usize,
    pub rh: usize,
    pub rw: usize,
    pub data: usize,
    pub ecc: usize,
    pub blocks: usize,
}
pub const SIZES: &[Size] = &[
    Size {
        h: 8,
        w: 48,
        rh: 6,
        rw: 22,
        data: 18,
        ecc: 15,
        blocks: 1,
    },
    Size {
        h: 8,
        w: 64,
        rh: 6,
        rw: 14,
        data: 24,
        ecc: 18,
        blocks: 1,
    },
    Size {
        h: 8,
        w: 80,
        rh: 6,
        rw: 18,
        data: 32,
        ecc: 22,
        blocks: 1,
    },
    Size {
        h: 8,
        w: 96,
        rh: 6,
        rw: 22,
        data: 38,
        ecc: 28,
        blocks: 1,
    },
    Size {
        h: 8,
        w: 120,
        rh: 6,
        rw: 18,
        data: 49,
        ecc: 32,
        blocks: 1,
    },
    Size {
        h: 8,
        w: 144,
        rh: 6,
        rw: 22,
        data: 63,
        ecc: 36,
        blocks: 1,
    },
    Size {
        h: 12,
        w: 64,
        rh: 10,
        rw: 14,
        data: 43,
        ecc: 27,
        blocks: 1,
    },
    Size {
        h: 12,
        w: 88,
        rh: 10,
        rw: 20,
        data: 64,
        ecc: 36,
        blocks: 1,
    },
    Size {
        h: 16,
        w: 64,
        rh: 14,
        rw: 14,
        data: 62,
        ecc: 36,
        blocks: 1,
    },
    Size {
        h: 20,
        w: 36,
        rh: 18,
        rw: 16,
        data: 44,
        ecc: 28,
        blocks: 1,
    },
    Size {
        h: 20,
        w: 44,
        rh: 18,
        rw: 20,
        data: 56,
        ecc: 34,
        blocks: 1,
    },
    Size {
        h: 20,
        w: 64,
        rh: 18,
        rw: 14,
        data: 84,
        ecc: 42,
        blocks: 1,
    },
    Size {
        h: 22,
        w: 48,
        rh: 20,
        rw: 22,
        data: 72,
        ecc: 38,
        blocks: 1,
    },
    Size {
        h: 24,
        w: 48,
        rh: 22,
        rw: 22,
        data: 80,
        ecc: 41,
        blocks: 1,
    },
    Size {
        h: 24,
        w: 64,
        rh: 22,
        rw: 14,
        data: 108,
        ecc: 46,
        blocks: 1,
    },
    Size {
        h: 26,
        w: 40,
        rh: 24,
        rw: 18,
        data: 70,
        ecc: 38,
        blocks: 1,
    },
    Size {
        h: 26,
        w: 48,
        rh: 24,
        rw: 22,
        data: 90,
        ecc: 42,
        blocks: 1,
    },
    Size {
        h: 26,
        w: 64,
        rh: 24,
        rw: 14,
        data: 118,
        ecc: 50,
        blocks: 1,
    },
    Size {
        h: 10,
        w: 10,
        rh: 8,
        rw: 8,
        data: 3,
        ecc: 5,
        blocks: 1,
    },
    Size {
        h: 12,
        w: 12,
        rh: 10,
        rw: 10,
        data: 5,
        ecc: 7,
        blocks: 1,
    },
    Size {
        h: 14,
        w: 14,
        rh: 12,
        rw: 12,
        data: 8,
        ecc: 10,
        blocks: 1,
    },
    Size {
        h: 16,
        w: 16,
        rh: 14,
        rw: 14,
        data: 12,
        ecc: 12,
        blocks: 1,
    },
    Size {
        h: 18,
        w: 18,
        rh: 16,
        rw: 16,
        data: 18,
        ecc: 14,
        blocks: 1,
    },
    Size {
        h: 20,
        w: 20,
        rh: 18,
        rw: 18,
        data: 22,
        ecc: 18,
        blocks: 1,
    },
    Size {
        h: 22,
        w: 22,
        rh: 20,
        rw: 20,
        data: 30,
        ecc: 20,
        blocks: 1,
    },
    Size {
        h: 24,
        w: 24,
        rh: 22,
        rw: 22,
        data: 36,
        ecc: 24,
        blocks: 1,
    },
    Size {
        h: 26,
        w: 26,
        rh: 24,
        rw: 24,
        data: 44,
        ecc: 28,
        blocks: 1,
    },
    Size {
        h: 32,
        w: 32,
        rh: 14,
        rw: 14,
        data: 62,
        ecc: 36,
        blocks: 1,
    },
    Size {
        h: 36,
        w: 36,
        rh: 16,
        rw: 16,
        data: 86,
        ecc: 42,
        blocks: 1,
    },
    Size {
        h: 40,
        w: 40,
        rh: 18,
        rw: 18,
        data: 114,
        ecc: 48,
        blocks: 1,
    },
    Size {
        h: 44,
        w: 44,
        rh: 20,
        rw: 20,
        data: 144,
        ecc: 56,
        blocks: 1,
    },
    Size {
        h: 48,
        w: 48,
        rh: 22,
        rw: 22,
        data: 174,
        ecc: 68,
        blocks: 1,
    },
    Size {
        h: 52,
        w: 52,
        rh: 24,
        rw: 24,
        data: 204,
        ecc: 84,
        blocks: 2,
    },
    Size {
        h: 64,
        w: 64,
        rh: 14,
        rw: 14,
        data: 280,
        ecc: 112,
        blocks: 2,
    },
    Size {
        h: 72,
        w: 72,
        rh: 16,
        rw: 16,
        data: 368,
        ecc: 144,
        blocks: 4,
    },
    Size {
        h: 80,
        w: 80,
        rh: 18,
        rw: 18,
        data: 456,
        ecc: 192,
        blocks: 4,
    },
    Size {
        h: 88,
        w: 88,
        rh: 20,
        rw: 20,
        data: 576,
        ecc: 224,
        blocks: 4,
    },
    Size {
        h: 96,
        w: 96,
        rh: 22,
        rw: 22,
        data: 696,
        ecc: 272,
        blocks: 4,
    },
    Size {
        h: 104,
        w: 104,
        rh: 24,
        rw: 24,
        data: 816,
        ecc: 336,
        blocks: 6,
    },
    Size {
        h: 120,
        w: 120,
        rh: 18,
        rw: 18,
        data: 1050,
        ecc: 408,
        blocks: 6,
    },
    Size {
        h: 132,
        w: 132,
        rh: 20,
        rw: 20,
        data: 1304,
        ecc: 496,
        blocks: 8,
    },
    Size {
        h: 144,
        w: 144,
        rh: 22,
        rw: 22,
        data: 1558,
        ecc: 620,
        blocks: 10,
    },
    Size {
        h: 8,
        w: 18,
        rh: 6,
        rw: 16,
        data: 5,
        ecc: 7,
        blocks: 1,
    },
    Size {
        h: 8,
        w: 32,
        rh: 6,
        rw: 14,
        data: 10,
        ecc: 11,
        blocks: 1,
    },
    Size {
        h: 12,
        w: 26,
        rh: 10,
        rw: 24,
        data: 16,
        ecc: 14,
        blocks: 1,
    },
    Size {
        h: 12,
        w: 36,
        rh: 10,
        rw: 16,
        data: 22,
        ecc: 18,
        blocks: 1,
    },
    Size {
        h: 16,
        w: 36,
        rh: 14,
        rw: 16,
        data: 32,
        ecc: 24,
        blocks: 1,
    },
    Size {
        h: 16,
        w: 48,
        rh: 14,
        rw: 22,
        data: 49,
        ecc: 28,
        blocks: 1,
    },
];
struct Placement<'a> {
    bits: &'a [bool],
    seen: Vec<bool>,
    rows: isize,
    cols: isize,
}
impl Placement<'_> {
    fn bit(&mut self, mut r: isize, mut c: isize) -> Option<bool> {
        if r < 0 {
            r += self.rows;
            c += 4 - (self.rows + 4) % 8;
        }
        if c < 0 {
            c += self.cols;
            r += 4 - (self.cols + 4) % 8;
        }
        if r >= self.rows {
            r -= self.rows;
        }
        if r < 0 || c < 0 || r >= self.rows || c >= self.cols {
            return None;
        }
        let i = (r * self.cols + c) as usize;
        self.seen[i] = true;
        Some(self.bits[i])
    }
    fn byte(&mut self, coords: &[(isize, isize)]) -> Option<u8> {
        let mut value = 0;
        for &(r, c) in coords {
            value = (value << 1) | u8::from(self.bit(r, c)?);
        }
        Some(value)
    }
    fn utah(&mut self, r: isize, c: isize) -> Option<u8> {
        self.byte(&[
            (r - 2, c - 2),
            (r - 2, c - 1),
            (r - 1, c - 2),
            (r - 1, c - 1),
            (r - 1, c),
            (r, c - 2),
            (r, c - 1),
            (r, c),
        ])
    }
    fn read(&mut self) -> Option<Vec<u8>> {
        let nr = self.rows;
        let nc = self.cols;
        let mut row = 4;
        let mut col = 0;
        let mut out = Vec::new();
        loop {
            if row == nr && col == 0 {
                out.push(self.byte(&[
                    (nr - 1, 0),
                    (nr - 1, 1),
                    (nr - 1, 2),
                    (0, nc - 2),
                    (0, nc - 1),
                    (1, nc - 1),
                    (2, nc - 1),
                    (3, nc - 1),
                ])?);
            }
            if row == nr - 2 && col == 0 && nc % 4 != 0 {
                out.push(self.byte(&[
                    (nr - 3, 0),
                    (nr - 2, 0),
                    (nr - 1, 0),
                    (0, nc - 4),
                    (0, nc - 3),
                    (0, nc - 2),
                    (0, nc - 1),
                    (1, nc - 1),
                ])?);
            }
            if row == nr + 4 && col == 2 && nc % 8 == 0 {
                out.push(self.byte(&[
                    (nr - 1, 0),
                    (nr - 1, nc - 1),
                    (0, nc - 3),
                    (0, nc - 2),
                    (0, nc - 1),
                    (1, nc - 3),
                    (1, nc - 2),
                    (1, nc - 1),
                ])?);
            }
            if row == nr - 2 && col == 0 && nc % 8 == 4 {
                out.push(self.byte(&[
                    (nr - 3, 0),
                    (nr - 2, 0),
                    (nr - 1, 0),
                    (0, nc - 2),
                    (0, nc - 1),
                    (1, nc - 1),
                    (2, nc - 1),
                    (3, nc - 1),
                ])?);
            }
            loop {
                if row < nr && col >= 0 && !self.seen[(row * nc + col) as usize] {
                    out.push(self.utah(row, col)?);
                }
                row -= 2;
                col += 2;
                if row < 0 || col >= nc {
                    break;
                }
            }
            row += 1;
            col += 3;
            loop {
                if row >= 0 && col < nc && !self.seen[(row * nc + col) as usize] {
                    out.push(self.utah(row, col)?);
                }
                row += 2;
                col -= 2;
                if row >= nr || col < 0 {
                    break;
                }
            }
            row += 3;
            col += 1;
            if row >= nr && col >= nc {
                break;
            }
        }
        Some(out)
    }
}
#[must_use]
pub fn decode_matrix(matrix: &[bool], w: usize, h: usize) -> Option<Payload> {
    let size = SIZES.iter().find(|s| s.w == w && s.h == h)?;
    if matrix.len() != w * h {
        return None;
    }
    let cols = w / (size.rw + 2) * size.rw;
    let rows = h / (size.rh + 2) * size.rh;
    let mut bits = Vec::with_capacity(rows * cols);
    for y in 0..rows {
        for x in 0..cols {
            let xx = x + x / size.rw * 2 + 1;
            let yy = y + y / size.rh * 2 + 1;
            bits.push(matrix[yy * w + xx]);
        }
    }
    let code = Placement {
        bits: &bits,
        seen: vec![false; bits.len()],
        rows: rows as isize,
        cols: cols as isize,
    }
    .read()?;
    if code.len() != size.data + size.ecc {
        return None;
    }
    let lengths: Vec<usize> = (0..size.blocks)
        .map(|i| size.data / size.blocks + usize::from(i < size.data % size.blocks))
        .collect();
    let ec = size.ecc / size.blocks;
    let mut blocks: Vec<Vec<u8>> = lengths.iter().map(|&l| vec![0; l + ec]).collect();
    for i in 0..size.data {
        blocks[i % size.blocks][i / size.blocks] = code[i];
    }
    for i in 0..size.ecc {
        let block = if size.w == 144 {
            (i % size.blocks + 8) % size.blocks
        } else {
            i % size.blocks
        };
        blocks[block][lengths[block] + i / size.blocks] = code[size.data + i];
    }
    let field = Field::new(0x12d);
    let mut corrected = 0;
    for block in &mut blocks {
        corrected += field.correct(block, ec, 1)?;
    }
    let data: Vec<u8> = (0..size.data)
        .map(|i| blocks[i % size.blocks][i / size.blocks])
        .collect();
    let (bytes, text, gs1, reader_initialization, structured_append) = parse(&data)?;
    Some(Payload {
        structured_append,
        reader_initialization,
        bytes,
        text,
        corrected,
        gs1,
    })
}
type ParsedData = (Vec<u8>, String, bool, bool, Option<crate::StructuredAppend>);
fn parse(data: &[u8]) -> Option<ParsedData> {
    let mut reader_initialization = false;
    let mut structured_append = None;
    let mut text = String::new();
    let mut segment_start = 0;
    let mut encoding = 3;
    let mut out = Vec::new();
    let mut at = 0;
    let mut upper = false;
    let mut gs1 = false;
    let mut trailer = false;
    while at < data.len() {
        let c = data[at];
        at += 1;
        match c {
            1..=128 => {
                out.push(c - 1 + if upper { 128 } else { 0 });
                upper = false;
            }
            129 => break,
            130..=229 => {
                let n = c - 130;
                out.extend([b'0' + n / 10, b'0' + n % 10]);
            }
            241 => {
                let first = *data.get(at)? as usize;
                at += 1;
                let eci = if first <= 127 {
                    first.checked_sub(1)?
                } else if first <= 191 {
                    let b = *data.get(at)? as usize;
                    at += 1;
                    (first - 128) * 254 + b + 126
                } else {
                    let b = *data.get(at)? as usize;
                    let c = *data.get(at + 1)? as usize;
                    at += 2;
                    (first - 192) * 64516 + (b - 1) * 254 + c + 16382
                };
                text.push_str(&crate::encoding::decode(
                    &out[segment_start..],
                    Some(encoding),
                )?);
                segment_start = out.len();
                encoding = eci;
            }
            232 => {
                if out.is_empty() {
                    gs1 = true;
                } else {
                    out.push(29);
                }
            }
            233 if at == 1 => {
                let sequence = *data.get(at)? as usize;
                let id1 = *data.get(at + 1)?;
                let id2 = *data.get(at + 2)?;
                at += 3;
                let index = (sequence >> 4) + 1;
                let count = 17 - (sequence & 15);
                if !(2..=16).contains(&count)
                    || index > count
                    || !(1..=254).contains(&id1)
                    || !(1..=254).contains(&id2)
                {
                    return None;
                }
                structured_append = Some(crate::StructuredAppend {
                    index,
                    count,
                    id: Some(format!("{id1:03}{id2:03}")),
                    parity: None,
                });
            }
            234 if at == 1 => reader_initialization = true,
            235 => upper = true,
            236 | 237 => {
                out.extend(if c == 236 {
                    b"[)>\x1e05\x1d"
                } else {
                    b"[)>\x1e06\x1d"
                });
                trailer = true;
            }
            230 | 239 | 238 => {
                let mut shift = 0;
                let mut shifted_upper = false;
                while at < data.len() {
                    if data[at] == 254 {
                        at += 1;
                        break;
                    }
                    if at + 1 >= data.len() {
                        break;
                    }
                    let value = (data[at] as usize * 256 + data[at + 1] as usize).checked_sub(1)?;
                    at += 2;
                    let values = [value / 1600, value / 40 % 40, value % 40];
                    for v in values {
                        if c == 238 {
                            out.push(match v {
                                0 => 13,
                                1 => b'*',
                                2 => b'>',
                                3 => b' ',
                                4..=13 => b'0' + (v - 4) as u8,
                                14..=39 => b'A' + (v - 14) as u8,
                                _ => return None,
                            });
                            continue;
                        }
                        let decoded = match shift {
                            0 => match v {
                                0..=2 => {
                                    shift = v + 1;
                                    continue;
                                }
                                3 => b' ',
                                4..=13 => b'0' + (v - 4) as u8,
                                14..=39 => (if c == 239 { b'a' } else { b'A' }) + (v - 14) as u8,
                                _ => return None,
                            },
                            1 => v as u8,
                            2 => {
                                if v == 27 {
                                    out.push(29);
                                    shift = 0;
                                    continue;
                                }
                                if v == 30 {
                                    shifted_upper = true;
                                    shift = 0;
                                    continue;
                                }
                                *b"!\"#$%&'()*+,-./:;<=>?@[\\]^_".get(v)?
                            }
                            3 => {
                                if c == 239 {
                                    *b"`ABCDEFGHIJKLMNOPQRSTUVWXYZ{|}~\x7f".get(v)?
                                } else {
                                    v as u8 + 96
                                }
                            }
                            _ => return None,
                        };
                        out.push(decoded.wrapping_add(if shifted_upper { 128 } else { 0 }));
                        shifted_upper = false;
                        shift = 0;
                    }
                }
            }
            231 => {
                fn unrandom(value: u8, pos: usize) -> u8 {
                    value.wrapping_sub(((149 * pos) % 255 + 1) as u8)
                }
                let first = unrandom(*data.get(at)?, at + 1) as usize;
                at += 1;
                let length = if first == 0 {
                    data.len() - at
                } else if first <= 249 {
                    first
                } else {
                    let second = unrandom(*data.get(at)?, at + 1) as usize;
                    at += 1;
                    (first - 249) * 250 + second
                };
                for _ in 0..length {
                    out.push(unrandom(*data.get(at)?, at + 1));
                    at += 1;
                }
            }
            240 => {
                let mut bit = at * 8;
                let mut unlatch = false;
                while bit + 6 <= data.len() * 8 {
                    let mut v = 0;
                    for _ in 0..6 {
                        v = (v << 1) | ((data[bit / 8] >> (7 - bit % 8)) & 1);
                        bit += 1;
                    }
                    if v == 31 {
                        unlatch = true;
                        break;
                    }
                    out.push(if v & 32 == 0 { v | 64 } else { v });
                    if bit % 8 == 0 && data.len() - bit / 8 <= 2 {
                        break;
                    }
                }
                at = bit.div_ceil(8);
                if !unlatch && at > data.len() {
                    return None;
                }
            }
            254 if at == data.len() => break,
            _ => return None,
        }
    }
    if out.is_empty() || upper {
        return None;
    }
    if trailer {
        out.extend([30, 4]);
    }
    text.push_str(&crate::encoding::decode(
        &out[segment_start..],
        Some(encoding),
    )?);
    Some((out, text, gs1, reader_initialization, structured_append))
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    #[test]
    fn reader_initialization_is_only_a_leading_control() {
        let read = parse(&[234, 66, 129]).unwrap();
        assert_eq!(read.1, "A");
        assert!(read.3);
        assert!(parse(&[66, 234, 129]).is_none());
    }
    #[test]
    fn sequence_header_preserves_payload_and_file_identifier() {
        let read = parse(&[233, 0x3a, 1, 1, 66, 67, 129]).unwrap();
        assert_eq!(read.1, "AB");
        assert_eq!(
            read.4,
            Some(crate::StructuredAppend {
                index: 4,
                count: 7,
                id: Some("001001".into()),
                parity: None
            })
        );
        assert!(parse(&[233, 0xfa, 1, 1, 66, 129]).is_none());
    }
}

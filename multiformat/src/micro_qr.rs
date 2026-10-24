//! Independent Micro QR (M1-M4) matrix decoder, ISO/IEC 18004.
//!
//! A Micro QR symbol has one finder in its top-left corner, timing patterns along
//! row 0 and column 0, and 15 format bits beside the finder that name the symbol
//! (version and error correction level) and one of four data masks. Data and error
//! correction form a single Reed-Solomon block; M1 and M3 end their data with a
//! four-bit codeword. Symbols with little error correction must also show the
//! standard pad codewords, which keeps random grids from passing as M1/M2 reads.
use crate::qr::Payload;
use crate::reed_solomon;

/// (version, data codewords, error correction codewords, data bits) per symbol number.
const SYMBOLS: [(usize, usize, usize, usize); 8] = [
    (1, 3, 2, 20),
    (2, 5, 5, 40),
    (2, 4, 6, 32),
    (3, 11, 6, 84),
    (3, 9, 8, 68),
    (4, 16, 8, 128),
    (4, 14, 10, 112),
    (4, 10, 14, 80),
];
/// QR mask patterns of the four Micro QR masks.
const MASKS: [usize; 4] = [1, 4, 6, 7];
const FORMAT_MASK: u16 = 0x4445;

/// The 15-bit format word of `data` (symbol number << 2 | mask), masked.
#[must_use]
pub const fn format_word(data: u16) -> u16 {
    let mut remainder = data;
    let mut bit = 0;
    while bit < 10 {
        remainder = (remainder << 1) ^ if remainder & 512 != 0 { 0x537 } else { 0 };
        bit += 1;
    }
    ((data << 10) | (remainder & 0x3ff)) ^ FORMAT_MASK
}
const FORMAT_WORDS: [u16; 32] = {
    let mut words = [0; 32];
    let mut data = 0;
    while data < 32 {
        words[data as usize] = format_word(data);
        data += 1;
    }
    words
};

/// Module (column, row) of format bit `i`, most significant first.
#[must_use]
pub const fn format_position(i: usize) -> (usize, usize) {
    if i < 8 { (i + 1, 8) } else { (8, 15 - i) }
}

/// Symbol number, mask and bit distance of the closest format word; `None` when
/// two words are equally close.
#[must_use]
pub fn read_format(word: u16) -> Option<(usize, usize, u32)> {
    let mut best = (0, u32::MAX);
    let mut tied = false;
    for (data, &code) in FORMAT_WORDS.iter().enumerate() {
        let e = (word ^ code).count_ones();
        if e < best.1 {
            best = (data, e);
            tied = false;
        } else if e == best.1 {
            tied = true;
        }
    }
    (!tied).then_some((best.0 >> 2, best.0 & 3, best.1))
}

/// Side length of symbol number `symbol`.
#[must_use]
pub fn size(symbol: usize) -> usize {
    9 + 2 * SYMBOLS[symbol].0
}

fn is_function(x: usize, y: usize) -> bool {
    x == 0 || y == 0 || (x <= 8 && y <= 8)
}

/// Data module order: two-column zigzags from the bottom right, as in QR.
fn layout(n: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut upward = true;
    let mut right = n - 1;
    while right >= 2 {
        for k in 0..n {
            let y = if upward { n - 1 - k } else { k };
            for x in [right, right - 1] {
                if !is_function(x, y) {
                    out.push((x, y));
                }
            }
        }
        upward = !upward;
        right -= 2;
    }
    out
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
    end: usize,
}
impl Bits<'_> {
    fn take(&mut self, n: usize) -> Option<usize> {
        if self.at + n > self.end {
            return None;
        }
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | usize::from((self.data[self.at / 8] >> (7 - self.at % 8)) & 1);
            self.at += 1;
        }
        Some(v)
    }
}

/// Segments of the corrected data stream; returns bytes, text and the bit
/// position after the terminator.
fn parse(data: &[u8], version: usize, data_bits: usize) -> Option<(Vec<u8>, String, usize)> {
    const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
    let mut bits = Bits {
        data,
        at: 0,
        end: data_bits,
    };
    let terminator = 2 * version + 1;
    let mut bytes = Vec::new();
    let mut text = String::new();
    loop {
        let left = bits.end - bits.at;
        if left == 0 {
            break;
        }
        let mut peek = Bits { ..bits };
        if peek.take(terminator.min(left))? == 0 {
            bits.at += terminator.min(left);
            break;
        }
        let mode = bits.take(version - 1)?;
        let count_bits = match mode {
            0 => [3, 4, 5, 6][version - 1],
            1 if version >= 2 => [3, 4, 5][version - 2],
            2 if version >= 3 => [4, 5][version - 3],
            3 if version >= 3 => [3, 4][version - 3],
            _ => return None,
        };
        let count = bits.take(count_bits)?;
        let mut segment = Vec::new();
        match mode {
            0 => {
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
            1 => {
                let mut left = count;
                while left >= 2 {
                    let x = bits.take(11)?;
                    if x >= 2025 {
                        return None;
                    }
                    segment.extend([ALPHANUMERIC[x / 45], ALPHANUMERIC[x % 45]]);
                    left -= 2;
                }
                if left == 1 {
                    segment.push(*ALPHANUMERIC.get(bits.take(6)?)?);
                }
            }
            2 => {
                for _ in 0..count {
                    segment.push(bits.take(8)?.to_le_bytes()[0]);
                }
            }
            _ => {
                for _ in 0..count {
                    let x = bits.take(13)?;
                    let a = (x / 192) * 256 + x % 192;
                    let value = a + if a < 0x1f00 { 0x8140 } else { 0xc140 };
                    segment.extend([(value >> 8).to_le_bytes()[0], value.to_le_bytes()[0]]);
                }
            }
        }
        // Micro QR has no ECI. Byte data is ISO-8859-1 by default; UTF-8 text is
        // kept. Unlike QR byte data, Shift JIS is not tried: Micro QR has a kanji
        // mode for it, and the guess turns ISO-8859-1 letters into kana.
        let eci = match mode {
            2 if std::str::from_utf8(&segment).is_ok() => Some(26),
            0..=2 => Some(3),
            _ => Some(20),
        };
        text.push_str(&crate::encoding::decode(&segment, eci)?);
        bytes.extend(segment);
    }
    if bytes.is_empty() {
        return None;
    }
    Some((bytes, text, bits.at))
}

/// Whether the data after the terminator is the standard padding: zero bits to the
/// codeword boundary, then alternating 0xEC/0x11 codewords and a zero half codeword.
fn padded(data: &[u8], data_bits: usize, mut at: usize) -> bool {
    let bit = |i: usize| (data[i / 8] >> (7 - i % 8)) & 1;
    while !at.is_multiple_of(8) && at < data_bits {
        if bit(at) != 0 {
            return false;
        }
        at += 1;
    }
    let mut pad = 0xEC;
    while at + 8 <= data_bits {
        if data[at / 8] != pad {
            return false;
        }
        pad ^= 0xEC ^ 0x11;
        at += 8;
    }
    (at..data_bits).all(|i| bit(i) == 0)
}

/// Decode a sampled `n`×`n` Micro QR grid (row-major, true = dark) whose format
/// bits name `symbol` and `mask`.
#[must_use]
pub fn decode_matrix(matrix: &[bool], n: usize, symbol: usize, mask: usize) -> Option<Payload> {
    let (version, data_count, ecc, data_bits) = *SYMBOLS.get(symbol)?;
    if n != size(symbol) || matrix.len() != n * n || mask > 3 {
        return None;
    }
    let mut stream = Vec::with_capacity(data_count + ecc);
    let mut cells = layout(n)
        .into_iter()
        .map(|(x, y)| matrix[y * n + x] ^ crate::qr::masked_pattern(MASKS[mask], x, y));
    let half = data_bits % 8 != 0;
    for i in 0..data_count + ecc {
        let width = if half && i == data_count - 1 { 4 } else { 8 };
        let mut value = 0_u8;
        for _ in 0..width {
            value = (value << 1) | u8::from(cells.next()?);
        }
        stream.push(if width == 4 { value << 4 } else { value });
    }
    // Errors corrected at most. M1's two codewords only detect errors. Codes of up
    // to eight codewords (M2, M3, M4-L) keep two in reserve: there a full
    // correction often turns a random grid into a valid-looking block. M4-M and
    // M4-Q correct to capacity, like QR.
    let limit = match symbol {
        0 => 0,
        _ if ecc <= 8 => (ecc - 2) / 2,
        _ => ecc / 2,
    };
    let field = reed_solomon::gf256(0x11d);
    let corrected = field.correct(&mut stream, ecc, 0)?;
    if corrected > limit || (half && stream[data_count - 1] & 15 != 0) {
        return None;
    }
    let data = &stream[..data_count];
    let (bytes, text, end) = parse(data, version, data_bits)?;
    if version <= 2 && !padded(data, data_bits, end) {
        return None;
    }
    Some(Payload {
        structured_append: None,
        reader_initialization: false,
        bytes,
        text,
        corrected,
        gs1: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zint 2.16 `-b 97 --vers=1 -d 12345`, mask from its format bits.
    const M1_12345: [&str; 11] = [
        "#######.#.#",
        "#.....#.##.",
        "#.###.#.#..",
        "#.###.#....",
        "#.###.#.###",
        "#.....#..##",
        "#######.#..",
        ".........##",
        "##..###..##",
        ".#.#...##..",
        "####.....##",
    ];

    /// Zint 2.16 `-b 97 --vers=2 --secure=1 -d AB12`.
    const M2_L_AB12: [&str; 13] = [
        "#######.#.#.#",
        "#.....#.#...#",
        "#.###.#.#.##.",
        "#.###.#.##.#.",
        "#.###.#.....#",
        "#.....#.#.#.#",
        "#######.#....",
        "........#....",
        "##.##.#.#.##.",
        ".#.#.#.##...#",
        "#.#..#####..#",
        "..##...#.##..",
        "####..##.###.",
    ];
    /// Zint 2.16 `-b 97 --vers=3 --secure=2 -d abc`.
    const M3_M_ABC: [&str; 15] = [
        "#######.#.#.#.#",
        "#.....#.#.##.##",
        "#.###.#..#.#..#",
        "#.###.#..#.#.#.",
        "#.###.#.##..##.",
        "#.....#..#..###",
        "#######.##.#..#",
        "........#...##.",
        "#.....#####...#",
        ".#####.....#.##",
        "#..##.######..#",
        ".#.#..#.####.#.",
        "##.###.######..",
        ".#.######.#.###",
        "#####.##......#",
    ];
    /// Zint 2.16 `-b 97 --vers=3 --secure=1 -d é1`.
    const M3_L_LATIN1: [&str; 15] = [
        "#######.#.#.#.#",
        "#.....#.#...###",
        "#.###.#.####...",
        "#.###.#.#.#.#..",
        "#.###.#.#.....#",
        "#.....#..#.#.##",
        "#######..###.##",
        "........####...",
        "####..##.####.#",
        ".##.####..#.##.",
        "###.###.......#",
        ".##..##...#.###",
        "##.##.#......#.",
        "....#.####.#.##",
        "###..#..####..#",
    ];
    /// Zint 2.16 `-b 97 --vers=4 --secure=3 -d 点茗`.
    const M4_Q_KANJI: [&str; 17] = [
        "#######.#.#.#.#.#",
        "#.....#...##.###.",
        "#.###.#..##.....#",
        "#.###.#.#.###...#",
        "#.###.#...######.",
        "#.....#.##.#####.",
        "#######.....#....",
        "........#..##....",
        "#.##...##.#######",
        ".#..#..#...##..#.",
        "####..######..##.",
        ".#.#.#.####..##.#",
        "####.###.#..#####",
        "...#..##....###..",
        "#..###.#.#..##..#",
        ".#.###.#..###..#.",
        "##..##..##.#..##.",
    ];

    fn grid(rows: &[&str]) -> Vec<bool> {
        rows.iter()
            .flat_map(|r| r.bytes().map(|b| b == b'#'))
            .collect()
    }

    fn format_of(matrix: &[bool], n: usize) -> u16 {
        (0..15).fold(0, |word, i| {
            let (x, y) = format_position(i);
            (word << 1) | u16::from(matrix[y * n + x])
        })
    }

    #[test]
    fn reads_a_zint_m1_symbol() {
        let m = grid(&M1_12345);
        let (symbol, mask, errors) = read_format(format_of(&m, 11)).unwrap();
        assert_eq!((symbol, errors), (0, 0));
        let payload = decode_matrix(&m, 11, symbol, mask).unwrap();
        assert_eq!(payload.text, "12345");
    }

    #[test]
    fn reads_zint_symbols_of_every_version() {
        // M2 alphanumeric, M3 byte data ending in the four-bit codeword, ISO-8859-1
        // bytes, M4 kanji.
        for (rows, symbol, text) in [
            (&M2_L_AB12[..], 1, "AB12"),
            (&M3_M_ABC[..], 4, "abc"),
            (&M3_L_LATIN1[..], 3, "é1"),
            (&M4_Q_KANJI[..], 7, "点茗"),
        ] {
            let n = rows.len();
            let m = grid(rows);
            let (found, mask, errors) = read_format(format_of(&m, n)).unwrap();
            assert_eq!((found, errors), (symbol, 0), "{text}");
            assert_eq!(decode_matrix(&m, n, found, mask).unwrap().text, text);
        }
    }

    #[test]
    fn layouts_hold_every_codeword() {
        for (symbol, &(_, data, ecc, bits)) in SYMBOLS.iter().enumerate() {
            let n = size(symbol);
            assert_eq!(layout(n).len(), bits + 8 * ecc, "symbol {symbol}");
            assert!(bits <= 8 * data && bits > 8 * (data - 1));
        }
    }

    #[test]
    fn format_words_are_seven_bits_apart() {
        for (i, &a) in FORMAT_WORDS.iter().enumerate() {
            for &b in &FORMAT_WORDS[i + 1..] {
                assert!((a ^ b).count_ones() >= 7);
            }
        }
    }

    #[test]
    fn rejects_damaged_m1_beyond_detection() {
        let m = grid(&M1_12345);
        let (symbol, mask, _) = read_format(format_of(&m, 11)).unwrap();
        for (x, y) in layout(11) {
            let mut damaged = m.clone();
            damaged[y * 11 + x] ^= true;
            assert!(
                decode_matrix(&damaged, 11, symbol, mask).is_none(),
                "{x},{y}"
            );
        }
    }
}

//! Han Xin Code (ISO/IEC 20830) matrix decoding: function information, data
//! masks, codeword placement, Reed-Solomon blocks and the data modes.
//!
//! A symbol of version 1 to 84 has `2 * version + 21` modules per side, a
//! finder in every corner and, from version 4, a grid of alignment patterns.
//! The 34-bit function information (version, error correction level and mask
//! with Reed-Solomon parity over GF(16)) is stored twice. Data codewords are
//! split into Reed-Solomon blocks over GF(256), interleaved in groups of 13
//! and placed row by row into the remaining modules. Text without an ECI is GB
//! 18030; Zint's encoder served as cross-check for the layout and test symbols.
use crate::reed_solomon;

/// Smallest and largest version.
pub const VERSIONS: std::ops::RangeInclusive<usize> = 1..=84;

/// Modules per side of `version`.
#[must_use]
pub const fn size(version: usize) -> usize {
    2 * version + 21
}

/// Version of a symbol with `size` modules per side, or `None`.
#[must_use]
pub const fn version(size: usize) -> Option<usize> {
    if size < 23 || size > 189 || size.is_multiple_of(2) {
        None
    } else {
        Some((size - 21) / 2)
    }
}

/// A module of the function layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    /// Data and parity.
    Data,
    /// Function information.
    Info,
    /// A light function module (separator, finder or alignment).
    Light,
    /// A dark function module.
    Dark,
}

/// Rows of the top-left finder, bit 6 the leftmost module.
const TOP_LEFT: [u8; 7] = [0x7F, 0x40, 0x5F, 0x50, 0x57, 0x57, 0x57];
/// Rows of the top-right and bottom-left finders.
const SIDE: [u8; 7] = [0x7F, 0x01, 0x7D, 0x05, 0x75, 0x75, 0x75];
/// Rows of the bottom-right finder.
const BOTTOM_RIGHT: [u8; 7] = [0x75, 0x75, 0x75, 0x05, 0x7D, 0x01, 0x7F];

/// The function layout of `version`, row-major.
#[must_use]
pub fn layout(version: usize) -> Vec<Module> {
    let s = size(version);
    let mut grid = vec![Module::Data; s * s];
    for (x, y, rows) in [
        (0, 0, TOP_LEFT),
        (0, s - 7, SIDE),
        (s - 7, 0, SIDE),
        (s - 7, s - 7, BOTTOM_RIGHT),
    ] {
        for (dy, row) in rows.iter().enumerate() {
            for dx in 0..7 {
                grid[(y + dy) * s + x + dx] = if row & 0x40 >> dx == 0 {
                    Module::Light
                } else {
                    Module::Dark
                };
            }
        }
    }
    for i in 0..8 {
        for (r, c) in [
            (7, i),
            (i, 7),
            (7, s - 1 - i),
            (s - 1 - i, 7),
            (i, s - 8),
            (s - 8, i),
            (s - 8, s - 1 - i),
            (s - 1 - i, s - 8),
        ] {
            grid[r * s + c] = Module::Light;
        }
    }
    for i in 0..9 {
        for (r, c) in [
            (8, i),
            (i, 8),
            (8, s - 1 - i),
            (s - 1 - i, 8),
            (i, s - 9),
            (s - 9, i),
            (s - 9, s - 1 - i),
            (s - 1 - i, s - 9),
        ] {
            grid[r * s + c] = Module::Info;
        }
    }
    if version > 3 {
        alignment(&mut grid, version);
    }
    grid
}

/// Alignment and assistant alignment patterns (ISO/IEC 20830 annex A).
fn alignment(grid: &mut [Module], version: usize) {
    let side = i64::try_from(size(version)).unwrap_or(0);
    let spacing = i64::from(MODULE_K[version - 1]);
    let last = i64::from(MODULE_R[version - 1]);
    let blocks = i64::from(MODULE_M[version - 1]);
    let step = |n: i64| if n < blocks { spacing } else { last - 1 };
    // Function modules already placed keep their value.
    let mut plot = |x: i64, y: i64, dark: bool| {
        if (0..side).contains(&x) && (0..side).contains(&y) {
            let i = usize::try_from(y * side + x).unwrap_or(0);
            if grid[i] == Module::Data {
                grid[i] = if dark { Module::Dark } else { Module::Light };
            }
        }
    };
    let assistant = |plot: &mut dyn FnMut(i64, i64, bool), x: i64, y: i64| {
        for dy in -1..=1 {
            for dx in -1..=1 {
                plot(x + dx, y + dy, dx == 0 && dy == 0);
            }
        }
    };
    let (mut y, mut n) = (0, 0);
    while y < side {
        if n % 2 == 0 {
            if blocks % 2 == 1 {
                assistant(&mut plot, 0, y);
            }
        } else {
            if blocks % 2 == 0 {
                assistant(&mut plot, 0, y);
            }
            assistant(&mut plot, side - 1, y);
        }
        y += step(n);
        n += 1;
    }
    let (mut x, mut n) = (side - 1, 0);
    while x >= 0 {
        if n % 2 == 0 {
            if blocks % 2 == 1 {
                assistant(&mut plot, x, side - 1);
            }
        } else {
            if blocks % 2 == 0 {
                assistant(&mut plot, x, side - 1);
            }
            assistant(&mut plot, x, 0);
        }
        x -= step(n);
        n += 1;
    }
    let (mut y, mut row) = (0, 0);
    let mut first = true;
    while y < side {
        let height = step(row);
        let mut on = first;
        first = !first;
        let (mut x, mut column) = (side - 1, 0);
        while x >= 0 {
            let width = step(column);
            if on && !(y == 0 && x == side - 1) {
                plot(x, y, true);
                plot(x - 1, y + 1, false);
                for i in 1..=width {
                    plot(x - i, y, true);
                    plot(x - i - 1, y + 1, false);
                }
                for i in 1..height {
                    plot(x, y + i, true);
                    plot(x - 1, y + i + 1, false);
                }
            }
            on = !on;
            x -= width;
            column += 1;
        }
        y += height;
        row += 1;
    }
}

/// Grid indices of the two copies of each function information bit.
#[must_use]
pub fn info_positions(s: usize) -> [[usize; 2]; 34] {
    let mut positions = [[0; 2]; 34];
    for i in 0..9 {
        positions[i] = [8 * s + i, (s - 9) * s + s - 1 - i];
        positions[i + 8] = [(8 - i) * s + 8, (s - 9 + i) * s + s - 9];
        positions[i + 17] = [i * s + s - 9, (s - 1 - i) * s + 8];
        positions[i + 25] = [8 * s + s - 9 + i, (s - 9) * s + 8 - i];
    }
    positions
}

/// GF(16) with polynomial x^4 + x + 1.
fn gf16_mul(a: u8, b: u8) -> u8 {
    let mut product = 0;
    let (mut a, mut b) = (a, b);
    while b != 0 {
        if b & 1 != 0 {
            product ^= a;
        }
        a <<= 1;
        if a & 0x10 != 0 {
            a ^= 0x13;
        }
        b >>= 1;
    }
    product
}

/// The 34 function information bits, first bit in bit 33.
#[must_use]
pub fn info_bits(version: usize, level: usize, mask: usize) -> u64 {
    let data = (version + 20) << 4 | level << 2 | mask;
    // Generator with roots a^1 to a^4, highest coefficient first.
    let mut generator = vec![1u8];
    let mut root = 1u8;
    for _ in 0..4 {
        root = gf16_mul(root, 2);
        let mut next = vec![0u8; generator.len() + 1];
        for (i, &g) in generator.iter().enumerate() {
            next[i] ^= g;
            next[i + 1] ^= gf16_mul(g, root);
        }
        generator = next;
    }
    let mut remainder = [0u8; 4];
    for shift in [8, 4, 0] {
        let nibble = u8::try_from(data >> shift & 15).unwrap_or(0);
        let factor = nibble ^ remainder[0];
        remainder.rotate_left(1);
        remainder[3] = 0;
        for (r, &g) in remainder.iter_mut().zip(&generator[1..]) {
            *r ^= gf16_mul(factor, g);
        }
    }
    let parity = remainder
        .iter()
        .fold(0u64, |acc, &n| acc << 4 | u64::from(n));
    (u64::try_from(data).unwrap_or(0) << 16 | parity) << 6
}

/// The error correction level (0 to 3) and mask (0 to 3) of `version` whose
/// function information is nearest to `read` (both copies of each bit, first
/// bit in bit 33), with the number of differing bits out of 68.
#[must_use]
pub fn nearest_info(version: usize, read: [u64; 2]) -> (usize, usize, u32) {
    let mut best = (0, 0, u32::MAX);
    for level in 0..4 {
        for mask in 0..4 {
            let bits = info_bits(version, level, mask);
            let distance = (bits ^ read[0]).count_ones() + (bits ^ read[1]).count_ones();
            if distance < best.2 {
                best = (level, mask, distance);
            }
        }
    }
    best
}

/// Function information differing in more bits is not trusted.
pub const MAX_INFO_ERRORS: u32 = 10;

/// Whether data mask `mask` inverts the module at `row`, `column`.
fn masked(mask: usize, row: usize, column: usize) -> bool {
    let (i, j) = (row + 1, column + 1);
    match mask {
        1 => (i + j) % 2 == 0,
        2 => ((i + j) % 3 + j % 3) % 2 == 0,
        3 => (i % j + j % i + i % 3 + j % 3) % 2 == 0,
        _ => false,
    }
}

/// A decoded symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub text: String,
    pub bytes: Vec<u8>,
    /// Corrected codewords.
    pub corrected: usize,
}

/// Decode a square module matrix (`true` dark, row-major).
#[must_use]
pub fn decode_matrix(modules: &[bool], s: usize) -> Option<Read> {
    let version = version(s)?;
    if modules.len() != s * s {
        return None;
    }
    let positions = info_positions(s);
    let mut read = [0u64; 2];
    for pair in positions {
        for (copy, &p) in read.iter_mut().zip(&pair) {
            *copy = *copy << 1 | u64::from(modules[p]);
        }
    }
    let (level, mask, errors) = nearest_info(version, read);
    if errors > MAX_INFO_ERRORS {
        return None;
    }
    decode_data(modules, version, level, mask)
}

/// Decode the data of a matrix with known function information.
#[must_use]
pub fn decode_data(modules: &[bool], version: usize, level: usize, mask: usize) -> Option<Read> {
    let s = size(version);
    let total = usize::from(TOTAL_CODEWORDS[version - 1]);
    let layout = layout(version);
    let mut fenced = vec![0u8; total];
    let mut bit = 0;
    'place: for row in 0..s {
        for column in 0..s {
            let i = row * s + column;
            if layout[i] != Module::Data {
                continue;
            }
            if bit == total * 8 {
                break 'place;
            }
            if modules[i] ^ masked(mask, row, column) {
                fenced[bit / 8] |= 0x80 >> (bit % 8);
            }
            bit += 1;
        }
    }
    if bit < total * 8 {
        return None;
    }
    let mut stream = vec![0u8; total];
    let mut source = fenced.iter();
    for start in 0..13 {
        for i in (start..total).step_by(13) {
            stream[i] = *source.next()?;
        }
    }
    let field = reed_solomon::gf256(0x163);
    let mut data = Vec::with_capacity(total);
    let mut corrected = 0;
    let mut at = 0;
    for batch in BLOCKS[version - 1][level].as_chunks::<3>().0 {
        let [blocks, k, parity] = batch.map(usize::from);
        for _ in 0..blocks {
            let block = stream.get_mut(at..at + k + parity)?;
            corrected += field.correct(block, parity, 1)?;
            data.extend_from_slice(&block[..k]);
            at += k + parity;
        }
    }
    let (text, bytes) = parse(&data)?;
    Some(Read {
        text,
        bytes,
        corrected,
    })
}

/// Bit reader, most significant bit first.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn remaining(&self) -> usize {
        self.data.len() * 8 - self.at
    }

    fn read(&mut self, n: usize) -> Option<usize> {
        if self.remaining() < n {
            return None;
        }
        let mut value = 0;
        for _ in 0..n {
            value = value << 1 | usize::from(self.data[self.at / 8] >> (7 - self.at % 8) & 1);
            self.at += 1;
        }
        Some(value)
    }
}

/// Text of one segment: its ECI's character set, else GB 18030 (after GB
/// modes), UTF-8 or GB 18030 for byte data, or Latin-1.
fn segment_text(bytes: &[u8], eci: Option<usize>, chinese: bool) -> Option<String> {
    if eci.is_some() {
        return crate::encoding::decode(bytes, eci);
    }
    if !chinese && let Ok(text) = std::str::from_utf8(bytes) {
        return Some(text.to_owned());
    }
    crate::encoding::decode(bytes, Some(32)).or_else(|| crate::encoding::decode(bytes, Some(3)))
}

fn push_digits(bytes: &mut Vec<u8>, value: usize, digits: usize) -> Option<()> {
    if value >= 10usize.pow(u32::try_from(digits).ok()?) {
        return None;
    }
    let text = format!("{value:0digits$}");
    bytes.extend_from_slice(text.as_bytes());
    Some(())
}

/// The text and bytes of the data codewords.
fn parse(data: &[u8]) -> Option<(String, Vec<u8>)> {
    let mut bits = Bits { data, at: 0 };
    let mut text = String::new();
    let mut all = Vec::new();
    let mut segment = Vec::new();
    let mut eci = None;
    let mut chinese = false;
    while bits.remaining() >= 4 {
        let mode = bits.read(4)?;
        match mode {
            0 => break,
            1 => numeric(&mut bits, &mut segment)?,
            2 => text_mode(&mut bits, &mut segment)?,
            3 => {
                for _ in 0..bits.read(13)? {
                    segment.push(u8::try_from(bits.read(8)?).ok()?);
                }
            }
            4 | 5 => region(&mut bits, &mut segment, mode)?,
            6 => double_byte(&mut bits, &mut segment)?,
            7 => four_byte(&mut bits, &mut segment)?,
            8 => {
                let value = eci_value(&mut bits)?;
                text.push_str(&segment_text(&segment, eci, chinese)?);
                all.append(&mut segment);
                eci = Some(value);
                chinese = false;
            }
            // Unicode, GS1 and URI modes are not read.
            _ => return None,
        }
        chinese |= (4..=7).contains(&mode);
    }
    text.push_str(&segment_text(&segment, eci, chinese)?);
    all.append(&mut segment);
    if text.is_empty() {
        return None;
    }
    Some((text, all))
}

/// Numeric mode: three digits per 10 bits, ended by the count of the last
/// group.
fn numeric(bits: &mut Bits, segment: &mut Vec<u8>) -> Option<()> {
    let mut pending = None;
    loop {
        let value = bits.read(10)?;
        match value {
            0..1000 => {
                if let Some(previous) = pending.replace(value) {
                    push_digits(segment, previous, 3)?;
                }
            }
            1021..=1023 => return push_digits(segment, pending?, value - 1020),
            _ => return None,
        }
    }
}

/// Text mode: two 6-bit subsets switched by 62 and ended by 63.
fn text_mode(bits: &mut Bits, segment: &mut Vec<u8>) -> Option<()> {
    let mut first = true;
    loop {
        let value = bits.read(6)?;
        let byte = match (value, first) {
            (63, _) => return Some(()),
            (62, _) => {
                first = !first;
                continue;
            }
            (0..10, true) => b'0' + u8::try_from(value).ok()?,
            (10..36, true) => b'A' + u8::try_from(value - 10).ok()?,
            (_, true) => b'a' + u8::try_from(value - 36).ok()?,
            (0..28, false) => u8::try_from(value).ok()?,
            (28..44, false) => b' ' + u8::try_from(value - 28).ok()?,
            (44..51, false) => b':' + u8::try_from(value - 44).ok()?,
            (51..57, false) => b'[' + u8::try_from(value - 51).ok()?,
            (_, false) => b'{' + u8::try_from(value - 57).ok()?,
        };
        segment.push(byte);
    }
}

/// GB 18030 region 1 (mode 4) or region 2 (mode 5): 12 bits per character,
/// 4094 switches region and 4095 ends.
fn region(bits: &mut Bits, segment: &mut Vec<u8>, mut region: usize) -> Option<()> {
    loop {
        let glyph = bits.read(12)?;
        let pair = match (glyph, region) {
            (4095, _) => return Some(()),
            (4094, _) => {
                region = 9 - region;
                continue;
            }
            (0..0xEB0, 4) => [0xB0 + glyph / 0x5E, 0xA1 + glyph % 0x5E],
            (0xEB0..0xFCA, 4) => [0xA1 + (glyph - 0xEB0) / 0x5E, 0xA1 + (glyph - 0xEB0) % 0x5E],
            (0xFCA..0xFEA, 4) => [0xA8, 0xA1 + glyph - 0xFCA],
            (0..0xBC0, 5) => [0xD8 + glyph / 0x5E, 0xA1 + glyph % 0x5E],
            _ => return None,
        };
        for b in pair {
            segment.push(u8::try_from(b).ok()?);
        }
    }
}

/// GB 18030 two-byte mode: 15 bits per character, ended by 32767.
fn double_byte(bits: &mut Bits, segment: &mut Vec<u8>) -> Option<()> {
    loop {
        let glyph = bits.read(15)?;
        if glyph == 32767 {
            return Some(());
        }
        let (first, rest) = (0x81 + glyph / 0xBE, glyph % 0xBE);
        let second = if rest < 0x3F {
            rest + 0x40
        } else {
            rest + 0x41
        };
        if first > 0xFE {
            return None;
        }
        segment.extend([first, second].map(|b| u8::try_from(b).unwrap_or(0)));
    }
}

/// GB 18030 four-byte mode: one character in 21 bits.
fn four_byte(bits: &mut Bits, segment: &mut Vec<u8>) -> Option<()> {
    let glyph = bits.read(21)?;
    let (first, rest) = (0x81 + glyph / 0x3138, glyph % 0x3138);
    if first > 0xFE {
        return None;
    }
    let bytes = [
        first,
        0x30 + rest / 0x4EC,
        0x81 + rest % 0x4EC / 10,
        0x30 + rest % 10,
    ];
    segment.extend(bytes.map(|b| u8::try_from(b).unwrap_or(0)));
    Some(())
}

/// ECI designator of 7, 14 or 21 bits after a 1, 2 or 3-bit prefix.
fn eci_value(bits: &mut Bits) -> Option<usize> {
    for width in [7, 14, 21] {
        if bits.read(1)? == 0 {
            return bits.read(width);
        }
    }
    None
}

/// Total codewords per version (ISO/IEC 20830 table B.1).
const TOTAL_CODEWORDS: [u16; 84] = [
    25, 37, 50, 54, 69, 84, 100, 117, 136, 155, 161, 181, 203, 225, 249, 273, 299, 325, 353, 381,
    411, 422, 453, 485, 518, 552, 587, 623, 660, 698, 737, 754, 794, 836, 878, 922, 966, 1011,
    1058, 1105, 1126, 1175, 1224, 1275, 1327, 1380, 1434, 1489, 1513, 1569, 1628, 1686, 1745, 1805,
    1867, 1929, 1992, 2021, 2086, 2151, 2218, 2286, 2355, 2425, 2496, 2528, 2600, 2673, 2749, 2824,
    2900, 2977, 3056, 3135, 3171, 3252, 3334, 3416, 3500, 3585, 3671, 3758, 3798, 3886,
];

/// Alignment grid `k`, `r` and `m` per version (ISO/IEC 20830 annex A).
const MODULE_K: [u8; 84] = [
    0, 0, 0, 14, 16, 16, 17, 18, 19, 20, 14, 15, 16, 16, 17, 17, 18, 19, 20, 20, 21, 16, 17, 17,
    18, 18, 19, 19, 20, 20, 21, 17, 17, 18, 18, 19, 19, 19, 20, 20, 17, 17, 18, 18, 18, 19, 19, 19,
    17, 17, 18, 18, 18, 18, 19, 19, 19, 17, 17, 18, 18, 18, 18, 19, 19, 17, 17, 17, 18, 18, 18, 18,
    19, 19, 17, 17, 17, 18, 18, 18, 18, 18, 17, 17,
];

const MODULE_R: [u8; 84] = [
    0, 0, 0, 15, 15, 17, 18, 19, 20, 21, 15, 15, 15, 17, 17, 19, 19, 19, 19, 21, 21, 17, 16, 18,
    17, 19, 18, 20, 19, 21, 20, 17, 19, 17, 19, 17, 19, 21, 19, 21, 18, 20, 17, 19, 21, 18, 20, 22,
    17, 19, 15, 17, 19, 21, 17, 19, 21, 18, 20, 15, 17, 19, 21, 16, 18, 17, 19, 21, 15, 17, 19, 21,
    15, 17, 18, 20, 22, 15, 17, 19, 21, 23, 17, 19,
];

const MODULE_M: [u8; 84] = [
    0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4,
    4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7,
    7, 8, 8, 8, 8, 8, 8, 8, 8, 8, 9, 9, 9, 9, 9, 9, 9, 9, 10, 10,
];

/// Reed-Solomon blocks per version and error correction level as three
/// batches of (blocks, data codewords, parity codewords) (ISO/IEC 20830 table D.1).
const BLOCKS: [[[u8; 9]; 4]; 84] = [
    [
        [1, 21, 4, 0, 0, 0, 0, 0, 0],
        [1, 17, 8, 0, 0, 0, 0, 0, 0],
        [1, 13, 12, 0, 0, 0, 0, 0, 0],
        [1, 9, 16, 0, 0, 0, 0, 0, 0],
    ],
    [
        [1, 31, 6, 0, 0, 0, 0, 0, 0],
        [1, 25, 12, 0, 0, 0, 0, 0, 0],
        [1, 19, 18, 0, 0, 0, 0, 0, 0],
        [1, 15, 22, 0, 0, 0, 0, 0, 0],
    ],
    [
        [1, 42, 8, 0, 0, 0, 0, 0, 0],
        [1, 34, 16, 0, 0, 0, 0, 0, 0],
        [1, 26, 24, 0, 0, 0, 0, 0, 0],
        [1, 20, 30, 0, 0, 0, 0, 0, 0],
    ],
    [
        [1, 46, 8, 0, 0, 0, 0, 0, 0],
        [1, 38, 16, 0, 0, 0, 0, 0, 0],
        [1, 30, 24, 0, 0, 0, 0, 0, 0],
        [1, 22, 32, 0, 0, 0, 0, 0, 0],
    ],
    [
        [1, 57, 12, 0, 0, 0, 0, 0, 0],
        [1, 49, 20, 0, 0, 0, 0, 0, 0],
        [1, 37, 32, 0, 0, 0, 0, 0, 0],
        [1, 14, 20, 1, 13, 22, 0, 0, 0],
    ],
    [
        [1, 70, 14, 0, 0, 0, 0, 0, 0],
        [1, 58, 26, 0, 0, 0, 0, 0, 0],
        [1, 24, 20, 1, 22, 18, 0, 0, 0],
        [1, 16, 24, 1, 18, 26, 0, 0, 0],
    ],
    [
        [1, 84, 16, 0, 0, 0, 0, 0, 0],
        [1, 70, 30, 0, 0, 0, 0, 0, 0],
        [1, 26, 22, 1, 28, 24, 0, 0, 0],
        [2, 14, 20, 1, 12, 20, 0, 0, 0],
    ],
    [
        [1, 99, 18, 0, 0, 0, 0, 0, 0],
        [1, 40, 18, 1, 41, 18, 0, 0, 0],
        [1, 31, 26, 1, 32, 28, 0, 0, 0],
        [2, 16, 24, 1, 15, 22, 0, 0, 0],
    ],
    [
        [1, 114, 22, 0, 0, 0, 0, 0, 0],
        [2, 48, 20, 0, 0, 0, 0, 0, 0],
        [2, 24, 20, 1, 26, 22, 0, 0, 0],
        [2, 18, 28, 1, 18, 26, 0, 0, 0],
    ],
    [
        [1, 131, 24, 0, 0, 0, 0, 0, 0],
        [1, 52, 22, 1, 57, 24, 0, 0, 0],
        [2, 27, 24, 1, 29, 24, 0, 0, 0],
        [2, 21, 32, 1, 19, 30, 0, 0, 0],
    ],
    [
        [1, 135, 26, 0, 0, 0, 0, 0, 0],
        [1, 56, 24, 1, 57, 24, 0, 0, 0],
        [2, 28, 24, 1, 31, 26, 0, 0, 0],
        [2, 22, 32, 1, 21, 32, 0, 0, 0],
    ],
    [
        [1, 153, 28, 0, 0, 0, 0, 0, 0],
        [1, 62, 26, 1, 65, 28, 0, 0, 0],
        [2, 32, 28, 1, 33, 28, 0, 0, 0],
        [3, 17, 26, 1, 22, 30, 0, 0, 0],
    ],
    [
        [1, 86, 16, 1, 85, 16, 0, 0, 0],
        [1, 71, 30, 1, 72, 30, 0, 0, 0],
        [2, 37, 32, 1, 35, 30, 0, 0, 0],
        [3, 20, 30, 1, 21, 32, 0, 0, 0],
    ],
    [
        [1, 94, 18, 1, 95, 18, 0, 0, 0],
        [2, 51, 22, 1, 55, 24, 0, 0, 0],
        [3, 30, 26, 1, 31, 26, 0, 0, 0],
        [4, 18, 28, 1, 17, 24, 0, 0, 0],
    ],
    [
        [1, 104, 20, 1, 105, 20, 0, 0, 0],
        [2, 57, 24, 1, 61, 26, 0, 0, 0],
        [3, 33, 28, 1, 36, 30, 0, 0, 0],
        [4, 20, 30, 1, 19, 30, 0, 0, 0],
    ],
    [
        [1, 115, 22, 1, 114, 22, 0, 0, 0],
        [2, 65, 28, 1, 61, 26, 0, 0, 0],
        [3, 38, 32, 1, 33, 30, 0, 0, 0],
        [5, 19, 28, 1, 14, 24, 0, 0, 0],
    ],
    [
        [1, 126, 24, 1, 125, 24, 0, 0, 0],
        [2, 70, 30, 1, 69, 30, 0, 0, 0],
        [4, 33, 28, 1, 29, 26, 0, 0, 0],
        [5, 20, 30, 1, 19, 30, 0, 0, 0],
    ],
    [
        [1, 136, 26, 1, 137, 26, 0, 0, 0],
        [3, 56, 24, 1, 59, 26, 0, 0, 0],
        [5, 35, 30, 0, 0, 0, 0, 0, 0],
        [6, 18, 28, 1, 21, 28, 0, 0, 0],
    ],
    [
        [1, 148, 28, 1, 149, 28, 0, 0, 0],
        [3, 61, 26, 1, 64, 28, 0, 0, 0],
        [7, 24, 20, 1, 23, 22, 0, 0, 0],
        [6, 20, 30, 1, 21, 32, 0, 0, 0],
    ],
    [
        [3, 107, 20, 0, 0, 0, 0, 0, 0],
        [3, 65, 28, 1, 72, 30, 0, 0, 0],
        [7, 26, 22, 1, 23, 22, 0, 0, 0],
        [7, 19, 28, 1, 20, 32, 0, 0, 0],
    ],
    [
        [3, 115, 22, 0, 0, 0, 0, 0, 0],
        [4, 56, 24, 1, 63, 28, 0, 0, 0],
        [7, 28, 24, 1, 25, 22, 0, 0, 0],
        [8, 18, 28, 1, 21, 22, 0, 0, 0],
    ],
    [
        [2, 116, 22, 1, 122, 24, 0, 0, 0],
        [4, 56, 24, 1, 72, 30, 0, 0, 0],
        [7, 28, 24, 1, 32, 26, 0, 0, 0],
        [8, 18, 28, 1, 24, 30, 0, 0, 0],
    ],
    [
        [3, 127, 24, 0, 0, 0, 0, 0, 0],
        [5, 51, 22, 1, 62, 26, 0, 0, 0],
        [7, 30, 26, 1, 35, 26, 0, 0, 0],
        [8, 20, 30, 1, 21, 32, 0, 0, 0],
    ],
    [
        [2, 135, 26, 1, 137, 26, 0, 0, 0],
        [5, 56, 24, 1, 59, 26, 0, 0, 0],
        [7, 33, 28, 1, 30, 28, 0, 0, 0],
        [11, 16, 24, 1, 19, 26, 0, 0, 0],
    ],
    [
        [3, 105, 20, 1, 121, 22, 0, 0, 0],
        [5, 61, 26, 1, 57, 26, 0, 0, 0],
        [9, 28, 24, 1, 28, 22, 0, 0, 0],
        [10, 19, 28, 1, 18, 30, 0, 0, 0],
    ],
    [
        [2, 157, 30, 1, 150, 28, 0, 0, 0],
        [5, 65, 28, 1, 61, 26, 0, 0, 0],
        [8, 33, 28, 1, 34, 30, 0, 0, 0],
        [10, 19, 28, 2, 15, 26, 0, 0, 0],
    ],
    [
        [3, 126, 24, 1, 115, 22, 0, 0, 0],
        [7, 51, 22, 1, 54, 22, 0, 0, 0],
        [8, 35, 30, 1, 37, 30, 0, 0, 0],
        [15, 15, 22, 1, 10, 22, 0, 0, 0],
    ],
    [
        [4, 105, 20, 1, 103, 20, 0, 0, 0],
        [7, 56, 24, 1, 45, 18, 0, 0, 0],
        [10, 31, 26, 1, 27, 26, 0, 0, 0],
        [10, 17, 26, 3, 20, 28, 1, 21, 28],
    ],
    [
        [3, 139, 26, 1, 137, 28, 0, 0, 0],
        [6, 66, 28, 1, 66, 30, 0, 0, 0],
        [9, 36, 30, 1, 34, 32, 0, 0, 0],
        [13, 19, 28, 1, 17, 32, 0, 0, 0],
    ],
    [
        [6, 84, 16, 1, 82, 16, 0, 0, 0],
        [6, 70, 30, 1, 68, 30, 0, 0, 0],
        [7, 35, 30, 3, 33, 28, 1, 32, 28],
        [13, 20, 30, 1, 20, 28, 0, 0, 0],
    ],
    [
        [5, 105, 20, 1, 94, 18, 0, 0, 0],
        [6, 74, 32, 1, 71, 30, 0, 0, 0],
        [11, 33, 28, 1, 34, 32, 0, 0, 0],
        [13, 19, 28, 3, 16, 26, 0, 0, 0],
    ],
    [
        [4, 127, 24, 1, 126, 24, 0, 0, 0],
        [7, 66, 28, 1, 66, 30, 0, 0, 0],
        [12, 30, 24, 1, 24, 28, 1, 24, 30],
        [15, 19, 28, 1, 17, 32, 0, 0, 0],
    ],
    [
        [7, 84, 16, 1, 78, 16, 0, 0, 0],
        [7, 70, 30, 1, 66, 28, 0, 0, 0],
        [12, 33, 28, 1, 32, 30, 0, 0, 0],
        [14, 21, 32, 1, 24, 28, 0, 0, 0],
    ],
    [
        [5, 117, 22, 1, 117, 24, 0, 0, 0],
        [8, 66, 28, 1, 58, 26, 0, 0, 0],
        [11, 38, 32, 1, 34, 32, 0, 0, 0],
        [15, 20, 30, 2, 17, 26, 0, 0, 0],
    ],
    [
        [4, 148, 28, 1, 146, 28, 0, 0, 0],
        [8, 68, 30, 1, 70, 24, 0, 0, 0],
        [10, 36, 32, 3, 38, 28, 0, 0, 0],
        [16, 19, 28, 3, 16, 26, 0, 0, 0],
    ],
    [
        [4, 126, 24, 2, 135, 26, 0, 0, 0],
        [8, 70, 28, 2, 43, 26, 0, 0, 0],
        [13, 32, 28, 2, 41, 30, 0, 0, 0],
        [17, 19, 28, 3, 15, 26, 0, 0, 0],
    ],
    [
        [5, 136, 26, 1, 132, 24, 0, 0, 0],
        [5, 67, 30, 4, 68, 28, 1, 69, 28],
        [14, 35, 30, 1, 32, 24, 0, 0, 0],
        [18, 18, 26, 3, 16, 28, 1, 14, 28],
    ],
    [
        [3, 142, 26, 3, 141, 28, 0, 0, 0],
        [8, 70, 30, 1, 73, 32, 1, 74, 32],
        [12, 34, 30, 3, 34, 26, 1, 35, 28],
        [18, 21, 32, 1, 27, 30, 0, 0, 0],
    ],
    [
        [5, 116, 22, 2, 103, 20, 1, 102, 20],
        [9, 74, 32, 1, 74, 30, 0, 0, 0],
        [14, 34, 28, 2, 32, 32, 1, 32, 30],
        [19, 21, 32, 1, 25, 26, 0, 0, 0],
    ],
    [
        [7, 116, 22, 1, 117, 22, 0, 0, 0],
        [11, 65, 28, 1, 58, 24, 0, 0, 0],
        [15, 38, 32, 1, 27, 28, 0, 0, 0],
        [20, 20, 30, 1, 20, 32, 1, 21, 32],
    ],
    [
        [6, 136, 26, 1, 130, 24, 0, 0, 0],
        [11, 66, 28, 1, 62, 30, 0, 0, 0],
        [14, 34, 28, 3, 34, 32, 1, 30, 30],
        [18, 20, 30, 3, 20, 28, 2, 15, 26],
    ],
    [
        [5, 105, 20, 2, 115, 22, 2, 116, 22],
        [10, 75, 32, 1, 73, 32, 0, 0, 0],
        [16, 38, 32, 1, 27, 28, 0, 0, 0],
        [22, 19, 28, 2, 16, 30, 1, 19, 30],
    ],
    [
        [6, 147, 28, 1, 146, 28, 0, 0, 0],
        [11, 66, 28, 2, 65, 30, 0, 0, 0],
        [18, 33, 28, 2, 33, 30, 0, 0, 0],
        [22, 21, 32, 1, 28, 30, 0, 0, 0],
    ],
    [
        [6, 116, 22, 3, 125, 24, 0, 0, 0],
        [11, 75, 32, 1, 68, 30, 0, 0, 0],
        [13, 35, 28, 6, 34, 32, 1, 30, 30],
        [23, 21, 32, 1, 26, 30, 0, 0, 0],
    ],
    [
        [7, 105, 20, 4, 95, 18, 0, 0, 0],
        [12, 67, 28, 1, 63, 30, 1, 62, 32],
        [21, 31, 26, 2, 33, 32, 0, 0, 0],
        [23, 21, 32, 2, 24, 30, 0, 0, 0],
    ],
    [
        [10, 116, 22, 0, 0, 0, 0, 0, 0],
        [12, 74, 32, 1, 78, 30, 0, 0, 0],
        [18, 37, 32, 1, 39, 30, 1, 41, 28],
        [25, 21, 32, 1, 27, 28, 0, 0, 0],
    ],
    [
        [5, 126, 24, 4, 115, 22, 1, 114, 22],
        [12, 67, 28, 2, 66, 32, 1, 68, 30],
        [21, 35, 30, 1, 39, 30, 0, 0, 0],
        [26, 21, 32, 1, 28, 28, 0, 0, 0],
    ],
    [
        [9, 126, 24, 1, 117, 22, 0, 0, 0],
        [13, 75, 32, 1, 68, 30, 0, 0, 0],
        [20, 35, 30, 3, 35, 28, 0, 0, 0],
        [27, 21, 32, 1, 28, 30, 0, 0, 0],
    ],
    [
        [9, 126, 24, 1, 137, 26, 0, 0, 0],
        [13, 71, 30, 2, 68, 32, 0, 0, 0],
        [20, 37, 32, 1, 39, 28, 1, 38, 28],
        [24, 20, 32, 5, 25, 28, 0, 0, 0],
    ],
    [
        [8, 147, 28, 1, 141, 28, 0, 0, 0],
        [10, 73, 32, 4, 74, 30, 1, 73, 30],
        [16, 36, 32, 6, 39, 30, 1, 37, 30],
        [27, 21, 32, 3, 20, 26, 0, 0, 0],
    ],
    [
        [9, 137, 26, 1, 135, 26, 0, 0, 0],
        [12, 70, 30, 4, 75, 32, 0, 0, 0],
        [24, 35, 30, 1, 40, 28, 0, 0, 0],
        [23, 20, 32, 8, 24, 30, 0, 0, 0],
    ],
    [
        [14, 95, 18, 1, 86, 18, 0, 0, 0],
        [13, 73, 32, 3, 77, 30, 0, 0, 0],
        [24, 35, 30, 2, 35, 28, 0, 0, 0],
        [26, 21, 32, 5, 21, 30, 1, 23, 30],
    ],
    [
        [9, 147, 28, 1, 142, 28, 0, 0, 0],
        [10, 73, 30, 6, 70, 32, 1, 71, 32],
        [25, 35, 30, 2, 34, 26, 0, 0, 0],
        [29, 21, 32, 4, 22, 30, 0, 0, 0],
    ],
    [
        [11, 126, 24, 1, 131, 24, 0, 0, 0],
        [16, 74, 32, 1, 79, 30, 0, 0, 0],
        [25, 38, 32, 1, 25, 30, 0, 0, 0],
        [33, 21, 32, 1, 28, 28, 0, 0, 0],
    ],
    [
        [14, 105, 20, 1, 99, 18, 0, 0, 0],
        [19, 65, 28, 1, 72, 28, 0, 0, 0],
        [24, 37, 32, 2, 40, 30, 1, 41, 30],
        [31, 21, 32, 4, 24, 32, 0, 0, 0],
    ],
    [
        [10, 147, 28, 1, 151, 28, 0, 0, 0],
        [15, 71, 30, 3, 71, 32, 1, 73, 32],
        [24, 37, 32, 3, 38, 30, 1, 39, 30],
        [36, 19, 30, 3, 29, 26, 0, 0, 0],
    ],
    [
        [15, 105, 20, 1, 99, 18, 0, 0, 0],
        [19, 70, 30, 1, 64, 28, 0, 0, 0],
        [27, 38, 32, 2, 25, 26, 0, 0, 0],
        [38, 20, 30, 2, 18, 28, 0, 0, 0],
    ],
    [
        [14, 105, 20, 1, 113, 22, 1, 114, 22],
        [17, 67, 30, 3, 92, 32, 0, 0, 0],
        [30, 35, 30, 1, 41, 30, 0, 0, 0],
        [36, 21, 32, 1, 26, 30, 1, 27, 30],
    ],
    [
        [11, 146, 28, 1, 146, 26, 0, 0, 0],
        [20, 70, 30, 1, 60, 26, 0, 0, 0],
        [29, 38, 32, 1, 24, 32, 0, 0, 0],
        [40, 20, 30, 2, 17, 26, 0, 0, 0],
    ],
    [
        [3, 137, 26, 1, 136, 26, 10, 126, 24],
        [22, 65, 28, 1, 75, 30, 0, 0, 0],
        [30, 37, 32, 1, 51, 30, 0, 0, 0],
        [42, 20, 30, 1, 21, 30, 0, 0, 0],
    ],
    [
        [12, 126, 24, 2, 118, 22, 1, 116, 22],
        [19, 74, 32, 1, 74, 30, 1, 72, 28],
        [30, 38, 32, 2, 29, 30, 0, 0, 0],
        [39, 20, 32, 2, 37, 26, 1, 38, 26],
    ],
    [
        [12, 126, 24, 3, 136, 26, 0, 0, 0],
        [21, 70, 30, 2, 65, 28, 0, 0, 0],
        [34, 35, 30, 1, 44, 32, 0, 0, 0],
        [42, 20, 30, 2, 19, 28, 2, 18, 28],
    ],
    [
        [12, 126, 24, 3, 117, 22, 1, 116, 22],
        [25, 61, 26, 2, 62, 28, 0, 0, 0],
        [34, 35, 30, 1, 40, 32, 1, 41, 32],
        [45, 20, 30, 1, 20, 32, 1, 21, 32],
    ],
    [
        [15, 105, 20, 2, 115, 22, 2, 116, 22],
        [25, 65, 28, 1, 72, 28, 0, 0, 0],
        [18, 35, 30, 17, 37, 32, 1, 50, 32],
        [42, 20, 30, 6, 19, 28, 1, 15, 28],
    ],
    [
        [19, 105, 20, 1, 101, 20, 0, 0, 0],
        [33, 51, 22, 1, 65, 22, 0, 0, 0],
        [40, 33, 28, 1, 28, 28, 0, 0, 0],
        [49, 20, 30, 1, 18, 28, 0, 0, 0],
    ],
    [
        [18, 105, 20, 2, 117, 22, 0, 0, 0],
        [26, 65, 28, 1, 80, 30, 0, 0, 0],
        [35, 35, 30, 3, 35, 28, 1, 36, 28],
        [52, 18, 28, 2, 38, 30, 0, 0, 0],
    ],
    [
        [26, 84, 16, 0, 0, 0, 0, 0, 0],
        [26, 70, 30, 0, 0, 0, 0, 0, 0],
        [45, 31, 26, 1, 9, 26, 0, 0, 0],
        [52, 20, 30, 0, 0, 0, 0, 0, 0],
    ],
    [
        [16, 126, 24, 1, 114, 22, 1, 115, 22],
        [23, 70, 30, 3, 65, 28, 1, 66, 28],
        [40, 35, 30, 1, 43, 30, 0, 0, 0],
        [46, 20, 30, 7, 19, 28, 1, 16, 28],
    ],
    [
        [19, 116, 22, 1, 105, 22, 0, 0, 0],
        [20, 70, 30, 7, 66, 28, 1, 63, 28],
        [40, 35, 30, 1, 42, 32, 1, 43, 32],
        [54, 20, 30, 1, 19, 30, 0, 0, 0],
    ],
    [
        [17, 126, 24, 2, 115, 22, 0, 0, 0],
        [24, 70, 30, 4, 74, 32, 0, 0, 0],
        [48, 31, 26, 2, 18, 26, 0, 0, 0],
        [54, 19, 28, 6, 15, 26, 1, 14, 26],
    ],
    [
        [29, 84, 16, 0, 0, 0, 0, 0, 0],
        [29, 70, 30, 0, 0, 0, 0, 0, 0],
        [6, 34, 30, 3, 36, 30, 38, 33, 28],
        [58, 20, 30, 0, 0, 0, 0, 0, 0],
    ],
    [
        [16, 147, 28, 1, 149, 28, 0, 0, 0],
        [31, 66, 28, 1, 37, 26, 0, 0, 0],
        [48, 33, 28, 1, 23, 26, 0, 0, 0],
        [53, 20, 30, 6, 19, 28, 1, 17, 28],
    ],
    [
        [20, 115, 22, 2, 134, 24, 0, 0, 0],
        [29, 66, 28, 2, 56, 26, 2, 57, 26],
        [45, 36, 30, 2, 15, 28, 0, 0, 0],
        [59, 20, 30, 2, 21, 32, 0, 0, 0],
    ],
    [
        [17, 147, 28, 1, 134, 26, 0, 0, 0],
        [26, 70, 30, 5, 75, 32, 0, 0, 0],
        [47, 35, 30, 1, 48, 32, 0, 0, 0],
        [64, 18, 28, 2, 33, 30, 1, 35, 30],
    ],
    [
        [22, 115, 22, 1, 133, 24, 0, 0, 0],
        [33, 65, 28, 1, 74, 28, 0, 0, 0],
        [43, 36, 30, 5, 27, 28, 1, 30, 28],
        [57, 20, 30, 5, 21, 32, 1, 24, 32],
    ],
    [
        [18, 136, 26, 2, 142, 26, 0, 0, 0],
        [33, 66, 28, 2, 49, 26, 0, 0, 0],
        [48, 35, 30, 2, 38, 28, 0, 0, 0],
        [64, 20, 30, 1, 20, 32, 0, 0, 0],
    ],
    [
        [19, 126, 24, 2, 135, 26, 1, 136, 26],
        [32, 66, 28, 2, 55, 26, 2, 56, 26],
        [49, 36, 30, 2, 18, 32, 0, 0, 0],
        [65, 18, 28, 5, 27, 30, 1, 29, 30],
    ],
    [
        [20, 137, 26, 1, 130, 26, 0, 0, 0],
        [30, 75, 32, 2, 71, 32, 0, 0, 0],
        [46, 35, 30, 6, 39, 32, 0, 0, 0],
        [3, 12, 30, 70, 19, 28, 0, 0, 0],
    ],
    [
        [20, 147, 28, 0, 0, 0, 0, 0, 0],
        [35, 70, 30, 0, 0, 0, 0, 0, 0],
        [49, 35, 30, 5, 35, 28, 0, 0, 0],
        [70, 20, 30, 0, 0, 0, 0, 0, 0],
    ],
    [
        [21, 136, 26, 1, 155, 28, 0, 0, 0],
        [34, 70, 30, 1, 64, 28, 1, 65, 28],
        [54, 35, 30, 1, 45, 30, 0, 0, 0],
        [68, 20, 30, 3, 18, 28, 1, 19, 28],
    ],
    [
        [19, 126, 24, 5, 115, 22, 1, 114, 22],
        [33, 70, 30, 3, 65, 28, 1, 64, 28],
        [52, 35, 30, 3, 41, 32, 1, 40, 32],
        [67, 20, 30, 5, 21, 32, 1, 24, 32],
    ],
    [
        [2, 150, 28, 21, 136, 26, 0, 0, 0],
        [32, 70, 30, 6, 65, 28, 0, 0, 0],
        [52, 38, 32, 2, 27, 32, 0, 0, 0],
        [73, 20, 30, 2, 22, 32, 0, 0, 0],
    ],
    [
        [21, 126, 24, 4, 136, 26, 0, 0, 0],
        [30, 74, 32, 6, 73, 30, 0, 0, 0],
        [54, 35, 30, 4, 40, 32, 0, 0, 0],
        [75, 20, 30, 1, 20, 28, 0, 0, 0],
    ],
    [
        [30, 105, 20, 1, 114, 22, 0, 0, 0],
        [3, 45, 22, 55, 47, 20, 0, 0, 0],
        [2, 26, 26, 62, 33, 28, 0, 0, 0],
        [79, 18, 28, 4, 33, 30, 0, 0, 0],
    ],
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::han_xin_detect::testing::zint_modules;

    /// `zint --dump --barcode=HANXIN` matrices with their text.
    const SYMBOLS: [(&str, &[&str]); 4] = [
        // --vers=1 --secure=3 (GB 18030 region 1 and 2)
        (
            "汉信码标准",
            &[
                "FE 88 FE", "80 E8 02", "BE EE FA", "A0 A4 0A", "AE 04 EA", "AE BC EA", "AE 7C EA",
                "00 2C 00", "15 EF 00", "1D 88 28", "C2 FA 00", "C6 A0 16", "1C 91 54", "74 B5 0E",
                "01 AB 50", "00 50 00", "FE 0C EA", "02 56 EA", "FA 2C EA", "0A 1E 0A", "EA 8A FA",
                "EA 56 02", "EA 2A FE",
            ],
        ),
        // --vers=2 --secure=1 (binary bytes)
        (
            "Grüße aus Köln",
            &[
                "FE EA BF 8",
                "80 D5 00 8",
                "BE EA BE 8",
                "A0 56 02 8",
                "AE 30 3A 8",
                "AE A9 BA 8",
                "AE 2A BA 8",
                "00 55 00 0",
                "16 73 80 0",
                "D5 6C 64 8",
                "AA 91 DA 8",
                "54 7B 55 8",
                "A5 32 AA 0",
                "5A 8D 53 8",
                "18 3A A3 8",
                "F5 35 5D 0",
                "00 8B 34 0",
                "00 65 00 0",
                "FE AA 3A 8",
                "02 E7 BA 8",
                "FA 7C 3A 8",
                "0A 55 02 8",
                "EA E6 BE 8",
                "EA 15 80 8",
                "EA AA BF 8",
            ],
        ),
        // --vers=3 --secure=3 --mask=3 (text and numeric)
        (
            "Hello 123",
            &[
                "FE B6 8F E",
                "80 92 A0 2",
                "BE 98 AF A",
                "A0 99 00 A",
                "AE 52 4E A",
                "AE 7A 4E A",
                "AE C4 AE A",
                "00 7D C0 0",
                "17 FF A8 0",
                "01 1B 6A 4",
                "2F C2 A2 C",
                "FF B5 49 4",
                "85 27 19 8",
                "1A B2 41 2",
                "42 E1 FF 2",
                "F9 33 6A 4",
                "3A C8 35 2",
                "FF 69 AB E",
                "02 A4 BD 0",
                "00 1B 40 0",
                "FE 94 2E A",
                "02 24 8E A",
                "FA 12 0E A",
                "0A 0D E0 A",
                "EA A4 AF A",
                "EA 90 E0 2",
                "EA 67 EF E",
            ],
        ),
        // --vers=4 --secure=3 (numeric, first version with alignment patterns)
        (
            "12345678901234567890",
            &[
                "FE A2 BB F8",
                "80 50 58 08",
                "BE AA DB E8",
                "A0 BB D8 28",
                "AE F7 0B A8",
                "AE D5 53 A8",
                "AE 28 C3 A8",
                "00 5F 68 00",
                "18 BA A4 00",
                "D5 58 F9 00",
                "AA AA A3 E8",
                "D5 5F 2D 58",
                "EF AA 97 70",
                "C3 35 54 20",
                "FF FE D4 08",
                "00 03 35 40",
                "F3 BA A8 68",
                "56 23 13 E8",
                "4A AB 32 48",
                "B5 53 47 A8",
                "01 53 A8 C0",
                "00 D3 50 00",
                "FE 2B D3 A8",
                "02 3A BB A8",
                "FA EA AB A8",
                "0A D3 48 28",
                "EA 92 BB E8",
                "EA F3 50 08",
                "EA AB FB F8",
            ],
        ),
    ];

    #[test]
    fn sizes() {
        assert_eq!(size(1), 23);
        assert_eq!(size(84), 189);
        assert_eq!(version(23), Some(1));
        assert_eq!(version(24), None);
        assert_eq!(version(191), None);
    }

    #[test]
    fn info_is_recovered() {
        for version in [1, 40, 84] {
            for level in 0..4 {
                for mask in 0..4 {
                    let bits = info_bits(version, level, mask);
                    let damaged = bits ^ 0b1000_0000_0100_0001;
                    let (l, m, d) = nearest_info(version, [damaged, bits]);
                    assert_eq!((l, m, d), (level, mask, 3));
                }
            }
        }
    }

    #[test]
    fn decodes_zint_symbols() {
        for (text, rows) in SYMBOLS {
            let (modules, s) = zint_modules(rows);
            let read = decode_matrix(&modules, s).expect(text);
            assert_eq!((read.text.as_str(), read.corrected), (text, 0));
        }
    }

    #[test]
    fn corrects_damage_and_rejects_mirrors() {
        let (text, rows) = SYMBOLS[3];
        let (modules, s) = zint_modules(rows);
        let mut damaged = modules.clone();
        for i in [10 * s + 12, 14 * s + 14, 20 * s + 9] {
            damaged[i] = !damaged[i];
        }
        let read = decode_matrix(&damaged, s).expect("damaged");
        assert_eq!(read.text, text);
        assert!(read.corrected > 0);
        let mirrored: Vec<bool> = (0..s * s).map(|i| modules[(i % s) * s + i / s]).collect();
        assert!(decode_matrix(&mirrored, s).is_none());
    }
}

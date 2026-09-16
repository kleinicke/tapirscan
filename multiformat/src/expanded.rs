//! Independent GS1 `DataBar Expanded` (single-row) decoder and GS1 bit parser.
use crate::{
    databar::{compositions, key, lookup_character, match_finder, Pattern},
    linear::{pattern_error, Read},
};
use std::fmt::Write as _;
use std::sync::OnceLock;
const FINDERS: [[u8; 5]; 12] = [
    [1, 8, 4, 1, 1],
    [1, 1, 4, 8, 1],
    [3, 6, 4, 1, 1],
    [1, 1, 4, 6, 3],
    [3, 4, 6, 1, 1],
    [1, 1, 6, 4, 3],
    [3, 2, 8, 1, 1],
    [1, 1, 8, 2, 3],
    [2, 6, 5, 1, 1],
    [1, 1, 5, 6, 2],
    [2, 2, 9, 1, 1],
    [1, 1, 9, 2, 2],
];
const SEQUENCES: [&[usize]; 10] = [
    &[1, 2],
    &[1, 4, 3],
    &[1, 6, 3, 8],
    &[1, 10, 3, 8, 5],
    &[1, 10, 3, 8, 7, 12],
    &[1, 10, 3, 8, 9, 12, 11],
    &[1, 2, 3, 4, 5, 6, 7, 8],
    &[1, 2, 3, 4, 5, 6, 7, 10, 9],
    &[1, 2, 3, 4, 5, 6, 7, 10, 11, 12],
    &[1, 2, 3, 4, 5, 8, 7, 10, 9, 12, 11],
];
fn table() -> &'static Vec<Pattern> {
    static TABLE: OnceLock<Vec<Pattern>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let groups = [
            (0, 348, 4, 12, 7),
            (348, 1388, 20, 10, 5),
            (1388, 2948, 52, 8, 4),
            (2948, 3988, 104, 6, 3),
            (3988, 4096, 204, 4, 1),
        ];
        let mut out = Vec::new();
        for (base, end, multiplier, sum, max) in groups {
            let odd = compositions(sum, max, true);
            let even = compositions(17 - sum, 9 - max, false);
            for (i, o) in odd.iter().enumerate() {
                for (j, e) in even.iter().enumerate() {
                    let value = base + i * multiplier + j;
                    if j >= multiplier || value >= end {
                        continue;
                    }
                    let widths =
                        std::array::from_fn(|k| if k % 2 == 0 { o[k / 2] } else { e[k / 2] });
                    out.push(Pattern {
                        key: key(widths),
                        value,
                        widths,
                    });
                }
            }
        }
        out.sort_by_key(|p| p.key);
        out
    })
}
struct Bits {
    data: Vec<bool>,
    at: usize,
}
impl Bits {
    fn peek(&self, n: usize) -> Option<usize> {
        if self.at + n > self.data.len() {
            return None;
        }
        Some(
            self.data[self.at..self.at + n]
                .iter()
                .fold(0, |v, &b| (v << 1) | usize::from(b)),
        )
    }
    fn take(&mut self, n: usize) -> Option<usize> {
        let v = self.peek(n)?;
        self.at += n;
        Some(v)
    }
}
fn gtin(bits: &mut Bits, leading: usize) -> Option<String> {
    if leading > 9 {
        return None;
    }
    let mut digits = leading.to_string();
    for _ in 0..4 {
        let value = bits.take(10)?;
        if value >= 1000 {
            return None;
        }
        // Writing to a String is infallible; ignoring the formatting result avoids a temporary allocation.
        let _ = write!(digits, "{value:03}");
    }
    let check = (10
        - digits
            .bytes()
            .rev()
            .enumerate()
            .map(|(i, d)| (d - b'0') as usize * if i % 2 == 0 { 3 } else { 1 })
            .sum::<usize>()
            % 10)
        % 10;
    Some(format!("01{digits}{check}"))
}
fn general(bits: &mut Bits, reset_separator: bool) -> Option<String> {
    let mut mode = 0;
    let mut out = Vec::new();
    while bits.at < bits.data.len() {
        if mode == 0 {
            if bits.data.len() - bits.at < 7 {
                if let Some(v) = bits.take(4) {
                    if (1..=10).contains(&v) {
                        out.push(b'0' + (v - 1).to_le_bytes()[0]);
                    } else if v != 0 && v != 11 {
                        return None;
                    }
                }
                break;
            }
            if bits.peek(4) == Some(0) {
                bits.take(4)?;
                mode = 1;
                continue;
            }
            let value = bits.take(7)?.checked_sub(8)?;
            let a = value / 11;
            let b = value % 11;
            if a > 10 {
                return None;
            }
            for v in [a, b] {
                out.push(if v == 10 {
                    29
                } else {
                    b'0' + (v).to_le_bytes()[0]
                });
            }
        } else {
            if bits.peek(3) == Some(0) {
                bits.take(3)?;
                mode = 0;
                continue;
            }
            if bits.peek(5) == Some(4) {
                bits.take(5)?;
                mode = if mode == 1 { 2 } else { 1 };
                continue;
            }
            let Some(v) = bits.peek(5) else {
                break;
            };
            if v == 15 {
                bits.take(5)?;
                out.push(29);
                if reset_separator {
                    mode = 0;
                }
                continue;
            }
            if (5..=14).contains(&v) {
                bits.take(5)?;
                out.push(b'0' + (v - 5).to_le_bytes()[0]);
                continue;
            }
            if mode == 1 {
                let v = bits.take(6)?;
                out.push(match v {
                    32..=57 => (v + 33).to_le_bytes()[0],
                    58..=62 => b"*,-./"[v - 58],
                    _ => return None,
                });
            } else {
                let v = bits.peek(7)?;
                if (64..=115).contains(&v) {
                    bits.take(7)?;
                    out.push((v + if v <= 89 { 1 } else { 7 }).to_le_bytes()[0]);
                } else {
                    let v = bits.take(8)?;
                    out.push(*b"!\"%&'()*+,-./:;<=>?_ ".get(v.checked_sub(232)?)?);
                }
            }
        }
    }
    while out.last() == Some(&29) {
        out.pop();
    }
    String::from_utf8(out).ok()
}
fn payload_mode(words: &[usize], reset_separator: bool) -> Option<String> {
    let mut bits = Bits {
        data: words
            .iter()
            .flat_map(|&v| (0..12).rev().map(move |i| (v >> i) & 1 != 0))
            .collect(),
        at: 1,
    };
    let symbols = words.len() + 1;
    let variable = (symbols % 2) * 2 + usize::from(symbols > 14);
    let mut out;
    if bits.peek(1) == Some(1) {
        if bits.take(3)? & 3 != variable {
            return None;
        }
        let leading = bits.take(4)?;
        out = gtin(&mut bits, leading)?;
        out.push_str(&general(&mut bits, reset_separator)?);
    } else if bits.peek(2) == Some(0) {
        if bits.take(4)? & 3 != variable {
            return None;
        }
        out = general(&mut bits, reset_separator)?;
    } else if matches!(bits.peek(4), Some(4 | 5)) {
        let method = bits.take(4)?;
        out = gtin(&mut bits, 9)?;
        let value = bits.take(15)?;
        if method == 4 {
            let _ = write!(out, "3103{value:06}");
        } else if value >= 10000 {
            let _ = write!(out, "3203{:06}", value - 10000);
        } else {
            let _ = write!(out, "3202{value:06}");
        }
    } else if matches!(bits.peek(5), Some(12 | 13)) {
        let header = bits.take(7)?;
        if header & 3 != variable {
            return None;
        }
        out = gtin(&mut bits, 9)?;
        let precision = bits.take(2)?;
        let _ = write!(
            out,
            "39{}{precision}",
            if header >> 2 == 12 { 2 } else { 3 }
        );
        if header >> 2 == 13 {
            let currency = bits.take(10)?;
            if currency >= 1000 {
                return None;
            }
            let _ = write!(out, "{currency:03}");
        }
        out.push_str(&general(&mut bits, reset_separator)?);
    } else {
        let method = bits.take(7)?;
        if !(56..=63).contains(&method) {
            return None;
        }
        out = gtin(&mut bits, 9)?;
        let weight = bits.take(20)?;
        if weight >= 1_000_000 {
            return None;
        }
        let unit = if method % 2 == 0 { 310 } else { 320 };
        let _ = write!(out, "{unit}{}{:06}", weight / 100_000, weight % 100_000);
        let date = bits.take(16)?;
        if date > 38400 {
            return None;
        }
        if date != 38400 {
            let ai = 11 + 2 * ((method - 56) / 2);
            out.push_str(
                &format!(
                    "{ai} {:02}{:02}{:02}",
                    date / 384,
                    (date / 32) % 12 + 1,
                    date % 32
                )
                .replace(' ', ""),
            );
        }
    }
    (!out.is_empty()).then_some(out)
}

// Older Expanded fixtures retain the alphanumeric state across FNC1;
// current GS1 encoding switches to numeric. Resolve differing parses only
// when exactly one has a valid known GS1 element-string structure. Never use
// a reference decoder or the fixture's expected value to choose the parse.
fn known_gs1(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == 29 {
            at += 1;
            continue;
        }
        let rest = &bytes[at..];
        let (ai, fixed, maximum, numeric) = if rest.len() >= 4
            && rest[..4].iter().all(u8::is_ascii_digit)
            && (rest.starts_with(b"310") || rest.starts_with(b"320"))
        {
            (4, 6, 6, true)
        } else if rest.starts_with(b"392") && rest.get(3).is_some_and(u8::is_ascii_digit) {
            (4, 0, 15, true)
        } else if rest.starts_with(b"393") && rest.get(3).is_some_and(u8::is_ascii_digit) {
            (4, 0, 18, true)
        } else if rest.starts_with(b"422") {
            (3, 3, 3, true)
        } else if rest.starts_with(b"423") {
            (3, 0, 15, true)
        } else if rest.starts_with(b"414") {
            (3, 13, 13, true)
        } else if rest.starts_with(b"420") {
            (3, 0, 20, false)
        } else if rest.len() >= 2 {
            match &rest[..2] {
                b"00" => (2, 18, 18, true),
                b"01" | b"02" => (2, 14, 14, true),
                b"11" | b"12" | b"13" | b"15" | b"16" | b"17" => (2, 6, 6, true),
                b"20" => (2, 2, 2, true),
                b"10" | b"21" => (2, 0, 20, false),
                b"22" => (2, 0, 29, false),
                b"30" | b"37" => (2, 0, 8, true),
                _ => return false,
            }
        } else {
            return false;
        };
        at += ai;
        let length = if fixed > 0 {
            fixed
        } else {
            bytes[at..]
                .iter()
                .position(|&b| b == 29)
                .unwrap_or(bytes.len() - at)
        };
        if length == 0 || length > maximum || at + length > bytes.len() {
            return false;
        }
        if numeric && !bytes[at..at + length].iter().all(u8::is_ascii_digit) {
            return false;
        }
        at += length;
    }
    true
}
#[must_use]
pub fn payload(words: &[usize]) -> Option<String> {
    let current = payload_mode(words, true);
    let legacy = payload_mode(words, false);
    match (current, legacy) {
        (Some(a), Some(b)) if a == b => Some(a),
        (Some(a), Some(b)) => match (known_gs1(&a), known_gs1(&b)) {
            (true, false) => Some(a),
            (false, true) => Some(b),
            _ => None,
        },
        (Some(a), None) => Some(a),
        (None, Some(b)) if known_gs1(&b) => Some(b),
        _ => None,
    }
}
#[must_use]
pub fn decode(r: &[f32], s: usize) -> Option<Read> {
    if s + 14 >= r.len() || pattern_error(&r[s + 9..], &FINDERS[0]) > 0.15 {
        return None;
    }
    let check = lookup_character(&r[s + 1..], 17, table())?;
    let symbols = check.0 / 211 + 4;
    if !(4..=22).contains(&symbols) {
        return None;
    }
    let sequence = SEQUENCES[(symbols - 3) / 2];
    let blocks = symbols.div_ceil(2);
    let pattern_width = blocks * 5 + symbols * 8 + 4;
    let end = s + pattern_width - if pattern_width % 2 == 0 { 1 } else { 2 };
    if end > r.len() {
        return None;
    }
    let mut err = check.2;
    for (i, &finder) in sequence.iter().enumerate().take(blocks) {
        let at = s + i * 21 + 9;
        if at + 5 > r.len() {
            return None;
        }
        let e = pattern_error(&r[at..], &FINDERS[finder - 1]);
        if e > 0.18 {
            return None;
        }
        err += e;
    }
    let mut checksum = 0;
    let mut words = Vec::new();
    for i in 0..symbols - 1 {
        let (at, reverse) = if i % 2 == 0 {
            (s + i / 2 * 21 + 14, true)
        } else {
            (s + (i - 1) / 2 * 21 + 22, false)
        };
        if at + 8 > r.len() {
            return None;
        }
        let mut widths = r[at..at + 8].to_vec();
        if reverse {
            widths.reverse();
        }
        let (value, pattern, e) = lookup_character(&widths, 17, table())?;
        words.push(value);
        err += e;
        let block = i.div_ceil(2);
        let row = 2 * (sequence[block] - 1) - usize::from(i % 2 == 1);
        let mut weight = 1;
        for _ in 0..row * 8 {
            weight = weight * 3 % 211;
        }
        for width in pattern {
            checksum += width as usize * weight;
            weight = weight * 3 % 211;
        }
    }
    if check.0 % 211 != checksum % 211 {
        return None;
    }
    Some(Read {
        decoded: true,
        addon: None,
        format: "DataBarExpanded",
        text: payload(&words)?,
        start: s,
        end,
        error: err / crate::numeric::usize_f32(symbols + blocks),
        gs1: true,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separator_state_legacy_fixture() {
        // Extracted codewords from upstream rssexpanded-3/108.png, with
        // expected GS1 text from its independent upstream annotation.
        assert_eq!(
            payload(&[19, 683, 1576, 1030, 929, 771, 3096, 3717, 1360]).as_deref(),
            Some("10123456A1234A\u{1d}15991231")
        );
    }
    #[test]
    fn domain_covers_twelve_bits() {
        let mut values: Vec<_> = table().iter().map(|p| p.value).collect();
        values.sort_unstable();
        assert_eq!(values, (0..4096).collect::<Vec<_>>());
    }
}

#[derive(Clone)]
struct Block {
    finder: usize,
    left: (usize, [u8; 8], f32),
    right: Option<(usize, [u8; 8], f32)>,
    module: f32,
    lo: f32,
    hi: f32,
    top: f32,
    bottom: f32,
    angle: f32,
    reverse: bool,
    support: usize,
    line: usize,
}
#[derive(Default)]
pub struct Stacked {
    blocks: Vec<Block>,
}
impl Stacked {
    pub fn observe(&mut self, line: crate::databar::ScanLine<'_>) {
        let r = line.runs;
        for k in 0..r.len().saturating_sub(12) {
            let Some((finder, e)) = match_finder(&r[k + 8..], &FINDERS, false) else {
                continue;
            };
            let finder = finder + 1;
            if e > 0.15 {
                continue;
            }
            let Some(left) = lookup_character(&r[k..], 17, table()) else {
                continue;
            };
            let right = if k + 21 <= r.len() {
                lookup_character(
                    &r[k + 13..k + 21].iter().rev().copied().collect::<Vec<_>>(),
                    17,
                    table(),
                )
            } else {
                None
            };
            let end = k + if right.is_some() { 21 } else { 13 };
            let (lo, hi) = if line.reverse {
                (
                    line.length - line.offsets[end],
                    line.length - line.offsets[k],
                )
            } else {
                (line.offsets[k], line.offsets[end])
            };
            let (lo, hi) = (
                crate::numeric::usize_f32(lo) + line.start,
                crate::numeric::usize_f32(hi) + line.start,
            );
            if let Some(b) = self.blocks.iter_mut().find(|b| {
                b.finder == finder
                    && b.left.0 == left.0
                    && b.right.map(|p| p.0) == right.map(|p| p.0)
                    && b.reverse == line.reverse
                    && (b.angle - line.angle).abs() < 0.01
                    && (b.lo - lo).abs() < (hi - lo) * 0.2
                    && (b.hi - hi).abs() < (hi - lo) * 0.2
                    && line.cross - b.bottom <= line.step * 3.
            }) {
                if b.line != line.line {
                    b.support += 1;
                    b.line = line.line;
                    b.bottom = line.cross;
                    b.lo = b.lo.min(lo);
                    b.hi = b.hi.max(hi);
                }
            } else if self.blocks.len() < 2048 {
                self.blocks.push(Block {
                    finder,
                    left,
                    right,
                    module: r[k + 8..k + 13].iter().sum::<f32>() / 15.,
                    lo,
                    hi,
                    top: line.cross,
                    bottom: line.cross,
                    angle: line.angle,
                    reverse: line.reverse,
                    support: 1,
                    line: line.line,
                });
            }
        }
    }
    #[must_use]
    pub fn finish(self, step: f32) -> (Vec<crate::Detection>, bool) {
        let mut out = Vec::new();
        let mut attempts = 0;
        for (i, root) in self
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.finder == 1 && b.support >= 2 && b.right.is_some())
        {
            let symbols = root.left.0 / 211 + 4;
            if !(4..=22).contains(&symbols) {
                continue;
            }
            let sequence = SEQUENCES[(symbols - 3) / 2];
            let mut path = vec![i];
            self.search(sequence, symbols, &mut path, &mut attempts, &mut out, step);
            if attempts >= 10000 {
                break;
            }
        }
        let limited = attempts >= 10000 || self.blocks.len() >= 2048;
        (out, limited)
    }
    #[expect(
        clippy::too_many_lines,
        reason = "The recursive DataBar assembly shares path, checksum and attempt budget; splitting the search would obscure backtracking invariants."
    )]
    fn search(
        &self,
        sequence: &[usize],
        symbols: usize,
        path: &mut Vec<usize>,
        attempts: &mut usize,
        out: &mut Vec<crate::Detection>,
        step: f32,
    ) {
        *attempts += 1;
        if *attempts >= 10000 {
            return;
        }
        let root = &self.blocks[path[0]];
        if path.len() == symbols.div_ceil(2) {
            let mut words = Vec::new();
            let mut checksum = 0;
            for (i, &index) in path.iter().enumerate() {
                let b = &self.blocks[index];
                for (side, p) in [Some(b.left), b.right].into_iter().enumerate() {
                    if i == 0 && side == 0 || i * 2 + side >= symbols {
                        continue;
                    }
                    let Some(p) = p else {
                        return;
                    };
                    words.push(p.0);
                    let row = 2 * (b.finder - 1) - usize::from(side == 0);
                    let mut weight = 1;
                    for _ in 0..row * 8 {
                        weight = weight * 3 % 211;
                    }
                    for width in p.1 {
                        checksum += width as usize * weight;
                        weight = weight * 3 % 211;
                    }
                }
            }
            if checksum % 211 != root.left.0 % 211 {
                return;
            }
            let Some(text) = payload(&words) else {
                return;
            };
            let lo = path
                .iter()
                .map(|&i| self.blocks[i].lo)
                .fold(f32::INFINITY, f32::min);
            let hi = path
                .iter()
                .map(|&i| self.blocks[i].hi)
                .fold(f32::NEG_INFINITY, f32::max);
            let top = path
                .iter()
                .map(|&i| self.blocks[i].top)
                .fold(f32::INFINITY, f32::min)
                - step / 2.;
            let bottom = path
                .iter()
                .map(|&i| self.blocks[i].bottom)
                .fold(f32::NEG_INFINITY, f32::max)
                + step / 2.;
            let last = &self.blocks[*path.last().unwrap()];
            // Same-row symbols are handled by the direct reader. This assembly
            // is only for separated rows, avoiding a second geometry per read.
            if root.top.max(last.top) <= root.bottom.min(last.bottom) + step {
                return;
            }
            let c = root.angle.cos();
            let s = root.angle.sin();
            let polygon = [[lo, top], [hi, top], [hi, bottom], [lo, bottom]]
                .map(|[x, y]| [x * c - y * s, x * s + y * c]);
            let center = [
                ((lo + hi) * c - (top + bottom) * s) / 2.,
                f32::midpoint((lo + hi) * s, (top + bottom) * c),
            ];
            if out.iter().any(|d| {
                d.text == text
                    && center[0] >= d.polygon.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min)
                    && center[0]
                        <= d.polygon
                            .iter()
                            .map(|p| p[0])
                            .fold(f32::NEG_INFINITY, f32::max)
                    && center[1] >= d.polygon.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min)
                    && center[1]
                        <= d.polygon
                            .iter()
                            .map(|p| p[1])
                            .fold(f32::NEG_INFINITY, f32::max)
            }) {
                return;
            }
            out.push(crate::Detection {
                bytes: None,
                structured_append: None,
                reader_initialization: false,
                addon: None,
                format: "DataBarExpanded".into(),
                text,
                polygon,
                support: path.iter().map(|&i| self.blocks[i].support).sum(),
                error: path.iter().map(|&i| self.blocks[i].left.2).sum::<f32>()
                    / crate::numeric::usize_f32(path.len()),
                gs1: true,
            });
            return;
        }
        let previous = &self.blocks[*path.last().unwrap()];
        for (i, b) in self.blocks.iter().enumerate() {
            if b.finder != sequence[path.len()]
                || b.support < 2
                || path.contains(&i)
                || (b.angle - root.angle).abs() > 0.01
                || !(0.65..=1.5).contains(&(b.module / root.module))
            {
                continue;
            }
            let same_row = previous.top.max(b.top) <= previous.bottom.min(b.bottom) + step;
            if same_row {
                let gap = if b.reverse {
                    previous.lo - b.hi
                } else {
                    b.lo - previous.hi
                };
                if b.reverse != previous.reverse || gap.abs() > b.module * 6. {
                    continue;
                }
            } else {
                let progression = ((b.top + b.bottom) - (previous.top + previous.bottom)) / 2.
                    * if root.reverse { -1. } else { 1. };
                let height = (previous.bottom - previous.top).max(b.bottom - b.top);
                if progression <= 0.
                    || progression > height + b.module * 25.
                    || ((b.lo + b.hi) - (root.lo + root.hi)).abs()
                        > root.module * crate::numeric::usize_f32(symbols) * 55.
                {
                    continue;
                }
            }
            path.push(i);
            self.search(sequence, symbols, path, attempts, out, step);
            path.pop();
            if *attempts >= 10000 {
                return;
            }
        }
    }
}

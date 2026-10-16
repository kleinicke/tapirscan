//! Independent PDF417 row reader, metadata voting and payload compaction.
use crate::{Detection, pdf417_tables::CODES, reed_prime};
use std::{collections::BTreeMap, sync::OnceLock};

// PDF417 codewords are 17-bit patterns with a fixed one at the MSB and zero
// at the LSB. Removing those fixed bits leaves a dense 15-bit table index.
fn lookup(pattern: u32) -> Option<(usize, usize)> {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    if pattern >= (1 << 17) || pattern & 0x1_0000 == 0 || pattern & 1 != 0 {
        return None;
    }
    let table = TABLE.get_or_init(|| {
        let mut table = vec![0; 1 << 15];
        for (cluster, row) in CODES.iter().enumerate() {
            for (value, &pattern) in row.iter().enumerate() {
                assert!(pattern < (1 << 17) && pattern & 0x1_0000 != 0 && pattern & 1 == 0);
                let index = ((pattern & 0xffff) >> 1) as usize;
                let encoded = u16::try_from((cluster << 10) | value)
                    .expect("PDF417 cluster and value fit the packed lookup word")
                    + 1;
                assert_eq!(table[index], 0, "duplicate PDF417 codeword pattern");
                table[index] = encoded;
            }
        }
        table
    });
    let encoded = table[((pattern & 0xffff) >> 1) as usize];
    if encoded == 0 {
        return None;
    }
    let encoded = usize::from(encoded - 1);
    Some((encoded & 0x3ff, encoded >> 10))
}

#[cfg(test)]
fn legacy_lookup() -> Vec<(u32, usize, usize)> {
    let mut v = Vec::new();
    for (c, row) in CODES.iter().enumerate() {
        for (i, &p) in row.iter().enumerate() {
            v.push((p, i, c));
        }
    }
    v.sort_unstable();
    v
}
fn quantized_symbol(r: &[f32]) -> Option<(usize, usize, f32)> {
    if r.len() < 8 {
        return None;
    }
    let module = r[..8].iter().sum::<f32>() / 17.;
    if module < 0.65 {
        return None;
    }
    let mut widths: [usize; 8] =
        std::array::from_fn(|i| crate::numeric::f32_usize((r[i] / module).round().clamp(1., 6.)));
    let total = widths.iter().sum::<usize>();
    if !(15..=19).contains(&total) {
        return None;
    }
    while widths.iter().sum::<usize>() != 17 {
        let add = widths.iter().sum::<usize>() < 17;
        let i = (0..8)
            .filter(|&i| if add { widths[i] < 6 } else { widths[i] > 1 })
            .max_by(|&a, &b| {
                let score = |i: usize| {
                    if add {
                        r[i] / module - crate::numeric::usize_f32(widths[i])
                    } else {
                        crate::numeric::usize_f32(widths[i]) - r[i] / module
                    }
                };
                score(a).total_cmp(&score(b))
            })?;
        if add {
            widths[i] += 1;
        } else {
            widths[i] -= 1;
        }
    }
    let mut pattern = 0;
    for (i, &count) in widths.iter().enumerate() {
        for _ in 0..count {
            pattern = (pattern << 1) | u32::from(i % 2 == 0);
        }
    }
    let (value, cluster) = lookup(pattern)?;
    let pattern: [u8; 8] = widths.map(|v| (v).to_le_bytes()[0]);
    Some((value, cluster, crate::linear::pattern_error(r, &pattern)))
}
fn symbol(r: &[f32]) -> Option<(usize, usize)> {
    if r.len() < 8 {
        return None;
    }
    let module = r[..8].iter().sum::<f32>() / 17.;
    let mut candidates = [(0, 0, f32::INFINITY); 3];
    let mut count = 0;
    for gain in [0f32, -0.3, 0.3] {
        let adjusted: [f32; 8] =
            std::array::from_fn(|i| r[i] - gain * module * if i % 2 == 0 { 1. } else { -1. });
        if let Some((value, cluster, error)) = quantized_symbol(&adjusted) {
            let error = error + gain.abs() * 0.02;
            if error < 0.23 {
                candidates[count] = (value, cluster, error);
                count += 1;
            }
        }
    }
    let candidates = &mut candidates[..count];
    candidates.sort_by(|a, b| a.2.total_cmp(&b.2));
    let &(value, cluster, error) = candidates.first()?;
    if candidates
        .iter()
        .skip(1)
        .any(|&(v, c, e)| (v, c) != (value, cluster) && e - error < 0.012)
    {
        return None;
    }
    Some((value, cluster))
}
/// Closest codeword of a known cluster for eight run widths that no codeword
/// matches exactly (print gain, blur). The Reed-Solomon stage then treats a
/// wrong guess as an error instead of every such cell as an erasure.
/// Check codewords a group with guessed cells must leave unused (at most).
const SPARE: usize = 8;
/// Check codewords a decode with erasures must leave unused: spending every
/// check codeword on erasures leaves nothing to confirm the result.
const MIN_SPARE: usize = 2;
/// Most proposals per scan that get the costlier column-count re-read, for Low, Medium and
/// High (and Very High): about one read per dense sheet, at a tenth of a second there.
const MAX_TOLERANT: [usize; 3] = [0, 0, 48];
/// Marks a codeword value chosen by nearest-pattern search rather than exact match.
const GUESS: usize = 1024;
fn nearest_symbol(r: &[f32], cluster: usize) -> Option<usize> {
    if r.len() < 8 {
        return None;
    }
    let module = r[..8].iter().sum::<f32>() / 17.;
    if module < 0.65 {
        return None;
    }
    let mut best = (f32::INFINITY, 0);
    for (value, &pattern) in CODES.get(cluster)?.iter().enumerate() {
        let mut widths = [0f32; 8];
        let (mut index, mut previous) = (0, true);
        for bit in (0..17).rev().map(|shift| (pattern >> shift) & 1 == 1) {
            if bit != previous {
                index += 1;
                previous = bit;
            }
            widths[index] += 1.;
        }
        for gain in [0f32, -0.3, 0.3, -0.6, 0.6] {
            let error: f32 = (0..8)
                .map(|i| {
                    let adjusted = r[i] / module - gain * if i % 2 == 0 { 1. } else { -1. };
                    (adjusted - widths[i]).powi(2)
                })
                .sum::<f32>()
                + gain.abs() * 0.3;
            if error < best.0 {
                best = (error, value);
            }
        }
    }
    (best.0 < 1.5).then_some(best.1)
}
#[derive(Clone)]
struct Row {
    left: Option<usize>,
    right: Option<usize>,
    cluster: usize,
    values: Vec<Option<usize>>,
    x0: f32,
    x1: f32,
    y: f32,
}
// Subpixel edges move by at most half a pixel; a run or an eight-run span
// therefore changes by at most one pixel (plus float rounding). The accepted
// start guard's first run is 8 modules with at most 1 module error. Reject only
// integer-run bounds that cannot satisfy that existing check after refinement.
fn possible_start(offsets: &[usize], start: usize) -> bool {
    if start + 8 >= offsets.len() {
        return false;
    }
    let span = crate::numeric::usize_f32(offsets[start + 8] - offsets[start]);
    let margin = 2. + span * 1e-6;
    let low_module = (span - margin).max(0.) / 17.;
    let high_module = (span + margin) / 17.;
    let first = crate::numeric::usize_f32(offsets[start + 1] - offsets[start]);
    let last = crate::numeric::usize_f32(offsets[start + 8] - offsets[start + 7]);
    first + margin >= low_module * 7.
        && first - margin <= high_module * 9.
        && last + margin >= low_module * 2.
        && last - margin <= high_module * 4.
        && (start + 1..start + 7).all(|i| {
            crate::numeric::usize_f32(offsets[i + 1] - offsets[i]) - margin <= high_module * 2.
        })
}
fn accepted_start(offsets: &[usize], start: usize, gray: &[u8], reverse: bool) -> bool {
    if !possible_start(offsets, start) {
        return false;
    }
    let mut previous = crate::refined_edge(offsets[start], gray, reverse);
    let widths: [f32; 8] = std::array::from_fn(|i| {
        let edge = crate::refined_edge(offsets[start + i + 1], gray, reverse);
        let width = edge - previous;
        previous = edge;
        width
    });
    crate::linear::pattern_error(&widths, &[8, 1, 1, 1, 1, 1, 1, 3]) <= 0.18
}
pub(crate) fn left_indicator(gray: &[u8], mode: usize) -> Option<(usize, usize, f32)> {
    let bits = crate::threshold(gray, mode);
    let start = usize::from(!bits[0]);
    let offsets = crate::run_offsets(&bits);
    if !accepted_start(&offsets, start, gray, false) {
        return None;
    }
    let (r, _) = crate::refine_run_offsets(offsets, gray, false);
    if r.len() < start + 16
        || crate::linear::pattern_error(&r[start..], &[8, 1, 1, 1, 1, 1, 1, 3]) > 0.18
    {
        return None;
    }
    let (value, cluster) = symbol(&r[start + 8..])?;
    Some((
        value,
        cluster,
        r[start..start + 16].iter().sum::<f32>() / 34.,
    ))
}
#[cfg(not(target_arch = "wasm32"))]
#[must_use]
pub fn diagnostic_rows(gray: &[u8], w: usize, h: usize) -> serde_json::Value {
    let mut rows = Vec::new();
    for y in 0..h {
        let line = &gray[y * w..(y + 1) * w];
        let bits = crate::threshold(line, 0);
        for r in read_row(&bits, line, false, crate::numeric::usize_f32(y)) {
            rows.push(serde_json::json!({"y":y,"left":r.left,"right":r.right,
                "cluster":r.cluster,"values":r.values,"x0":r.x0,"x1":r.x1}));
        }
    }
    serde_json::json!(rows)
}
#[cfg(not(target_arch = "wasm32"))]
fn read_row(bits: &[bool], gray: &[u8], reversed: bool, y: f32) -> Vec<Row> {
    let mut offsets = Vec::new();
    let mut refined = Vec::new();
    read_row_reuse(bits, gray, reversed, y, None, &mut offsets, &mut refined)
}
fn read_row_reuse(
    bits: &[bool],
    gray: &[u8],
    reversed: bool,
    y: f32,
    columns: Option<usize>,
    offsets: &mut Vec<usize>,
    refined: &mut Vec<f32>,
) -> Vec<Row> {
    crate::run_offsets_into(bits, offsets);
    let first = usize::from(!bits[0]);
    let runs = offsets.len().saturating_sub(1);
    if !(first..runs.saturating_sub(24))
        .step_by(2)
        .any(|start| accepted_start(offsets, start, gray, reversed))
    {
        return Vec::new();
    }
    crate::refine_run_offsets_into(offsets, gray, reversed, refined);
    let r = refined.as_slice();
    let mut out = Vec::new();
    let mut s = usize::from(!bits[0]);
    while s + 25 <= r.len() {
        if crate::linear::pattern_error(&r[s..], &[8, 1, 1, 1, 1, 1, 1, 3]) > 0.18 {
            s += 2;
            continue;
        }
        let left_symbol = symbol(&r[s + 8..]);
        let mut at = s + 16;
        let mut values = Vec::new();
        while at + 9 <= r.len() && values.len() < 32 {
            let mut val = symbol(&r[at..]);
            if let (None, Some(_), Some((_, cluster))) = (val, columns, left_symbol) {
                val = nearest_symbol(&r[at..], cluster).map(|value| (value + GUESS, cluster));
            }
            if at + 17 <= r.len()
                && crate::linear::pattern_error(&r[at + 8..], &[7, 1, 1, 3, 1, 1, 1, 2, 1]) < 0.18
            {
                if let Some((_, cluster)) = left_symbol.or(val)
                    && !values.is_empty()
                    && left_symbol.is_none_or(|(_, c)| c == cluster)
                    && val.is_none_or(|(_, c)| c == cluster)
                {
                    out.push(Row {
                        left: left_symbol.map(|(v, _)| v),
                        right: val.map(|(v, _)| v),
                        cluster,
                        values: values
                            .iter()
                            .map(|&v: &Option<(usize, usize)>| {
                                v.and_then(|(v, c)| if c == cluster { Some(v) } else { None })
                            })
                            .collect(),
                        x0: crate::numeric::usize_f32(offsets[s]),
                        x1: crate::numeric::usize_f32(offsets[at + 17]),
                        y,
                    });
                }
                break;
            }
            values.push(val);
            // A symbol whose column count is known from its other rows ends
            // after that many cells, even when print damage hides the stop
            // guard of this row.
            if let (Some(count), Some((left, cluster))) = (columns, left_symbol)
                && values.len() == count
            {
                out.push(Row {
                    left: Some(left),
                    right: None,
                    cluster,
                    values: values
                        .iter()
                        .map(|v| v.and_then(|(value, c)| (c == cluster).then_some(value)))
                        .collect(),
                    x0: crate::numeric::usize_f32(offsets[s]),
                    x1: crate::numeric::usize_f32(offsets[at + 8]),
                    y,
                });
                break;
            }
            // Compact PDF417 ends after the data cells with a single-module
            // black bar. It has no right row indicator or full stop pattern.
            let module = r[at..at + 8].iter().sum::<f32>() / 17.;
            if r[at + 8] >= module * 0.5
                && r[at + 8] <= module * 1.8
                && (at + 9 == r.len() || r[at + 9] >= module * 1.75)
                && let Some((left, cluster)) = left_symbol
            {
                out.push(Row {
                    left: Some(left),
                    right: None,
                    cluster,
                    values: values
                        .iter()
                        .map(|v| v.and_then(|(value, c)| (c == cluster).then_some(value)))
                        .collect(),
                    x0: crate::numeric::usize_f32(offsets[s]),
                    x1: crate::numeric::usize_f32(offsets[at + 9]),
                    y,
                });
            }
            at += 8;
        }
        s += 2;
    }
    out
}
fn numeric(values: &[usize]) -> Option<Vec<u8>> {
    let mut decimal = vec![0u8];
    for &v in values {
        let mut carry = v;
        for d in decimal.iter_mut().rev() {
            let x = *d as usize * 900 + carry;
            *d = (x % 10).to_le_bytes()[0];
            carry = x / 10;
        }
        while carry > 0 {
            decimal.insert(0, (carry % 10).to_le_bytes()[0]);
            carry /= 10;
        }
    }
    if decimal.first() != Some(&1) {
        return None;
    }
    Some(decimal[1..].iter().map(|&d| d + b'0').collect())
}
fn text_value(
    v: usize,
    mode: &mut usize,
    shift: &mut Option<usize>,
    out: &mut Vec<u8>,
) -> Option<()> {
    let active = shift.take().unwrap_or(*mode);
    let c = match active {
        0 => match v {
            0..=25 => Some(b'A' + (v).to_le_bytes()[0]),
            26 => Some(b' '),
            27 => {
                *mode = 1;
                None
            }
            28 => {
                *mode = 2;
                None
            }
            29 => {
                *shift = Some(3);
                None
            }
            _ => return None,
        },
        1 => match v {
            0..=25 => Some(b'a' + (v).to_le_bytes()[0]),
            26 => Some(b' '),
            27 => {
                *shift = Some(0);
                None
            }
            28 => {
                *mode = 2;
                None
            }
            29 => {
                *shift = Some(3);
                None
            }
            _ => return None,
        },
        2 => match v {
            0..=24 => Some(b"0123456789&\r\t,:#-.$/+%*=^"[v]),
            25 => {
                *mode = 3;
                None
            }
            26 => Some(b' '),
            27 => {
                *mode = 1;
                None
            }
            28 => {
                *mode = 0;
                None
            }
            29 => {
                *shift = Some(3);
                None
            }
            _ => return None,
        },
        3 => match v {
            0..=28 => Some(b";<>@[\\]_`~!\r\t,:\n-.$/\"|*()?{}'"[v]),
            29 => {
                *mode = 0;
                None
            }
            _ => return None,
        },
        _ => return None,
    };
    if let Some(c) = c {
        out.push(c);
    }
    Some(())
}
#[must_use]
pub fn payload(code: &[usize]) -> Option<Vec<u8>> {
    Some(parse_payload(code)?.text.into_bytes())
}
struct DecodedPayload {
    bytes: Vec<u8>,
    text: String,
    reader_initialization: bool,
    structured_append: Option<crate::StructuredAppend>,
}
/// Text-compaction codewords as ASCII, as used by the Macro PDF417 file id.
fn text_codewords(values: &[usize]) -> Option<String> {
    let (mut mode, mut shift, mut out) = (0, None, Vec::new());
    for &v in values {
        text_value(v / 30, &mut mode, &mut shift, &mut out)?;
        text_value(v % 30, &mut mode, &mut shift, &mut out)?;
    }
    String::from_utf8(out).ok()
}
/// Macro PDF417 control block after a 928 codeword: five-digit segment index,
/// file id, optional tagged fields and the optional last-segment terminator
/// 922. Returns the next codeword position and the sequence position, which is
/// only exposed once the segment count is known (field 1 or a terminator).
fn macro_block(
    code: &[usize],
    mut at: usize,
    end: usize,
) -> Option<(usize, Option<crate::StructuredAppend>)> {
    let digits = numeric(&[*code.get(at)?, *code.get(at + 1)?])?;
    at += 2;
    if digits.len() != 5 || at > end {
        return None;
    }
    let index: usize = std::str::from_utf8(&digits).ok()?.parse().ok()?;
    let begin = at;
    while at < end && code[at] < 900 {
        at += 1;
    }
    let id = text_codewords(&code[begin..at])?;
    let mut count = None;
    let mut last = false;
    while at < end {
        match code[at] {
            923 => {
                let tag = *code.get(at + 1)?;
                at += 2;
                let begin = at;
                while at < end && code[at] < 900 {
                    at += 1;
                }
                if tag == 1 {
                    let digits = numeric(code.get(begin..at.min(begin + 15))?)?;
                    count = std::str::from_utf8(&digits).ok()?.parse::<usize>().ok();
                }
            }
            922 => {
                last = true;
                at += 1;
                break;
            }
            _ => break,
        }
    }
    let count = count.or(last.then_some(index + 1));
    Some((
        at,
        count.map(|count| crate::StructuredAppend {
            index: index + 1,
            count,
            id: Some(id),
            parity: None,
        }),
    ))
}
#[expect(
    clippy::too_many_lines,
    reason = "PDF417 compaction modes share a payload cursor, ECI and macro state in one sequential dispatcher."
)]
fn parse_payload(code: &[usize]) -> Option<DecodedPayload> {
    let length = *code.first()?;
    if length < 2 || length > code.len() {
        return None;
    }
    let mut at = 1;
    let mut out = Vec::new();
    let mut mode = 0;
    let mut shift = None;
    let mut eci = 3;
    let mut segment_start = 0;
    let mut text = String::new();
    let mut compaction = 900;
    let mut reader_initialization = false;
    let mut structured_append = None;
    while at < length {
        let mut c = code[at];
        at += 1;
        // An ECI boundary changes character interpretation, not the active
        // compaction mode. Continue byte/numeric segments after that boundary.
        if c < 900 && compaction != 900 {
            at -= 1;
            c = compaction;
        }
        match c {
            0..=899 => {
                text_value(c / 30, &mut mode, &mut shift, &mut out)?;
                text_value(c % 30, &mut mode, &mut shift, &mut out)?;
            }
            900 => {
                compaction = 900;
                mode = 0;
                shift = None;
            }
            913 => {
                // A pending punctuation shift can be the text padding before
                // a byte shift. The byte consumes it; it must not affect the
                // next text-compacted character.
                shift = None;
                let b = *code.get(at)?;
                if b > 255 {
                    return None;
                }
                out.push((b).to_le_bytes()[0]);
                at += 1;
            }
            901 | 924 => {
                compaction = c;
                let begin = at;
                while at < length && code[at] < 900 {
                    at += 1;
                }
                let values = &code[begin..at];
                // A byte-latch run whose length is a multiple of five is read
                // as raw bytes unless a value above 255 proves it is a group.
                let groups = if c == 901
                    && !(!values.is_empty()
                        && values.len().is_multiple_of(5)
                        && values[values.len() - 5..].iter().any(|&v| v > 255))
                {
                    values.len().saturating_sub(1) / 5
                } else {
                    values.len() / 5
                };
                for block in values[..groups * 5].as_chunks::<5>().0 {
                    let mut value = 0u64;
                    for &v in block {
                        value = value * 900 + v as u64;
                    }
                    if value >= 1u64 << 48 {
                        return None;
                    }
                    for i in (0..6).rev() {
                        out.push((value >> (i * 8)).to_le_bytes()[0]);
                    }
                }
                if c == 924 && !values.len().is_multiple_of(5) {
                    return None;
                }
                for &v in &values[groups * 5..] {
                    if v > 255 {
                        return None;
                    }
                    out.push((v).to_le_bytes()[0]);
                }
            }
            902 => {
                compaction = 902;
                while at < length && code[at] < 900 {
                    let start = at;
                    while at < length && at - start < 15 && code[at] < 900 {
                        at += 1;
                    }
                    out.extend(numeric(&code[start..at])?);
                }
            }
            925..=927 => {
                // Unknown character sets matter only when bytes follow them.
                if out.len() > segment_start {
                    text.push_str(&crate::encoding::decode(&out[segment_start..], Some(eci))?);
                }
                segment_start = out.len();
                let first = *code.get(at)?;
                (eci, at) = match c {
                    927 => (first, at + 1),
                    926 => (900 + first * 900 + *code.get(at + 1)?, at + 2),
                    _ => (810_900 + first, at + 1),
                };
            }
            928 => {
                let (next, sequence) = macro_block(code, at, length)?;
                at = next;
                structured_append = sequence;
            }
            921 if at == 2 => reader_initialization = true,
            _ => return None,
        }
    }
    if out.is_empty() {
        return None;
    }
    if out.len() > segment_start {
        text.push_str(&crate::encoding::decode(&out[segment_start..], Some(eci))?);
    }
    Some(DecodedPayload {
        bytes: out,
        text,
        reader_initialization,
        structured_append,
    })
}
fn group_metadata(rows: &[Row], strict: bool) -> Option<(usize, usize, usize)> {
    let mut votes: BTreeMap<(usize, usize, usize), usize> = BTreeMap::new();
    for r in rows {
        for (side, indicator) in [r.left, r.right].into_iter().enumerate() {
            let Some(indicator) = indicator else {
                continue;
            };
            let kind = match (r.cluster + if side == 1 { 2 } else { 0 }) % 3 {
                0 => 0,
                1 => 1,
                _ => 2,
            };
            let value = indicator % 30 + usize::from(kind == 2);
            *votes.entry((kind, value, 0)).or_default() += 1;
        }
    }
    let best = |kind| {
        let matching: Vec<_> = votes.iter().filter(|((k, _, _), _)| *k == kind).collect();
        let &((_, value, _), &support) = matching.iter().max_by_key(|(_, n)| *n)?;
        if strict && (support < 2 || support * 2 <= matching.iter().map(|(_, n)| **n).sum()) {
            return None;
        }
        Some(*value)
    };
    let upper = best(0)?;
    let lower = best(1)?;
    let cols = best(2)?;
    let count = upper * 3 + lower % 3 + 1;
    let level = lower / 3;
    if !(3..=90).contains(&count) || !(1..=30).contains(&cols) || level > 8 || count * cols > 928 {
        return None;
    }
    let ecc = 1usize << (level + 1);
    if ecc >= count * cols {
        return None;
    }
    Some((count, cols, ecc))
}
fn compact_region_valid(rows: &[Row]) -> bool {
    let Some((count, columns, _)) = group_metadata(rows, true) else {
        return false;
    };
    rows.iter()
        .filter(|r| {
            r.values.len() == columns && r.left.is_some_and(|v| v / 30 * 3 + r.cluster < count)
        })
        .count()
        * 2
        > rows.len()
}
fn decode_group(rows: &[Row]) -> Option<(DecodedPayload, usize)> {
    let meta = group_metadata(rows, false);
    let (count, cols, ecc) = meta?;
    let mut guessed = false;
    let mut cells = vec![BTreeMap::<usize, usize>::new(); count * cols];
    for r in rows {
        let index = r.left.or(r.right)? / 30 * 3 + r.cluster;
        if index >= count
            || r.values.len() != cols
            || r.left.zip(r.right).is_some_and(|(l, r)| l / 30 != r / 30)
        {
            continue;
        }
        for (i, v) in r.values.iter().enumerate() {
            if let Some(v) = v {
                guessed |= *v >= GUESS;
                let weight = if *v >= GUESS { 1 } else { 4 };
                *cells[index * cols + i].entry(*v % GUESS).or_default() += weight;
            }
        }
    }
    let mut erasures = Vec::new();
    let mut code: Vec<usize> = cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if let Some((&v, _)) = c.iter().max_by_key(|(_, n)| *n) {
                v
            } else {
                erasures.push(i);
                0
            }
        })
        .collect();
    // Guessed cells make a wrong decode likelier, so a guessed group must
    // leave spare check codewords unused: that many extra zero syndromes make
    // a chance match of garbage negligible.
    let spare = if guessed {
        SPARE
    } else if erasures.is_empty() {
        0
    } else {
        MIN_SPARE
    }
    .min(ecc);
    let consumed = |locations: usize, erased: usize| 2 * locations - erased;
    let mut attempt = code.clone();
    let mut corrected = reed_prime::correct(&mut attempt, ecc, &erasures)
        .filter(|&n| consumed(n, erasures.len()) + spare <= ecc);
    if corrected.is_some() {
        code = attempt;
    } else if guessed {
        // Nearest-pattern guesses can be wrong. Erase the least certain cells
        // too: an erasure costs one correction where an error costs two.
        let mut order: Vec<usize> = (0..cells.len()).filter(|i| !erasures.contains(i)).collect();
        let margin = |i: usize| {
            let mut votes: Vec<usize> = cells[i].values().copied().collect();
            votes.sort_unstable_by(|a, b| b.cmp(a));
            votes[0] - votes.get(1).copied().unwrap_or(0)
        };
        order.sort_by_key(|&i| margin(i));
        let room = ecc.saturating_sub(spare + erasures.len());
        for extra in (1..=12).map(|i| (room * i / 12).max(1)) {
            if erasures.len() + extra + spare > ecc {
                break;
            }
            let mut all = erasures.clone();
            all.extend(order.iter().take(extra));
            let mut attempt = code.clone();
            for &i in &all {
                attempt[i] = 0;
            }
            if let Some(count) = reed_prime::correct(&mut attempt, ecc, &all)
                .filter(|&n| consumed(n, all.len()) + spare <= ecc)
            {
                code = attempt;
                corrected = Some(count);
                break;
            }
        }
    }
    let corrected = corrected?;
    if code[0] != count * cols - ecc {
        return None;
    }
    let pp = parse_payload(&code);
    Some((pp?, corrected))
}
#[expect(
    clippy::too_many_lines,
    reason = "Row caching, bidirectional decoding and spatial grouping share traversal state; exact-output pilots preserve that order."
)]
fn detect_axes(
    gray: &[u8],
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    target_scanlines: usize,
    columns: Option<usize>,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    // Thresholds depend only on the sampled row bytes and mode. Keep the last
    // row from each axis pass: repeated rows are common for flat backgrounds
    // and this avoids recomputing global/local thresholds without changing
    // traversal, threshold mode order, or any downstream grouping state.
    for vertical in [false, true] {
        let (width, height) = if vertical { (h, w) } else { (w, h) };
        for threshold in 0..2 {
            let mut groups: Vec<Vec<Row>> = Vec::new();
            let mut row = Vec::with_capacity(width);
            let mut previous_row = Vec::new();
            let mut previous_bits = Vec::new();
            let mut run_offsets = Vec::new();
            let mut refined_runs = Vec::new();
            for y in (0..height).step_by((height / target_scanlines).max(1)) {
                row.clear();
                row.extend((0..width).map(|x| {
                    if vertical {
                        gray[x * w + y]
                    } else {
                        gray[y * w + x]
                    }
                }));
                // A constant scanline cannot contain a start guard. Avoid
                // thresholding and run extraction for both directions.
                if row.iter().all(|&value| value == row[0]) {
                    continue;
                }
                let same_row = previous_row == row;
                if same_row {
                    // The two-direction loop leaves the working bitmap
                    // reversed. Restore it before reusing it next time.
                    previous_bits.reverse();
                } else {
                    crate::threshold_into(&row, threshold, &mut previous_bits);
                }
                let bits = &mut previous_bits;
                for reversed in [false, true] {
                    if reversed {
                        bits.reverse();
                    }
                    for mut r in read_row_reuse(
                        bits,
                        &row,
                        reversed,
                        crate::numeric::usize_f32(y),
                        columns,
                        &mut run_offsets,
                        &mut refined_runs,
                    ) {
                        if reversed {
                            let x0 = crate::numeric::usize_f32(width) - r.x1;
                            r.x1 = crate::numeric::usize_f32(width) - r.x0;
                            r.x0 = x0;
                        }
                        let span = r.x1 - r.x0;
                        if let Some(g) = groups.iter_mut().find(|g| {
                            let p = &g[g.len() - 1];
                            p.values.len() == r.values.len()
                                && (p.x0 - r.x0).abs() < span * 0.15
                                && (p.x1 - r.x1).abs() < span * 0.15
                                && r.y - p.y < span * 0.2
                        }) {
                            g.push(r);
                        } else {
                            groups.push(vec![r]);
                        }
                    }
                }
                std::mem::swap(&mut row, &mut previous_row);
            }
            for group in groups {
                if group.len() < 3 {
                    continue;
                }
                let x0 = group.iter().map(|r| r.x0).fold(f32::INFINITY, f32::min);
                let x1 = group.iter().map(|r| r.x1).fold(f32::NEG_INFINITY, f32::max);
                let y0 = group.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
                let y1 = group.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
                let polygon = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                    .map(|p| if vertical { [p[1], p[0]] } else { p });
                if group.iter().any(|r| r.right.is_some()) || compact_region_valid(&group) {
                    regions.add("PDF417", polygon, 1., group.len());
                }
                if let Some((read, corrected)) = decode_group(&group) {
                    let text = read.text;
                    let x0 = group.iter().map(|r| r.x0).fold(f32::INFINITY, f32::min);
                    let x1 = group.iter().map(|r| r.x1).fold(f32::NEG_INFINITY, f32::max);
                    let y0 = group.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
                    let y1 = group.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
                    let polygon = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
                        .map(|p| if vertical { [p[1], p[0]] } else { p });
                    if !results.iter().any(|r| {
                        r.text == text
                            && (r.polygon[0][0] - polygon[0][0])
                                .hypot(r.polygon[0][1] - polygon[0][1])
                                < (x1 - x0) * 0.3
                    }) {
                        results.push(Detection {
                            bytes: Some(read.bytes),
                            structured_append: read.structured_append,
                            reader_initialization: read.reader_initialization,
                            addon: None,
                            format: "PDF417".into(),
                            text,
                            polygon,
                            support: group.len(),
                            error: crate::numeric::usize_f32(corrected),
                            gs1: false,
                        });
                    }
                }
            }
        }
    }
    (results, false)
}

// Coverage uses the candidate's area, so one decoded small symbol cannot
// suppress a retry of an aggregate band containing several other symbols.
fn covered_fraction(candidate: &[[f32; 2]; 4], decoded: &[[f32; 2]; 4]) -> f32 {
    let area = |quad: &[[f32; 2]; 4]| {
        (0..4)
            .map(|i| quad[i][0] * quad[(i + 1) % 4][1] - quad[i][1] * quad[(i + 1) % 4][0])
            .sum::<f32>()
            .abs()
            * 0.5
    };
    let candidate_area = area(candidate);
    if !candidate_area.is_finite() || candidate_area <= 0. {
        return 0.;
    }
    crate::regions::overlap(candidate, decoded) * candidate_area.min(area(decoded)) / candidate_area
}

/// Column count voted by the left row indicators of an already rectified crop.
fn crop_columns(gray: &[u8], w: usize, h: usize) -> Option<usize> {
    let mut votes = [0usize; 30];
    let step = (h / 100).max(1);
    for y in (0..h).step_by(step) {
        let line = &gray[y * w..(y + 1) * w];
        let mirrored: Vec<u8> = line.iter().rev().copied().collect();
        for line in [line, &mirrored] {
            for mode in 0..2 {
                if let Some((value, 2, _)) = left_indicator(line, mode) {
                    votes[value % 30] += 1;
                }
            }
        }
    }
    let (best, &count) = votes.iter().enumerate().max_by_key(|(_, count)| **count)?;
    (count >= 3 && count * 2 > votes.iter().sum::<usize>()).then_some(best + 1)
}
/// Whole-image rows plus guard-guided rectification at arbitrary in-plane angles, with the
/// retry limits of a scanner `mode` (0 Low, 1 Medium, 2 High, 3 Very High).
pub fn detect(
    gray: &[u8],
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
    mode: usize,
) -> (Vec<Detection>, bool) {
    let mut coarse_regions = crate::regions::Regions::default();
    let (mut results, mut limited) = detect_axes(gray, w, h, &mut coarse_regions, 96, None);
    // A localized band without a covering payload may contain dense rows or
    // multiple symbols. Retry the original full-frame density, even if another
    // barcode has already decoded elsewhere. Sparse search is an effort
    // policy, not proof that an absent format is impossible.
    let dense_retry = coarse_regions.limited
        || coarse_regions.candidates.iter().any(|region| {
            !results
                .iter()
                .any(|read| covered_fraction(&region.polygon, &read.polygon) >= 0.85)
        });
    for region in coarse_regions.candidates {
        regions.add(
            "PDF417",
            region.polygon,
            region.localization_score,
            region.support,
        );
    }
    limited |= coarse_regions.limited;
    if dense_retry {
        let (dense_results, dense_limited) = detect_axes(gray, w, h, regions, 500, None);
        limited |= dense_limited;
        for read in dense_results {
            if !results.iter().any(|existing| {
                existing.text == read.text
                    && crate::regions::overlap(&existing.polygon, &read.polygon) > 0.65
            }) {
                results.push(read);
            }
        }
    }
    let (proposals, capped) = crate::pdf_localize::proposals(gray, w, h);
    limited |= capped;
    let mut tolerant_attempts = 0;
    for (polygon, support) in proposals {
        if results
            .iter()
            .any(|d| crate::regions::overlap(&d.polygon, &polygon) > 0.7)
        {
            continue;
        }
        regions.add("PDF417", polygon, 1., support);
        let Some((pixels, width, height, t)) = crate::pdf_localize::rectify(gray, w, h, polygon)
        else {
            limited = true;
            continue;
        };
        let mut crop_regions = crate::regions::Regions::default();
        let (mut found, capped) = detect_axes(&pixels, width, height, &mut crop_regions, 500, None);
        limited |= capped || crop_regions.limited;
        if found.is_empty()
            && tolerant_attempts < MAX_TOLERANT[mode.min(2)]
            && let Some(columns) = crop_columns(&pixels, width, height)
        {
            tolerant_attempts += 1;
            // The guards localized a symbol the strict row reader could not
            // finish: read each row for the column count its indicators give.
            let (tolerant, capped) = detect_axes(
                &pixels,
                width,
                height,
                &mut crop_regions,
                500,
                Some(columns),
            );
            limited |= capped;
            found = tolerant;
        }
        for mut read in found {
            read.polygon = read.polygon.map(|p| crate::qr_detect::map(&t, p[0], p[1]));
            if !results.iter().any(|d| {
                d.text == read.text && crate::regions::overlap(&d.polygon, &read.polygon) > 0.65
            }) {
                results.push(read);
            }
        }
    }
    // The rectified proposal pass can add a new full-frame PDF region after
    // the coarse decision. Give that region one dense whole-image attempt,
    // unless the same bounded dense pass already ran above. Keep the test
    // PDF-only because `regions` is shared with every matrix reader.
    if !dense_retry
        && regions.candidates.iter().any(|region| {
            region.format == "PDF417"
                && !results
                    .iter()
                    .any(|read| covered_fraction(&region.polygon, &read.polygon) >= 0.85)
        })
    {
        let (dense_results, dense_limited) = detect_axes(gray, w, h, regions, 500, None);
        limited |= dense_limited;
        for read in dense_results {
            if !results.iter().any(|existing| {
                existing.text == read.text
                    && crate::regions::overlap(&existing.polygon, &read.polygon) > 0.65
            }) {
                results.push(read);
            }
        }
    }
    drop_conflicting_corrections(&mut results);
    (results, limited)
}

/// Two different payloads decoded at the same place cannot both be right.
/// Low error-correction levels let a damaged row group "correct" into another
/// valid codeword, so the read that needed more corrections is dropped.
fn drop_conflicting_corrections(results: &mut Vec<Detection>) {
    let keep: Vec<bool> = results
        .iter()
        .map(|read| {
            read.error == 0.
                || !results.iter().any(|other| {
                    other.text != read.text
                        && other.error < read.error
                        && crate::regions::overlap(&other.polygon, &read.polygon) > 0.5
                })
        })
        .collect();
    let mut keep = keep.into_iter();
    results.retain(|_| keep.next().unwrap_or(true));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widths(pattern: u32) -> [f32; 8] {
        let mut out = [0f32; 8];
        let (mut index, mut previous) = (0, true);
        for bit in (0..17).rev().map(|shift| (pattern >> shift) & 1 == 1) {
            if bit != previous {
                index += 1;
                previous = bit;
            }
            out[index] += 1.;
        }
        out
    }

    #[test]
    fn nearest_symbol_recovers_codewords_through_print_gain() {
        for (cluster, value) in [(0, 5), (1, 400), (2, 928)] {
            let mut run = widths(CODES[cluster][value]);
            for (i, width) in run.iter_mut().enumerate() {
                // Ink spread: bars grow, spaces shrink, scale is 3 px/module.
                *width = (*width + if i % 2 == 0 { 0.4 } else { -0.4 }) * 3.;
            }
            assert_eq!(nearest_symbol(&run, cluster), Some(value));
        }
    }

    #[test]
    fn nearest_symbol_rejects_runs_far_from_any_codeword() {
        assert_eq!(nearest_symbol(&[2., 9., 2., 9., 2., 9., 2., 9.], 0), None);
    }

    #[test]
    fn macro_control_block_reports_the_sequence_position() {
        // Text "AB", then 928, index 00000 (codewords 111 100), file id "AB",
        // and the last-segment terminator 922.
        let parsed = parse_payload(&[7, 1, 928, 111, 100, 1, 922]).expect("macro block parses");
        assert_eq!(parsed.text, "AB");
        let append = parsed.structured_append.expect("segment count is known");
        assert_eq!((append.index, append.count), (1, 1));
        assert_eq!(append.id.as_deref(), Some("AB"));
    }

    #[test]
    fn macro_block_without_a_known_count_is_not_a_sequence() {
        let parsed = parse_payload(&[6, 1, 928, 111, 100, 1]).expect("macro block parses");
        assert!(parsed.structured_append.is_none());
    }

    #[test]
    fn eci_codeword_before_a_text_run_selects_its_charset() {
        let parsed = parse_payload(&[4, 927, 3, 1]).expect("ECI 3 parses");
        assert_eq!(parsed.text, "AB");
    }

    #[test]
    fn conflicting_reads_keep_the_one_with_fewer_corrections() {
        let read = |text: &str, x: f32, error: f32| Detection {
            bytes: None,
            structured_append: None,
            reader_initialization: false,
            addon: None,
            format: "PDF417".into(),
            text: text.into(),
            polygon: [[x, 0.], [x + 100., 0.], [x + 100., 40.], [x, 40.]],
            support: 20,
            error,
            gs1: false,
        };
        let mut results = vec![
            read("right", 0., 0.),
            read("miscorrected", 5., 1.),
            read("elsewhere", 300., 2.),
            read("also uncorrected", 3., 0.),
        ];
        drop_conflicting_corrections(&mut results);
        let texts: Vec<_> = results.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["right", "elsewhere", "also uncorrected"]);
    }

    #[test]
    fn dense_lookup_matches_sorted_reference_for_every_pattern() {
        let reference = legacy_lookup();
        for pattern in 0..(1_u32 << 17) {
            let expected = reference
                .binary_search_by_key(&pattern, |entry| entry.0)
                .ok()
                .map(|index| (reference[index].1, reference[index].2));
            assert_eq!(lookup(pattern), expected, "pattern {pattern:#07x}");
        }
        for row in CODES {
            for &pattern in &row {
                assert!(lookup(pattern).is_some());
            }
        }
    }

    #[test]
    fn dense_lookup_rejects_invalid_fixed_bits() {
        assert_eq!(lookup(0), None);
        assert_eq!(lookup(0x1_0001), None);
        assert_eq!(lookup(0xffff), None);
        for row in CODES {
            for &pattern in &row {
                assert_eq!(lookup(pattern | (1 << 20)), None);
            }
        }
    }

    #[test]
    fn one_small_decode_does_not_cover_a_multisymbol_retry_band() {
        let band = [[0., 0.], [100., 0.], [100., 500.], [0., 500.]];
        let single = [[0., 0.], [100., 0.], [100., 100.], [0., 100.]];
        assert!(covered_fraction(&band, &single) < 0.21);
        assert!(covered_fraction(&single, &band) > 0.99);
        assert!(covered_fraction(&band, &band) > 0.99);
    }

    #[test]
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "Bounded test generator uses positive module widths and mirrors the integer guard's production casts."
    )]
    fn integer_guard_bounds_preserve_subpixel_accepted_starts() {
        let pattern = [8, 1, 1, 1, 1, 1, 1, 3];
        let mut seed = 48721_u32;
        let mut accepted = 0;
        for module in 1..=8 {
            for case in 0..512 {
                let mut random = || {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    seed
                };
                let mut widths = vec![5];
                widths.extend(
                    pattern.map(|n| (n * module + (random() % 3) as i32 - 1).max(1) as usize),
                );
                widths.push(5);
                let mut bits = Vec::new();
                let mut gray = Vec::new();
                for (i, width) in widths.into_iter().enumerate() {
                    let black = i % 2 == 1;
                    for _ in 0..width {
                        bits.push(black);
                        gray.push(if black {
                            (random() % 80) as u8
                        } else {
                            176 + (random() % 80) as u8
                        });
                    }
                }
                let reverse = case % 2 == 1;
                if reverse {
                    gray.reverse();
                }
                let (refined, offsets) = crate::refined_runs(&bits, &gray, reverse);
                let expected =
                    crate::linear::pattern_error(&refined[1..], &[8, 1, 1, 1, 1, 1, 1, 3]) <= 0.18;
                assert_eq!(accepted_start(&offsets, 1, &gray, reverse), expected);
                if expected {
                    accepted += 1;
                    assert!(possible_start(&offsets, 1));
                }
            }
        }
        assert!(accepted > 1000);
    }
    #[test]
    fn byte_compaction_continues_after_an_eci_boundary() {
        assert_eq!(
            payload(&[8, 901, 65, 927, 3, 66, 67, 900]),
            Some(b"ABC".to_vec())
        );
    }
    #[test]
    fn initialization_is_leading_metadata() {
        let read = parse_payload(&[3, 921, 1]).unwrap();
        assert_eq!(read.text, "AB");
        assert!(read.reader_initialization);
        assert!(parse_payload(&[4, 1, 921, 900]).is_none());
    }
    #[test]
    fn byte_shift_consumes_text_padding_shift() {
        assert_eq!(
            payload(&[5, 29, 913, 29, 13 * 30 + 4]),
            Some(b"A\x1dNE".to_vec())
        );
    }
    #[test]
    fn character_sets_change_at_eci_boundaries() {
        assert_eq!(
            payload(&[9, 927, 7, 913, 0xb0, 927, 3, 913, 0xa3]),
            Some("А£".as_bytes().to_vec())
        );
    }
}

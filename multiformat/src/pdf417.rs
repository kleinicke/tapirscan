//! Independent PDF417 row reader, metadata voting and payload compaction.
use crate::{pdf417_tables::CODES, reed_prime, Detection};
use std::{collections::BTreeMap, sync::OnceLock};
fn lookup() -> &'static Vec<(u32, usize, usize)> {
    static TABLE: OnceLock<Vec<(u32, usize, usize)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v = Vec::new();
        for (c, row) in CODES.iter().enumerate() {
            for (i, &p) in row.iter().enumerate() {
                v.push((p, i, c));
            }
        }
        v.sort_unstable();
        v
    })
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
        std::array::from_fn(|i| (r[i] / module).round().clamp(1., 6.) as usize);
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
                        r[i] / module - widths[i] as f32
                    } else {
                        widths[i] as f32 - r[i] / module
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
    let table = lookup();
    let i = table.binary_search_by_key(&pattern, |p| p.0).ok()?;
    let pattern: [u8; 8] = widths.map(|v| v as u8);
    Some((
        table[i].1,
        table[i].2,
        crate::linear::pattern_error(r, &pattern),
    ))
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
    let span = (offsets[start + 8] - offsets[start]) as f32;
    let margin = 2. + span * 1e-6;
    let low_module = (span - margin).max(0.) / 17.;
    let high_module = (span + margin) / 17.;
    let first = (offsets[start + 1] - offsets[start]) as f32;
    let last = (offsets[start + 8] - offsets[start + 7]) as f32;
    first + margin >= low_module * 7.
        && first - margin <= high_module * 9.
        && last + margin >= low_module * 2.
        && last - margin <= high_module * 4.
        && (start + 1..start + 7)
            .all(|i| (offsets[i + 1] - offsets[i]) as f32 - margin <= high_module * 2.)
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
        for r in read_row(&bits, line, false, y as f32) {
            rows.push(serde_json::json!({"y":y,"left":r.left,"right":r.right,
                "cluster":r.cluster,"values":r.values,"x0":r.x0,"x1":r.x1}));
        }
    }
    serde_json::json!(rows)
}
fn read_row(bits: &[bool], gray: &[u8], reversed: bool, y: f32) -> Vec<Row> {
    let offsets = crate::run_offsets(bits);
    let first = usize::from(!bits[0]);
    let runs = offsets.len().saturating_sub(1);
    if !(first..runs.saturating_sub(24))
        .step_by(2)
        .any(|start| accepted_start(&offsets, start, gray, reversed))
    {
        return Vec::new();
    }
    let (r, offsets) = crate::refine_run_offsets(offsets, gray, reversed);
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
            let val = symbol(&r[at..]);
            if at + 17 <= r.len()
                && crate::linear::pattern_error(&r[at + 8..], &[7, 1, 1, 3, 1, 1, 1, 2, 1]) < 0.18
            {
                if let Some((_, cluster)) = left_symbol.or(val) {
                    if !values.is_empty()
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
                            x0: offsets[s] as f32,
                            x1: offsets[at + 17] as f32,
                            y,
                        });
                    }
                }
                break;
            }
            values.push(val);
            // Compact PDF417 ends after the data cells with a single-module
            // black bar. It has no right row indicator or full stop pattern.
            let module = r[at..at + 8].iter().sum::<f32>() / 17.;
            if r[at + 8] >= module * 0.5
                && r[at + 8] <= module * 1.8
                && (at + 9 == r.len() || r[at + 9] >= module * 1.75)
            {
                if let Some((left, cluster)) = left_symbol {
                    out.push(Row {
                        left: Some(left),
                        right: None,
                        cluster,
                        values: values
                            .iter()
                            .map(|v| v.and_then(|(value, c)| (c == cluster).then_some(value)))
                            .collect(),
                        x0: offsets[s] as f32,
                        x1: offsets[at + 9] as f32,
                        y,
                    });
                }
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
            *d = (x % 10) as u8;
            carry = x / 10;
        }
        while carry > 0 {
            decimal.insert(0, (carry % 10) as u8);
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
            0..=25 => Some(b'A' + v as u8),
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
            0..=25 => Some(b'a' + v as u8),
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
    text: String,
    reader_initialization: bool,
}
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
                out.push(b as u8);
                at += 1;
            }
            901 | 924 => {
                compaction = c;
                let begin = at;
                while at < length && code[at] < 900 {
                    at += 1;
                }
                let values = &code[begin..at];
                let groups = if c == 901 {
                    values.len().saturating_sub(1) / 5
                } else {
                    values.len() / 5
                };
                for block in values[..groups * 5].chunks_exact(5) {
                    let mut value = 0u64;
                    for &v in block {
                        value = value * 900 + v as u64;
                    }
                    if value >= 1u64 << 48 {
                        return None;
                    }
                    for i in (0..6).rev() {
                        out.push((value >> (i * 8)) as u8);
                    }
                }
                if c == 924 && !values.len().is_multiple_of(5) {
                    return None;
                }
                for &v in &values[groups * 5..] {
                    if v > 255 {
                        return None;
                    }
                    out.push(v as u8);
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
            927 => {
                text.push_str(&crate::encoding::decode(&out[segment_start..], Some(eci))?);
                segment_start = out.len();
                eci = *code.get(at)?;
                at += 1;
            }
            921 if at == 2 => reader_initialization = true,
            _ => return None,
        }
    }
    if out.is_empty() {
        return None;
    }
    text.push_str(&crate::encoding::decode(&out[segment_start..], Some(eci))?);
    Some(DecodedPayload {
        text,
        reader_initialization,
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
    let (count, cols, ecc) = group_metadata(rows, false)?;
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
                *cells[index * cols + i].entry(*v).or_default() += 1;
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
    let corrected = reed_prime::correct(&mut code, ecc, &erasures)?;
    if code[0] != count * cols - ecc {
        return None;
    }
    Some((parse_payload(&code)?, corrected))
}
fn detect_axes(
    gray: &[u8],
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
) -> (Vec<Detection>, bool) {
    let mut results: Vec<Detection> = Vec::new();
    for vertical in [false, true] {
        let (width, height) = if vertical { (h, w) } else { (w, h) };
        for threshold in 0..2 {
            let mut groups: Vec<Vec<Row>> = Vec::new();
            for y in (0..height).step_by((height / 500).max(1)) {
                let row: Vec<u8> = (0..width)
                    .map(|x| {
                        if vertical {
                            gray[x * w + y]
                        } else {
                            gray[y * w + x]
                        }
                    })
                    .collect();
                let mut bits = crate::threshold(&row, threshold);
                for reversed in [false, true] {
                    if reversed {
                        bits.reverse();
                    }
                    for mut r in read_row(&bits, &row, reversed, y as f32) {
                        if reversed {
                            let x0 = width as f32 - r.x1;
                            r.x1 = width as f32 - r.x0;
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
            }
            for group in groups {
                if group.len() < 3 {
                    continue;
                }
                let x0 = group.iter().map(|r| r.x0).fold(f32::INFINITY, f32::min);
                let x1 = group.iter().map(|r| r.x1).fold(f32::NEG_INFINITY, f32::max);
                let y0 = group.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
                let y1 = group.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
                let polygon = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| {
                    if vertical {
                        [p[1], p[0]]
                    } else {
                        p
                    }
                });
                if group.iter().any(|r| r.right.is_some()) || compact_region_valid(&group) {
                    regions.add("PDF417", polygon, 1., group.len());
                }
                if let Some((read, corrected)) = decode_group(&group) {
                    let text = read.text;
                    let x0 = group.iter().map(|r| r.x0).fold(f32::INFINITY, f32::min);
                    let x1 = group.iter().map(|r| r.x1).fold(f32::NEG_INFINITY, f32::max);
                    let y0 = group.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
                    let y1 = group.iter().map(|r| r.y).fold(f32::NEG_INFINITY, f32::max);
                    let polygon = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| {
                        if vertical {
                            [p[1], p[0]]
                        } else {
                            p
                        }
                    });
                    if !results.iter().any(|r| {
                        r.text == text
                            && (r.polygon[0][0] - polygon[0][0])
                                .hypot(r.polygon[0][1] - polygon[0][1])
                                < (x1 - x0) * 0.3
                    }) {
                        results.push(Detection {
                            structured_append: None,
                            reader_initialization: read.reader_initialization,
                            addon: None,
                            format: "PDF417".into(),
                            text,
                            polygon,
                            support: group.len(),
                            error: corrected as f32,
                            gs1: false,
                        });
                    }
                }
            }
        }
    }
    (results, false)
}

/// Whole-image rows plus guard-guided rectification at arbitrary in-plane angles.
pub fn detect(
    gray: &[u8],
    w: usize,
    h: usize,
    regions: &mut crate::regions::Regions,
) -> (Vec<Detection>, bool) {
    let (mut results, mut limited) = detect_axes(gray, w, h, regions);
    let (proposals, capped) = crate::pdf_localize::proposals(gray, w, h);
    limited |= capped;
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
        let (found, capped) = detect_axes(&pixels, width, height, &mut crop_regions);
        limited |= capped || crop_regions.limited;
        for mut read in found {
            read.polygon = read.polygon.map(|p| crate::qr_detect::map(&t, p[0], p[1]));
            if !results.iter().any(|d| {
                d.text == read.text && crate::regions::overlap(&d.polygon, &read.polygon) > 0.65
            }) {
                results.push(read);
            }
        }
    }
    (results, limited)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
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

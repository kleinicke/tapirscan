//! Independent GS1 `DataBar` Omnidirectional width/rank decoder.
//! Standard group/finder tables are referenced from Zint's BSD-licensed encoder;
//! composition enumeration and decoding here are project-owned.
use crate::linear::{pattern_error, Read};
use std::sync::OnceLock;
const FINDERS: [[u8; 5]; 9] = [
    [3, 8, 2, 1, 1],
    [3, 5, 5, 1, 1],
    [3, 3, 7, 1, 1],
    [3, 1, 9, 1, 1],
    [2, 7, 4, 1, 1],
    [2, 5, 6, 1, 1],
    [2, 3, 8, 1, 1],
    [1, 5, 7, 1, 1],
    [1, 3, 9, 1, 1],
];
const GROUPS: [(usize, usize, usize, usize, usize); 9] = [
    (0, 1, 12, 4, 8),
    (161, 10, 10, 6, 6),
    (961, 34, 8, 8, 4),
    (2015, 70, 6, 10, 3),
    (2715, 126, 4, 12, 1),
    (0, 4, 5, 10, 2),
    (336, 20, 7, 8, 4),
    (1036, 48, 9, 6, 6),
    (1516, 81, 11, 4, 8),
];
const WEIGHTS: [[usize; 8]; 4] = [
    [1, 3, 9, 27, 2, 6, 18, 54],
    [4, 12, 36, 29, 8, 24, 72, 58],
    [16, 48, 65, 37, 32, 17, 51, 74],
    [64, 34, 23, 69, 49, 68, 46, 59],
];
#[derive(Clone)]
pub(crate) struct Pattern {
    pub key: u32,
    pub value: usize,
    pub widths: [u8; 8],
}
pub(crate) fn compositions(sum: usize, max: usize, one_required: bool) -> Vec<[u8; 4]> {
    let mut out = Vec::new();
    for a in 1..=max {
        for b in 1..=max {
            for c in 1..=max {
                if a + b + c >= sum {
                    continue;
                }
                let d = sum - a - b - c;
                if d <= max && (!one_required || [a, b, c, d].contains(&1)) {
                    out.push([
                        (a).to_le_bytes()[0],
                        (b).to_le_bytes()[0],
                        (c).to_le_bytes()[0],
                        (d).to_le_bytes()[0],
                    ]);
                }
            }
        }
    }
    out
}
pub(crate) fn key(widths: [u8; 8]) -> u32 {
    widths.iter().fold(0, |v, &n| (v << 4) | u32::from(n))
}
fn tables() -> &'static [Vec<Pattern>; 2] {
    static TABLES: OnceLock<[Vec<Pattern>; 2]> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut out = [Vec::new(), Vec::new()];
        for (i, &(base, multiplier, odd_sum, even_sum, odd_max)) in GROUPS.iter().enumerate() {
            let inside = i >= 5;
            let odd = compositions(odd_sum, odd_max, inside);
            let even = compositions(even_sum, 9 - odd_max, !inside);
            for (a, o) in odd.iter().enumerate() {
                for (b, e) in even.iter().enumerate() {
                    let value = base
                        + if inside {
                            b * multiplier + a
                        } else {
                            a * multiplier + b
                        };
                    let end = if i == 4 {
                        2841
                    } else if i == 8 {
                        1597
                    } else {
                        GROUPS[i + 1].0
                    };
                    if (inside && a >= multiplier) || (!inside && b >= multiplier) || value >= end {
                        continue;
                    }
                    let widths =
                        std::array::from_fn(|j| if j % 2 == 0 { o[j / 2] } else { e[j / 2] });
                    out[usize::from(inside)].push(Pattern {
                        key: key(widths),
                        value,
                        widths,
                    });
                }
            }
        }
        for table in &mut out {
            table.sort_by_key(|p| p.key);
        }
        out
    })
}
pub(crate) fn lookup_character(
    r: &[f32],
    units: usize,
    table: &[Pattern],
) -> Option<(usize, [u8; 8], f32)> {
    if r.len() < 8 {
        return None;
    }
    let module = r[..8].iter().sum::<f32>() / crate::numeric::usize_f32(units);
    if module < 0.65 {
        return None;
    }
    let mut widths: [u8; 8] =
        std::array::from_fn(|i| crate::numeric::f32_u8((r[i] / module).round().clamp(1., 8.)));
    let sum = widths.iter().map(|&v| v as usize).sum::<usize>();
    if sum.abs_diff(units) > 2 {
        return None;
    }
    while widths.iter().map(|&v| v as usize).sum::<usize>() != units {
        let add = widths.iter().map(|&v| v as usize).sum::<usize>() < units;
        let i = (0..8)
            .filter(|&i| if add { widths[i] < 8 } else { widths[i] > 1 })
            .max_by(|&a, &b| {
                let e = |i: usize| {
                    if add {
                        r[i] / module - f32::from(widths[i])
                    } else {
                        f32::from(widths[i]) - r[i] / module
                    }
                };
                e(a).total_cmp(&e(b))
            })?;
        if add {
            widths[i] += 1;
        } else {
            widths[i] -= 1;
        }
    }
    let index = table.binary_search_by_key(&key(widths), |p| p.key).ok()?;
    let p = &table[index];
    let error = pattern_error(r, &p.widths);
    (error < 0.22).then_some((p.value, p.widths, error))
}
fn character(r: &[f32], inside: bool) -> Option<(usize, [u8; 8], f32)> {
    lookup_character(
        r,
        if inside { 15 } else { 16 },
        &tables()[usize::from(inside)],
    )
}
/// Shared-scale evaluation for the `DataBar` families' fifteen-module finders.
/// Every supplied pattern has five widths and a pair of unit widths at an end.
/// The error accumulation order matches `pattern_error`, including reverse scans.
pub(crate) fn match_finder(r: &[f32], patterns: &[[u8; 5]], reverse: bool) -> Option<(usize, f32)> {
    if r.len() < 5 {
        return None;
    }
    let sum = r[..5].iter().sum::<f32>();
    let module = sum / 15.;
    if module < 0.65 {
        return None;
    }
    let unit = |i: usize| (r[i] - module).abs() <= module;
    let left_unit = unit(0) && unit(1);
    let right_unit = unit(3) && unit(4);
    if !(left_unit || right_unit) {
        return None;
    }
    let mut best = (0, 10_f32);
    for (i, p) in patterns.iter().enumerate() {
        let first = if reverse { [p[4], p[3]] } else { [p[0], p[1]] };
        let last = if reverse { [p[1], p[0]] } else { [p[3], p[4]] };
        // A candidate whose required unit pair is on a side that failed the
        // existing necessary gate cannot satisfy all five d <= module tests.
        // Patterns without a unit pair retain the generic path's behavior.
        if (first == [1, 1] && !left_unit) || (last == [1, 1] && !right_unit) {
            continue;
        }
        let mut e = 0.;
        let mut valid = true;
        for j in 0..5 {
            let d = (r[j] - f32::from(p[if reverse { 4 - j } else { j }]) * module).abs();
            if d > module {
                valid = false;
                break;
            }
            e += d;
        }
        if valid {
            e /= sum;
        } else {
            e = 10.;
        }
        if e < best.1 {
            best = (i, e);
        }
    }
    (best.1 < 10.).then_some(best)
}

#[cfg(test)]
mod finder_gate_tests {
    use super::*;

    fn reference(r: &[f32], patterns: &[[u8; 5]], reverse: bool) -> Option<(usize, f32)> {
        if r.len() < 5 {
            return None;
        }
        let sum = r[..5].iter().sum::<f32>();
        let module = sum / 15.;
        if module < 0.65 {
            return None;
        }
        let unit = |i: usize| (r[i] - module).abs() <= module;
        if !(unit(0) && unit(1) || unit(3) && unit(4)) {
            return None;
        }
        let mut best = (0, 10_f32);
        for (i, p) in patterns.iter().enumerate() {
            let mut e = 0.;
            let mut valid = true;
            for j in 0..5 {
                let d = (r[j] - f32::from(p[if reverse { 4 - j } else { j }]) * module).abs();
                if d > module {
                    valid = false;
                    break;
                }
                e += d;
            }
            if valid {
                e /= sum;
            } else {
                e = 10.;
            }
            if e < best.1 {
                best = (i, e);
            }
        }
        (best.1 < 10.).then_some(best)
    }

    #[test]
    fn gate_rejects_only_impossible_unit_sides() {
        let patterns = [[3, 8, 2, 1, 1], [1, 3, 9, 1, 1]];
        let right = [6., 16., 4., 2., 2.];
        assert!(match_finder(&right, &patterns, false).is_some());
        let left = [2., 2., 4., 16., 6.];
        assert!(match_finder(&left, &patterns, true).is_some());
        let neither = [6., 16., 4., 6., 16.];
        assert!(match_finder(&neither, &patterns, false).is_none());
    }

    #[test]
    fn gate_matches_original_on_databar_expanded_and_generic_patterns() {
        let expanded: [[u8; 5]; 12] = [
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
        let patterns: Vec<_> = FINDERS.into_iter().chain(expanded).collect();
        let generic = [[2, 3, 4, 5, 6], [4, 2, 3, 5, 6]];
        let mut seed = 193_u32;
        for _ in 0..10000 {
            let row: [f32; 5] = std::array::from_fn(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from(u16::try_from(seed % 5000).unwrap()) / 100.
            });
            for reverse in [false, true] {
                for table in [patterns.as_slice(), generic.as_slice()] {
                    assert_eq!(
                        match_finder(&row, table, reverse),
                        reference(&row, table, reverse)
                    );
                }
            }
        }
        let perturbations = [-1.001_f32, -1., -0.999, -0.25, 0., 0.25, 0.999, 1., 1.001];
        for module in [0.65_f32, 0.7, 1., 3., 20.] {
            for delta in perturbations {
                let row = [
                    3. * module + delta * module,
                    8. * module - delta * module,
                    2. * module + delta * module * 0.5,
                    module + delta * module,
                    module - delta * module,
                ];
                for reverse in [false, true] {
                    assert_eq!(
                        match_finder(&row, &patterns, reverse),
                        reference(&row, &patterns, reverse)
                    );
                    assert_eq!(
                        match_finder(&row, &generic, reverse),
                        reference(&row, &generic, reverse)
                    );
                }
            }
        }
    }
}
fn finder(r: &[f32], reverse: bool) -> Option<(usize, f32)> {
    let best = match_finder(r, &FINDERS, reverse)?;
    (best.1 < 0.15).then_some(best)
}
#[must_use]
pub fn decode(runs: &[f32], s: usize) -> Option<Read> {
    if s + 45 > runs.len() {
        return None;
    }
    let (left_finder, le) = finder(&runs[s + 9..], false)?;
    let (right_finder, re) = finder(&runs[s + 30..], true)?;
    let mut inside_left = runs[s + 14..s + 22].to_vec();
    inside_left.reverse();
    let mut outside_right = runs[s + 35..s + 43].to_vec();
    outside_right.reverse();
    let outside_left = character(&runs[s + 1..], false)?;
    let b = character(&inside_left, true)?;
    let c = character(&outside_right, false)?;
    let d = character(&runs[s + 22..], true)?;
    let chars = [outside_left, b, c, d];
    let mut checksum = chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            c.1.iter()
                .enumerate()
                .map(|(j, &w)| w as usize * WEIGHTS[i][j])
                .sum::<usize>()
        })
        .sum::<usize>()
        % 79;
    if checksum >= 8 {
        checksum += 1;
    }
    if checksum >= 72 {
        checksum += 1;
    }
    if checksum != left_finder * 9 + right_finder {
        return None;
    }
    let module = runs[s + 9..s + 14].iter().sum::<f32>() / 15.;
    if (runs[s] - module).abs() > module
        || (runs[s + 43] - module).abs() > module
        || (runs[s + 44] - module).abs() > module
    {
        return None;
    }
    let value = (chars[0].0 as u64 * 1597 + chars[1].0 as u64) * 4_537_077
        + chars[2].0 as u64 * 1597
        + chars[3].0 as u64;
    if value >= 20_000_000_000_000 {
        return None;
    }
    let digits = format!("{:013}", value % 10_000_000_000_000);
    let check = (10
        - digits
            .bytes()
            .rev()
            .enumerate()
            .map(|(i, d)| (d - b'0') as usize * if i % 2 == 0 { 3 } else { 1 })
            .sum::<usize>()
            % 10)
        % 10;
    Some(Read {
        decoded: true,
        addon: None,
        format: "DataBar",
        text: format!("01{digits}{check}"),
        start: s,
        end: s + 45,
        error: (le + re + chars.iter().map(|c| c.2).sum::<f32>()) / 6.,
        gs1: true,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_character_domains_are_complete_and_unique() {
        let tables = tables();
        assert_eq!(tables[0].len(), 2841);
        assert_eq!(tables[1].len(), 1597);
        for table in tables {
            let mut values: Vec<_> = table.iter().map(|p| p.value).collect();
            values.sort_unstable();
            assert_eq!(values, (0..table.len()).collect::<Vec<_>>());
        }
    }
}

struct Half {
    side: usize,
    finder: usize,
    value: u64,
    checksum: usize,
    start: usize,
    end: usize,
    error: f32,
}
fn halves(r: &[f32], first_black: bool) -> Vec<Half> {
    let mut out = Vec::new();
    for s in (usize::from(!first_black)..r.len().saturating_sub(22)).step_by(2) {
        for side in 0..2 {
            let end = s + if side == 0 { 23 } else { 25 };
            if end > r.len() {
                continue;
            }
            let at = s + if side == 0 { 9 } else { 10 };
            let Some((f, fe)) = finder(&r[at..], side == 1) else {
                continue;
            };
            let module = r[at..at + 5].iter().sum::<f32>() / 15.;
            let guard = |v: f32| (v - module).abs() <= module * 0.65;
            if !guard(r[s]) || !guard(r[end - 1]) || (side == 1 && !guard(r[s + 1])) {
                continue;
            }
            let (outside, inside) = if side == 0 {
                (
                    character(&r[s + 1..], false),
                    character(
                        &r[s + 14..s + 22].iter().rev().copied().collect::<Vec<_>>(),
                        true,
                    ),
                )
            } else {
                (
                    character(
                        &r[s + 15..s + 23].iter().rev().copied().collect::<Vec<_>>(),
                        false,
                    ),
                    character(&r[s + 2..], true),
                )
            };
            let (Some(a), Some(b)) = (outside, inside) else {
                continue;
            };
            let checksum = [a.1, b.1]
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    p.iter()
                        .enumerate()
                        .map(|(j, &v)| v as usize * WEIGHTS[side * 2 + i][j])
                        .sum::<usize>()
                })
                .sum::<usize>();
            out.push(Half {
                side,
                finder: f,
                value: a.0 as u64 * 1597 + b.0 as u64,
                checksum,
                start: s,
                end,
                error: (fe + a.2 + b.2) / 3.,
            });
        }
    }
    out
}
struct HalfGroup {
    half: Half,
    lo: f32,
    hi: f32,
    top: f32,
    bottom: f32,
    angle: f32,
    support: usize,
    line: usize,
}
#[derive(Default)]
pub struct Stacked {
    groups: Vec<HalfGroup>,
}
#[derive(Clone, Copy)]
pub struct ScanLine<'a> {
    pub runs: &'a [f32],
    pub offsets: &'a [usize],
    pub first_black: bool,
    pub reverse: bool,
    pub length: usize,
    pub start: f32,
    pub cross: f32,
    pub angle: f32,
    pub line: usize,
    pub step: f32,
}
impl Stacked {
    pub fn observe(&mut self, line: ScanLine<'_>) {
        for h in halves(line.runs, line.first_black) {
            let (lo, hi) = if line.reverse {
                (
                    line.length - line.offsets[h.end],
                    line.length - line.offsets[h.start],
                )
            } else {
                (line.offsets[h.start], line.offsets[h.end])
            };
            let (lo, hi) = (
                crate::numeric::usize_f32(lo) + line.start,
                crate::numeric::usize_f32(hi) + line.start,
            );
            if let Some(g) = self.groups.iter_mut().find(|g| {
                g.half.side == h.side
                    && g.half.value == h.value
                    && g.half.finder == h.finder
                    && (g.angle - line.angle).abs() < 0.01
                    && (g.lo - lo).abs() < (hi - lo) * 0.2
                    && (g.hi - hi).abs() < (hi - lo) * 0.2
                    && line.cross - g.bottom <= line.step * 3.
            }) {
                if g.line != line.line {
                    g.support += 1;
                    g.line = line.line;
                    g.bottom = line.cross;
                    g.lo = g.lo.min(lo);
                    g.hi = g.hi.max(hi);
                }
            } else if self.groups.len() < 4096 {
                self.groups.push(HalfGroup {
                    half: h,
                    lo,
                    hi,
                    top: line.cross,
                    bottom: line.cross,
                    angle: line.angle,
                    support: 1,
                    line: line.line,
                });
            }
        }
    }
    #[must_use]
    pub fn finish(self, step: f32) -> (Vec<crate::Detection>, bool) {
        let mut out: Vec<crate::Detection> = Vec::new();
        let limited = self.groups.len() >= 4096;
        for a in self
            .groups
            .iter()
            .filter(|g| g.half.side == 0 && g.support >= 2)
        {
            for b in self
                .groups
                .iter()
                .filter(|g| g.half.side == 1 && g.support >= 2)
            {
                let width = (a.hi - a.lo).min(b.hi - b.lo);
                let overlap = a.hi.min(b.hi) - a.lo.max(b.lo);
                let gap = (a.top.max(b.top) - a.bottom.min(b.bottom)).max(0.);
                if (a.angle - b.angle).abs() > 0.01
                    || overlap < width * 0.75
                    || ((a.hi - a.lo) / (b.hi - b.lo) - 1.).abs() > 0.25
                    || gap > width * 1.5
                    || a.top.max(b.top) < a.bottom.min(b.bottom) - step
                {
                    continue;
                }
                let mut check = (a.half.checksum + b.half.checksum) % 79;
                if check >= 8 {
                    check += 1;
                }
                if check >= 72 {
                    check += 1;
                }
                if check != a.half.finder * 9 + b.half.finder {
                    continue;
                }
                let value = a.half.value * 4_537_077 + b.half.value;
                if value >= 20_000_000_000_000 {
                    continue;
                }
                let digits = format!("{:013}", value % 10_000_000_000_000);
                let check = (10
                    - digits
                        .bytes()
                        .rev()
                        .enumerate()
                        .map(|(i, d)| (d - b'0') as usize * if i % 2 == 0 { 3 } else { 1 })
                        .sum::<usize>()
                        % 10)
                    % 10;
                let text = format!("01{digits}{check}");
                let lo = a.lo.min(b.lo);
                let hi = a.hi.max(b.hi);
                let top = a.top.min(b.top) - step / 2.;
                let bottom = a.bottom.max(b.bottom) + step / 2.;
                let c = a.angle.cos();
                let s = a.angle.sin();
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
                    continue;
                }
                out.push(crate::Detection {
                    bytes: None,
                    structured_append: None,
                    reader_initialization: false,
                    addon: None,
                    format: "DataBar".into(),
                    text,
                    polygon,
                    support: a.support + b.support,
                    error: f32::midpoint(a.half.error, b.half.error),
                    gs1: true,
                });
            }
        }
        (out, limited)
    }
}

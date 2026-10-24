//! Independent run-length decoders. Tables describe standardized symbol patterns;
//! no `ZXing` reader implementation or runtime is used.

// Format bits as the linear decoders name them; the values come from the
// generated `crate::formats`.
pub const EAN13: u32 = crate::formats::EAN13;
pub const UPCA: u32 = crate::formats::UPCA;
pub const EAN8: u32 = crate::formats::EAN8;
pub const UPCE: u32 = crate::formats::UPCE;
pub const CODE128: u32 = crate::formats::CODE128;
pub const CODE39: u32 = crate::formats::CODE39;
pub const ITF: u32 = crate::formats::ITF;
pub const CODABAR: u32 = crate::formats::CODABAR;
pub const CODE93: u32 = crate::formats::CODE93;
pub const DATABAR: u32 = crate::formats::DATA_BAR;
pub const DATABAR_EXPANDED: u32 = crate::formats::DATA_BAR_EXPANDED;
pub const DATABAR_LIMITED: u32 = crate::formats::DATA_BAR_LIMITED;
pub const ADDON_READ: u32 = crate::formats::EAN_ADDON_READ_FLAG;
pub const ADDON_REQUIRE: u32 = crate::formats::EAN_ADDON_REQUIRE_FLAG;
pub const MSI_PLESSEY: u32 = crate::formats::MSI_PLESSEY;
pub const INDUSTRIAL25: u32 = crate::formats::INDUSTRIAL25;
pub const IATA25: u32 = crate::formats::IATA25;
pub const MATRIX25: u32 = crate::formats::MATRIX25;
/// Pharmacode has no start or stop: callers select it only for runs in the image's
/// reading direction (see [`pharmacode_direction`]).
pub const PHARMACODE: u32 = crate::formats::PHARMACODE;

/// Whether runs sampled along image direction `(dx, dy)` (y downward) are in
/// Pharmacode reading order. A Pharmacode value depends on the reading direction, which
/// the symbol does not show, so the image defines it: along the diagonal half-plane
/// `dx + dy > 0`, that is left to right for horizontal symbols and top to bottom for
/// vertical ones. Each scan line is read in exactly one direction; the switch lies at
/// symbols read along the 45-degree diagonal from bottom left to top right on the
/// screen (their bars run from top left to bottom right), away from the upright and
/// quarter-turn orientations.
#[must_use]
pub fn pharmacode_direction(dx: f64, dy: f64) -> bool {
    let along = dx + dy;
    along > 0. || (along >= 0. && dx > 0.)
}

pub(crate) const DIGITS: [[u8; 4]; 10] = [
    [3, 2, 1, 1],
    [2, 2, 2, 1],
    [2, 1, 2, 2],
    [1, 4, 1, 1],
    [1, 1, 3, 2],
    [1, 2, 3, 1],
    [1, 1, 1, 4],
    [1, 3, 1, 2],
    [1, 2, 1, 3],
    [3, 1, 1, 2],
];
/// Left-half parity of EAN-13 per leading digit; bit 5 is the first digit, set for G codes.
pub(crate) const PARITY: [u8; 10] = [0, 11, 13, 14, 19, 25, 28, 21, 22, 26];
/// UPC-E parity per check digit (number system 0); bit 5 is the first digit, set for G codes.
pub(crate) const UPC_PARITY: [u8; 10] = [56, 52, 50, 49, 44, 38, 35, 42, 41, 37];
/// EAN-5 parity pattern per check value; bit 4 is the first digit, set for G codes.
pub(crate) const EAN5_PARITY: [u8; 10] = [24, 20, 18, 17, 12, 6, 3, 10, 9, 5];

/// Parity pattern an EAN-2 or EAN-5 supplement of `digits` must show (bit 1 or 4
/// is the first digit, set for G codes); `None` for other lengths.
pub(crate) fn supplement_parity(digits: &[u8]) -> Option<u8> {
    let d: Vec<u32> = digits.iter().map(|&x| u32::from(x)).collect();
    match d.len() {
        2 => u8::try_from((d[0] * 10 + d[1]) % 4).ok(),
        5 => {
            let check = (3 * (d[0] + d[2] + d[4]) + 9 * (d[1] + d[3])) % 10;
            Some(EAN5_PARITY[check as usize])
        }
        _ => None,
    }
}
const C128: [&[u8; 6]; 107] = [
    b"212222", b"222122", b"222221", b"121223", b"121322", b"131222", b"122213", b"122312",
    b"132212", b"221213", b"221312", b"231212", b"112232", b"122132", b"122231", b"113222",
    b"123122", b"123221", b"223211", b"221132", b"221231", b"213212", b"223112", b"312131",
    b"311222", b"321122", b"321221", b"312212", b"322112", b"322211", b"212123", b"212321",
    b"232121", b"111323", b"131123", b"131321", b"112313", b"132113", b"132311", b"211313",
    b"231113", b"231311", b"112133", b"112331", b"132131", b"113123", b"113321", b"133121",
    b"313121", b"211331", b"231131", b"213113", b"213311", b"213131", b"311123", b"311321",
    b"331121", b"312113", b"312311", b"332111", b"314111", b"221411", b"431111", b"111224",
    b"111422", b"121124", b"121421", b"141122", b"141221", b"112214", b"112412", b"122114",
    b"122411", b"142112", b"142211", b"241211", b"221114", b"413111", b"241112", b"134111",
    b"111242", b"121142", b"121241", b"114212", b"124112", b"124211", b"411212", b"421112",
    b"421211", b"212141", b"214121", b"412121", b"111143", b"111341", b"131141", b"114113",
    b"114311", b"411113", b"411311", b"113141", b"114131", b"311141", b"411131", b"211412",
    b"211214", b"211232", b"233111",
];
const C39_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%*";
const C39: [u16; 44] = [
    0x034, 0x121, 0x061, 0x160, 0x031, 0x130, 0x070, 0x025, 0x124, 0x064, 0x109, 0x049, 0x148,
    0x019, 0x118, 0x058, 0x00d, 0x10c, 0x04c, 0x01c, 0x103, 0x043, 0x142, 0x013, 0x112, 0x052,
    0x007, 0x106, 0x046, 0x016, 0x181, 0x0c1, 0x1c0, 0x091, 0x190, 0x0d0, 0x085, 0x184, 0x0c4,
    0x0a8, 0x0a2, 0x08a, 0x02a, 0x094,
];
/// Smallest module width, in pixels, that the run-length matchers accept.
pub(crate) const MIN_MODULE: f32 = 0.65;
pub(crate) const ITF_DIGITS: [u8; 10] = [6, 17, 9, 24, 5, 20, 12, 3, 18, 10];
const CODA: [u8; 20] = [
    3, 6, 9, 96, 18, 66, 33, 36, 48, 72, 12, 24, 69, 81, 84, 21, 26, 41, 11, 14,
];
const CODA_ALPHABET: &[u8] = b"0123456789-$:/.+ABCD";
/// Codabar start and stop characters in reading order, indexed by
/// `(start - 16) * 4 + stop - 16`.
const CODA_START_STOP: [&str; 16] = [
    "AA", "AB", "AC", "AD", "BA", "BB", "BC", "BD", "CA", "CB", "CC", "CD", "DA", "DB", "DC", "DD",
];
const C93: [u16; 48] = [
    0x114, 0x148, 0x144, 0x142, 0x128, 0x124, 0x122, 0x150, 0x112, 0x10a, 0x1a8, 0x1a4, 0x1a2,
    0x194, 0x192, 0x18a, 0x168, 0x164, 0x162, 0x134, 0x11a, 0x158, 0x14c, 0x146, 0x12c, 0x116,
    0x1b4, 0x1b2, 0x1ac, 0x1a6, 0x196, 0x19a, 0x16c, 0x166, 0x136, 0x13a, 0x12e, 0x1d4, 0x1d2,
    0x1ca, 0x16e, 0x176, 0x1ae, 0x126, 0x1da, 0x1d6, 0x132, 0x15e,
];
const C93_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-. $/+%";

fn c93_pattern(bits: u16) -> [u8; 6] {
    let mut out = [0; 6];
    let mut at = 0;
    let mut old = 1;
    for i in (0..9).rev() {
        let b = (bits >> i) & 1;
        if b != old {
            at += 1;
            old = b;
        }
        out[at] += 1;
    }
    out
}
/// Per-character error accepted without further evidence; the shared bound of the
/// run-length matchers.
const C93_CHARACTER_ERROR: f32 = 0.23;
/// Larger per-character error accepted only inside a symbol that passes both
/// mod-47 checks (C and K): blur, ink spread and perspective accumulate over long
/// symbols, and one marginal character otherwise discards the whole symbol.
const C93_WEAK_CHARACTER_ERROR: f32 = 0.3;
/// Match one Code 93 character. Returns its value, error and whether the match is
/// weak (accepted only inside a symbol that passes both checks). Weak matches are
/// tried only for a character as wide as the previous one (`previous`, pixels).
fn c93_match(r: &[f32], previous: f32) -> Option<(usize, f32, bool)> {
    let mut best = (0, 10.);
    let mut second = 10.;
    for (i, &bits) in C93.iter().enumerate() {
        let e = error(r, &c93_pattern(bits));
        if e < best.1 {
            second = best.1;
            best = (i, e);
        } else {
            second = second.min(e);
        }
    }
    if best.1 < C93_CHARACTER_ERROR && second - best.1 > 0.012 {
        return Some((best.0, best.1, false));
    }
    if !similar_width(r, previous) {
        return None;
    }
    if best.1 < C93_WEAK_CHARACTER_ERROR && second - best.1 > 0.012 {
        return Some((best.0, best.1, true));
    }
    c93_edge_match(r).map(|(v, e)| (v, e, true))
}
/// Whether the six elements at `r` span within a quarter of `previous` pixels: the
/// edge matchers run only where the character pitch continues, not on merged or
/// split runs.
fn similar_width(r: &[f32], previous: f32) -> bool {
    r.len() >= 6 && (r[..6].iter().sum::<f32>() - previous).abs() <= previous * 0.25
}
/// Match by the four sums of adjacent element pairs (edge-to-similar-edge
/// distances). Uniform ink spread widens every bar by as much as it narrows every
/// space, so these sums keep their nominal module counts; the 48 characters have
/// distinct sums.
fn c93_edge_match(r: &[f32]) -> Option<(usize, f32)> {
    let (best, second) = edge_ranked(r, 9, C93.iter().map(|&bits| c93_pattern(bits)))?;
    (best.1 < C93_EDGE_ERROR && second - best.1 > C93_EDGE_MARGIN).then_some(best)
}
/// The best match of the six elements at `r` among six-element `patterns` of
/// `modules` modules each, by the four sums of adjacent elements, with the
/// second-best error. A sum more than three quarters of a module off rules a
/// pattern out.
fn edge_ranked(
    r: &[f32],
    modules: u8,
    patterns: impl Iterator<Item = [u8; 6]>,
) -> Option<((usize, f32), f32)> {
    if r.len() < 6 {
        return None;
    }
    let total: f32 = r[..6].iter().sum();
    let module = total / f32::from(modules);
    if module < MIN_MODULE {
        return None;
    }
    let pairs: [f32; 4] = std::array::from_fn(|i| r[i] + r[i + 1]);
    let mut best = (0, 10f32);
    let mut second = 10f32;
    for (i, p) in patterns.enumerate() {
        let mut e = 0.;
        for (j, &pair) in pairs.iter().enumerate() {
            let d = (pair - f32::from(p[j] + p[j + 1]) * module).abs();
            if d > module * 0.75 {
                e = 10.;
                break;
            }
            e += d;
        }
        let e = e / total;
        if e < best.1 {
            second = best.1;
            best = (i, e);
        } else {
            second = second.min(e);
        }
    }
    Some((best, second))
}
const C93_EDGE_ERROR: f32 = 0.15;
const C93_EDGE_MARGIN: f32 = 0.02;
/// Quiet zone required on both sides of a Code 93 symbol, in modules: less than the
/// specified ten, which the two check characters make safe.
const C93_QUIET_MODULES: f32 = 3.;
#[expect(
    clippy::too_many_lines,
    reason = "The symbol parser keeps start/stop, parity, checksum and work-limit transitions together for the same run cursor."
)]
fn code93(
    runs: &[f32],
    start_index: usize,
    retain_failed: bool,
    limited: &mut bool,
) -> Option<Read> {
    let module = runs[start_index..start_index + 6].iter().sum::<f32>() / 9.;
    if start_index == 0 || (start_index != 1 && runs[start_index - 1] < module * C93_QUIET_MODULES)
    {
        return None;
    }
    // Ink spread shifts every edge of the start pattern alike: a near miss is
    // accepted when its spread-corrected fit is close.
    let start = c93_pattern(C93[47]);
    let plain = error(&runs[start_index..], &start);
    if plain > 0.2 && (plain > 0.45 || error_adjusted(&runs[start_index..], &start) > 0.2) {
        return None;
    }
    let mut at = start_index + 6;
    let mut values = Vec::new();
    let mut err = 0.;
    // Characters accepted only by the relaxed or edge matchers rely on both checks
    // and never produce a retained failed-checksum candidate.
    let mut weak = 0usize;
    while at + 7 <= runs.len() && values.len() < 256 {
        // Weak matches only after a spread start or two characters read, and at an
        // unchanged pitch.
        let previous = if values.len() >= 2 || plain > 0.2 {
            runs[at - 6..at].iter().sum::<f32>()
        } else {
            0.
        };
        let (v, e, marginal) = c93_match(&runs[at..], previous)?;
        err += e;
        weak += usize::from(marginal);
        if v == 47 {
            // Perspective changes the module along the symbol: judge the
            // termination bar and trailing quiet zone at the stop character's scale.
            let stop_module = runs[at..at + 6].iter().sum::<f32>() / 9.;
            if values.len() < 3
                || !quiet(
                    runs,
                    start_index,
                    at + 7,
                    module.min(stop_module),
                    C93_QUIET_MODULES,
                )
                || (runs[at + 6] - stop_module).abs() > stop_module * 0.8
            {
                return None;
            }
            let mut valid = true;
            for cap in [15, 20] {
                let check = values.pop()?;
                if values
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, &v)| v * (i % cap + 1))
                    .sum::<usize>()
                    % 47
                    != check
                {
                    valid = false;
                }
            }
            if !valid {
                return (retain_failed && weak == 0).then_some(Read {
                    decoded: false,
                    addon: None,
                    codabar_start_stop: None,
                    format: "Code93",
                    text: String::new(),
                    start: start_index,
                    end: at + 7,
                    error: err / crate::numeric::usize_f32(values.len() + 3),
                    gs1: false,
                });
            }
            let mut out = String::new();
            let mut i = 0;
            while i < values.len() {
                let v = values[i];
                if v < 43 {
                    out.push(C93_ALPHABET[v] as char);
                } else {
                    let n: usize = *values.get(i + 1)?;
                    if !(10..36).contains(&n) {
                        return None;
                    }
                    let letter = b'A' + (n - 10).to_le_bytes()[0];
                    let decoded = match v {
                        43 => letter - 64,
                        44 => match letter {
                            b'A'..=b'E' => letter - 38,
                            b'F'..=b'J' => letter - 11,
                            b'K'..=b'O' => letter + 16,
                            b'P'..=b'T' => letter + 43,
                            b'U' => 0,
                            b'V' => b'@',
                            b'W' => b'`',
                            _ => 127,
                        },
                        45 => {
                            if letter <= b'O' {
                                letter - 32
                            } else if letter == b'Z' {
                                b':'
                            } else {
                                return None;
                            }
                        }
                        46 => letter + 32,
                        _ => return None,
                    };
                    out.push(decoded as char);
                    i += 1;
                }
                i += 1;
            }
            return Some(Read {
                decoded: true,
                addon: None,
                codabar_start_stop: None,
                format: "Code93",
                text: out,
                start: start_index,
                end: at + 7,
                error: err / crate::numeric::usize_f32(values.len() + 3),
                gs1: false,
            });
        }
        values.push(v);
        at += 6;
    }
    *limited |= at + 7 <= runs.len() && values.len() >= 256;
    None
}

#[derive(Debug, Clone)]
pub struct Read {
    pub decoded: bool,
    pub addon: Option<String>,
    /// Codabar start and stop characters (A to D), which `text` omits.
    pub codabar_start_stop: Option<&'static str>,
    pub format: &'static str,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub error: f32,
    pub gs1: bool,
}

fn error_adjusted(r: &[f32], p: &[u8]) -> f32 {
    if r.len() < p.len() {
        return 10.;
    }
    let sum: f32 = r[..p.len()].iter().sum();
    let units: f32 = p.iter().map(|&x| f32::from(x)).sum();
    let mut module = sum / units;
    if module < MIN_MODULE {
        return 10.;
    }
    // Printing and thresholding can widen every bar while narrowing every
    // space. Even-length patterns preserve their total width under this bias.
    let bias = if p.len().is_multiple_of(2) {
        (r.iter()
            .zip(p)
            .enumerate()
            .map(|(i, (&v, &n))| (v - f32::from(n) * module) * if i % 2 == 0 { 1. } else { -1. })
            .sum::<f32>()
            / crate::numeric::usize_f32(p.len()))
        .clamp(-module * 0.5, module * 0.5)
    } else if p.len() > 1 && p.iter().all(|&x| x == 1) {
        let even = r.iter().take(p.len()).step_by(2).sum::<f32>()
            / crate::numeric::usize_f32(p.len().div_ceil(2));
        let odd = r.iter().skip(1).take(p.len() - 1).step_by(2).sum::<f32>()
            / crate::numeric::usize_f32(p.len() / 2);
        module = f32::midpoint(even, odd);
        ((even - odd) * 0.5).clamp(-module * 0.5, module * 0.5)
    } else {
        0.
    };
    let mut e = bias.abs() * 0.1;
    for (i, (&v, &n)) in r.iter().zip(p).enumerate() {
        let d = (v - f32::from(n) * module - bias * if i % 2 == 0 { 1. } else { -1. }).abs();
        if d > module * 1.0 {
            return 10.;
        }
        e += d;
    }
    e / sum
}
fn error(r: &[f32], p: &[u8]) -> f32 {
    if r.len() < p.len() {
        return 10.;
    }
    let sum: f32 = r[..p.len()].iter().sum();
    let module = sum / p.iter().map(|&x| f32::from(x)).sum::<f32>();
    if module < MIN_MODULE {
        return 10.;
    }
    let mut e = 0.;
    for (&v, &n) in r.iter().zip(p) {
        let d = (v - f32::from(n) * module).abs();
        if d > module {
            return 10.;
        }
        e += d;
    }
    e / sum
}
pub(crate) fn pattern_error(r: &[f32], p: &[u8]) -> f32 {
    error(r, p)
}
fn digit(r: &[f32], even: bool, gain: f32) -> Option<(u8, f32)> {
    let module = r[..4].iter().sum::<f32>() / 7.;
    let adjusted: [f32; 4] =
        std::array::from_fn(|i| r[i] - gain * module * if i % 2 == 0 { 1. } else { -1. });
    let sum: f32 = adjusted.iter().sum();
    let adjusted_module = sum / 7.;
    if adjusted_module < MIN_MODULE {
        return None;
    }
    let mut best = (0, 10.);
    let mut second = 10.;
    for (i, p) in DIGITS.iter().enumerate() {
        let mut e = 0.;
        for (j, &value) in adjusted.iter().enumerate() {
            let pattern = p[if even { 3 - j } else { j }];
            let delta = (value - f32::from(pattern) * adjusted_module).abs();
            if delta > adjusted_module {
                e = 10. * sum;
                break;
            }
            e += delta;
        }
        let e = e / sum;
        if e < best.1 {
            second = best.1;
            best = ((i).to_le_bytes()[0], e);
        } else {
            second = second.min(e);
        }
    }
    (best.1 < 0.23 && second - best.1 > 0.012).then_some(best)
}
#[must_use]
pub fn checksum(d: &[u8]) -> bool {
    !d.is_empty()
        && d.iter()
            .rev()
            .enumerate()
            .map(|(i, &v)| v as usize * if i % 2 == 0 { 1 } else { 3 })
            .sum::<usize>()
            % 10
            == 0
}
fn quiet(r: &[f32], start: usize, end: usize, module: f32, min: f32) -> bool {
    start > 0
        && r[start - 1] >= module * if start == 1 { min.min(0.5) } else { min }
        && (end == r.len()
            || r.get(end).is_some_and(|&x| {
                x >= module
                    * if end + 1 == r.len() {
                        min.min(0.5)
                    } else {
                        min
                    }
            }))
}
/// Whether both quiet zones of the symbol in runs `start..end` lie inside the runs,
/// not cut by the image or crop edge, and are at least `modules` modules wide.
pub(crate) fn full_quiet_zones(
    r: &[f32],
    start: usize,
    end: usize,
    module: f32,
    modules: f32,
) -> bool {
    start > 0
        && r[start - 1] >= module * modules
        && r.get(end).is_some_and(|&q| q >= module * modules)
}
fn ean_guards(r: &[f32], s: usize, end: usize, count: usize) -> bool {
    let center_ok = count == 6
        || error_adjusted(&r[s + 3 + (count / 2) * 4..], &[1, 1, 1, 1, 1]).partial_cmp(&0.25)
            != Some(std::cmp::Ordering::Greater);
    let end_start = end - if count == 6 { 6 } else { 3 };
    center_ok
        && error_adjusted(
            &r[end_start..],
            if count == 6 {
                &[1, 1, 1, 1, 1, 1]
            } else {
                &[1, 1, 1]
            },
        )
        .partial_cmp(&0.25)
            != Some(std::cmp::Ordering::Greater)
}
#[expect(
    clippy::too_many_lines,
    reason = "The symbol parser keeps start/stop, parity, checksum and work-limit transitions together for the same run cursor."
)]
fn ean(r: &[f32], s: usize, mask: u32, retain_failed: bool) -> Option<Read> {
    let module = r.get(s..s + 3)?.iter().sum::<f32>() / 3.;
    // Every retail layout requires this same leading quiet zone. Reject before
    // guard fitting and share the measured guard scale across candidate layouts.
    if !(s > 0 && r[s - 1] >= module * if s == 1 { 0.5 } else { 4. }) {
        return None;
    }
    if error_adjusted(&r[s..], &[1, 1, 1]) > 0.25 {
        return None;
    }
    let mut failed = None;
    for count in [12usize, 8, 6] {
        if (count == 12 && mask & (EAN13 | UPCA) == 0)
            || (count == 8 && mask & EAN8 == 0)
            || (count == 6 && mask & UPCE == 0)
        {
            continue;
        }
        let end = s + if count == 6 { 33 } else { count * 4 + 11 };
        if end > r.len() {
            continue;
        }
        if !quiet(r, s, end, module, 4.) {
            continue;
        }
        if !ean_guards(r, s, end, count) {
            continue;
        }
        let black = f32::midpoint(r[s], r[s + 2]);
        let white = r[s + 1];
        let measured_gain = ((black - white) / (black + white)).clamp(-0.45, 0.45);
        let gains: &[f32] = if count == 8 {
            &[measured_gain, 0., 0.25, -0.25, 0.5, -0.5]
        } else {
            &[measured_gain, 0.]
        };
        for &gain in gains {
            let mut digits = Vec::new();
            let mut parity = 0u8;
            let mut at = s + 3;
            let mut err = 0.;
            let mut ok = true;
            for i in 0..count {
                if count != 6 && i == count / 2 {
                    at += 5;
                }
                let color_gain = gain * if count == 6 || i < count / 2 { -1. } else { 1. };
                let odd = digit(&r[at..], false, color_gain);
                let even = if count != 8 && i < 6 {
                    digit(&r[at..], true, color_gain)
                } else {
                    None
                };
                let selected = match (odd, even) {
                    (Some(a), Some(b)) => {
                        if a.1 <= b.1 {
                            (a, false)
                        } else {
                            (b, true)
                        }
                    }
                    (Some(a), None) => (a, false),
                    (None, Some(b)) => (b, true),
                    _ => {
                        ok = false;
                        break;
                    }
                };
                if count != 8 && i < 6 {
                    parity = (parity << 1) | u8::from(selected.1);
                }
                digits.push(selected.0.0);
                err += selected.0.1;
                at += 4;
            }
            if !ok {
                continue;
            }
            let format;
            let valid;
            if count == 12 {
                let Some(first) = PARITY.iter().position(|&p| p == parity) else {
                    continue;
                };
                digits.insert(0, (first).to_le_bytes()[0]);
                valid = checksum(&digits);
                if first == 0 && mask & UPCA != 0 {
                    digits.remove(0);
                    format = "UPCA";
                } else if mask & EAN13 != 0 {
                    format = "EAN13";
                } else {
                    continue;
                }
            } else if count == 8 {
                valid = checksum(&digits);
                format = "EAN8";
            } else {
                let mut sys = 0;
                let check = if let Some(c) = UPC_PARITY.iter().position(|&p| p == parity) {
                    c
                } else if let Some(c) = UPC_PARITY.iter().position(|&p| p ^ 63 == parity) {
                    sys = 1;
                    c
                } else {
                    continue;
                };
                digits.insert(0, sys);
                digits.push((check).to_le_bytes()[0]);
                valid = checksum(&expand_upce(&digits));
                format = "UPCE";
            }
            if !valid {
                // The left half of EAN13 can mimic UPC-E without checksum
                // validation. A genuine trailing quiet zone disambiguates an
                // unread UPC-E candidate from the continuing EAN13 data bars.
                let unambiguous = count != 6 || r.get(end).is_some_and(|&v| v >= module * 7.);
                if retain_failed && failed.is_none() && unambiguous {
                    failed = Some(Read {
                        decoded: false,
                        addon: None,
                        codabar_start_stop: None,
                        format,
                        text: String::new(),
                        start: s,
                        end,
                        error: err / crate::numeric::usize_f32(count),
                        gs1: false,
                    });
                }
                continue;
            }
            let addon = if mask & (ADDON_READ | ADDON_REQUIRE) != 0 {
                ean_addon(r, end, module, gain)
            } else {
                None
            };
            if mask & ADDON_REQUIRE != 0 && addon.is_none() {
                if retain_failed && failed.is_none() {
                    failed = Some(Read {
                        decoded: false,
                        addon: None,
                        codabar_start_stop: None,
                        format,
                        text: String::new(),
                        start: s,
                        end,
                        error: err / crate::numeric::usize_f32(count),
                        gs1: false,
                    });
                }
                continue;
            }
            return Some(Read {
                decoded: true,
                addon,
                codabar_start_stop: None,
                format,
                text: digits.iter().map(|d| (b'0' + d) as char).collect(),
                start: s,
                end,
                error: err / crate::numeric::usize_f32(count),
                gs1: false,
            });
        }
    }
    failed
}
fn ean_addon(r: &[f32], base_end: usize, module: f32, gain: f32) -> Option<String> {
    let gap = *r.get(base_end)?;
    if !(module * 4.0..=module * 30.0).contains(&gap) {
        return None;
    }
    let s = base_end + 1;
    if s + 13 > r.len() || error(&r[s..], &[1, 1, 2]) > 0.23 {
        return None;
    }
    for count in [5, 2] {
        let end = s + 3 + count * 4 + (count - 1) * 2;
        if end > r.len()
            || (end < r.len() && r[end] < module * if end + 1 == r.len() { 0.5 } else { 4. })
        {
            continue;
        }
        let mut digits = Vec::new();
        let mut parity = 0u8;
        let mut at = s + 3;
        for i in 0..count {
            let odd = digit(&r[at..], false, -gain);
            let even = digit(&r[at..], true, -gain);
            let selected = match (odd, even) {
                (Some(a), Some(b)) => {
                    if a.1 <= b.1 {
                        (a, false)
                    } else {
                        (b, true)
                    }
                }
                (Some(a), None) => (a, false),
                (None, Some(b)) => (b, true),
                _ => break,
            };
            digits.push(selected.0.0);
            parity = parity * 2 + u8::from(selected.1);
            at += 4;
            if i + 1 < count {
                if error(&r[at..], &[1, 1]) > 0.23 {
                    break;
                }
                at += 2;
            }
        }
        if digits.len() != count {
            continue;
        }
        if supplement_parity(&digits) == Some(parity) {
            return Some(digits.iter().map(|&d| char::from(b'0' + d)).collect());
        }
    }
    None
}
#[must_use]
pub fn expand_upce(d: &[u8]) -> Vec<u8> {
    if d.len() != 8 {
        return vec![];
    }
    let mut out = vec![d[0], d[1], d[2]];
    match d[6] {
        0..=2 => out.extend([d[6], 0, 0, 0, 0, d[3], d[4], d[5]]),
        3 => out.extend([d[3], 0, 0, 0, 0, 0, d[4], d[5]]),
        4 => out.extend([d[3], d[4], 0, 0, 0, 0, 0, d[5]]),
        _ => out.extend([d[3], d[4], d[5], 0, 0, 0, 0, d[6]]),
    }
    out.push(d[7]);
    out
}
fn c128_match(r: &[f32], lo: usize, hi: usize) -> Option<(usize, f32)> {
    c128_ranked(r, lo, hi)
        .and_then(|(best, second)| (best.1 < 0.23 && second - best.1 > 0.012).then_some(best))
}
/// The best width match among patterns `lo..hi` and the second-best error.
fn c128_ranked(r: &[f32], lo: usize, hi: usize) -> Option<((usize, f32), f32)> {
    if r.len() < 6 {
        return None;
    }
    let sum = r[..6].iter().sum::<f32>();
    let module = sum / 11.;
    if module < MIN_MODULE {
        return None;
    }
    // Every six-element Code128 character spans eleven modules. Reuse its
    // scale and four possible element errors instead of recomputing them
    // independently for all 107 patterns.
    let distances: [[f32; 4]; 6] = if lo >= 103 {
        [[0.; 4]; 6]
    } else {
        std::array::from_fn(|j| {
            std::array::from_fn(|n| (r[j] - crate::numeric::usize_f32(n + 1) * module).abs())
        })
    };
    let mut best = (0, 10.);
    let mut second = 10.;
    for (i, p) in C128.iter().enumerate().take(hi).skip(lo) {
        let mut total = 0.;
        let mut valid = true;
        for j in 0..6 {
            let d = if lo >= 103 {
                (r[j] - f32::from(p[j] - b'0') * module).abs()
            } else {
                distances[j][(p[j] - b'1') as usize]
            };
            if d > module {
                valid = false;
                break;
            }
            total += d;
        }
        let e = if valid { total / sum } else { 10. };
        if e < best.1 {
            second = best.1;
            best = (i, e);
        } else {
            second = second.min(e);
        }
    }
    Some((best, second))
}
/// Match by the four sums of adjacent element pairs (edge-to-similar-edge
/// distances, the reference decode of Code 128). Uniform ink spread widens every
/// bar by as much as it narrows every space, so these sums keep their nominal
/// module counts; the 107 patterns have distinct sums.
fn c128_edge_match(r: &[f32], lo: usize, hi: usize) -> Option<(usize, f32)> {
    let ((index, e), second) =
        edge_ranked(r, 11, C128[lo..hi].iter().map(|p| p.map(|b| b - b'0')))?;
    let best = (lo + index, e);
    (best.1 < C128_EDGE_ERROR && second - best.1 > C128_EDGE_MARGIN).then_some(best)
}
const C128_EDGE_ERROR: f32 = 0.12;
const C128_EDGE_MARGIN: f32 = 0.03;
/// The stop pattern `2331112`. With edge-matched characters (`spread`), the bars
/// may be uniformly wider or narrower than the spaces: fit that bias on the first
/// six elements and require the final bar to agree with it.
fn c128_stop(r: &[f32], spread: bool) -> bool {
    const STOP: [u8; 7] = [2, 3, 3, 1, 1, 1, 2];
    if error(r, &STOP) <= 0.23 {
        return true;
    }
    if !spread || r.len() < 7 || error_adjusted(r, &STOP[..6]) > 0.23 {
        return false;
    }
    let module = r[..6].iter().sum::<f32>() / 11.;
    // Bars 2+3+1 and spaces 3+1+1 modules: their difference is one module.
    let bias = (r[0] + r[2] + r[4] - r[1] - r[3] - r[5] - module) / 6.;
    (r[6] - bias - 2. * module).abs() <= module * 0.8
}
fn code128(r: &[f32], s: usize, retain_failed: bool, limited: &mut bool) -> Option<Read> {
    let module = r[s..s + 6].iter().sum::<f32>() / 11.;
    if s == 0 || (s != 1 && r[s - 1] < module * 2.5) {
        return None;
    }
    let (best, second) = c128_ranked(&r[s..], 103, 106)?;
    let plain_start = (best.1 < 0.23 && second - best.1 > 0.012).then_some(best);
    let edge_start = plain_start.is_none();
    // Edge-match a start character only for a near miss behind a five-module quiet
    // zone: the fallback would otherwise run at almost every position.
    let (start, mut err) = plain_start.or_else(|| {
        (best.1 < 0.45 && (s == 1 || r[s - 1] >= module * 5.))
            .then(|| c128_edge_match(&r[s..], 103, 106))
            .flatten()
    })?;
    // A start character found only by its edges measures the ink spread: correct
    // every following character by it before the ordinary width match, so only
    // characters that still fail fall back to edge matching.
    let bias = if edge_start {
        let p = C128[start];
        let signed = |i: usize| if i.is_multiple_of(2) { 1. } else { -1. };
        (0..6)
            .map(|i| signed(i) * (r[s + i] - f32::from(p[i] - b'0') * module))
            .sum::<f32>()
            / 6.
    } else {
        0.
    };
    let corrected = |at: usize| -> [f32; 6] {
        std::array::from_fn(|i| r[at + i] - if i.is_multiple_of(2) { bias } else { -bias })
    };
    let mut weak = 0usize;
    let mut at = s + 6;
    let mut values = vec![start];
    while at + 7 <= r.len() && values.len() < 256 {
        let plain = if edge_start {
            c128_match(&corrected(at), 0, 107)
        } else {
            c128_match(&r[at..], 0, 107)
        };
        let (v, e) = plain.or_else(|| {
            weak += 1;
            // Edge-match only inside a symbol with two characters read after the
            // start and an unchanged pitch.
            (values.len() >= 3 && similar_width(&r[at..], r[at - 6..at].iter().sum()))
                .then(|| c128_edge_match(&r[at..], 0, 107))
                .flatten()
        })?;
        err += e;
        if v == 106 {
            if !c128_stop(&r[at..], edge_start || weak > 0)
                || !quiet(r, s, at + 7, module, 2.5)
                || values.len() < 3
                // Edge-matched characters rely on the check; keep them a minority.
                || weak * 4 > values.len()
            {
                return None;
            }
            let check = values.pop()?;
            let valid = (values[0]
                + values
                    .iter()
                    .enumerate()
                    .skip(1)
                    .map(|(i, &n)| i * n)
                    .sum::<usize>())
                % 103
                == check;
            if !valid && (!retain_failed || edge_start || weak > 0) {
                return None;
            }
            let (text, gs1) = if valid {
                c128_text(&values)?
            } else {
                (String::new(), false)
            };
            return Some(Read {
                decoded: valid,
                addon: None,
                codabar_start_stop: None,
                format: "Code128",
                text,
                start: s,
                end: at + 7,
                error: err / crate::numeric::usize_f32(values.len()),
                gs1,
            });
        }
        if v >= 103 {
            return None;
        }
        values.push(v);
        at += 6;
    }
    *limited |= at + 7 <= r.len() && values.len() >= 256;
    None
}
fn c128_text(v: &[usize]) -> Option<(String, bool)> {
    let mut set = v[0] - 103;
    let mut shift = false;
    let mut upper = false;
    let mut upper_once = false;
    let mut out = Vec::new();
    let mut gs1 = false;
    for (i, &c) in v.iter().enumerate().skip(1) {
        if set == 2 {
            match c {
                0..=99 => {
                    out.extend([
                        b'0' + (c / 10).to_le_bytes()[0],
                        b'0' + (c % 10).to_le_bytes()[0],
                    ]);
                }
                100 => set = 1,
                101 => set = 0,
                102 => {
                    if i == 1 {
                        gs1 = true;
                    } else {
                        out.push(29);
                    }
                }
                _ => return None,
            }
            continue;
        }
        let effective = if shift { 1 - set } else { set };
        if c < 96 {
            let x = if effective == 0 && c >= 64 {
                c - 64
            } else {
                c + 32
            };
            out.push((x).to_le_bytes()[0] + if upper ^ upper_once { 128 } else { 0 });
            shift = false;
            upper_once = false;
            continue;
        }
        match c {
            98 => shift = true,
            99 => set = 2,
            102 => {
                if i == 1 {
                    gs1 = true;
                } else {
                    out.push(29);
                }
            }
            100 | 101 => {
                if (set == 1 && c == 100) || (set == 0 && c == 101) {
                    if upper_once {
                        upper = !upper;
                        upper_once = false;
                    } else {
                        upper_once = true;
                    }
                } else {
                    set = usize::from(c != 101);
                }
            }
            _ => return None,
        }
    }
    if shift || upper_once || out.is_empty() {
        return None;
    }
    Some((out.iter().map(|&b| b as char).collect(), gs1))
}
pub(crate) fn wide_bits(r: &[f32], n: usize, wides: usize) -> Option<(u16, f32)> {
    if r.len() < n {
        return None;
    }
    let mut storage = [0f32; 9];
    let sorted = &mut storage[..n];
    sorted.copy_from_slice(&r[..n]);
    sorted.sort_by(f32::total_cmp);
    wide_bits_sorted(&r[..n], sorted, n, wides)
}
fn wide_bits_sorted(r: &[f32], sorted: &[f32], n: usize, wides: usize) -> Option<(u16, f32)> {
    wide_bits_sorted_ratio(r, sorted, n, wides, 1.45)
}
fn wide_bits_sorted_ratio(
    r: &[f32],
    sorted: &[f32],
    n: usize,
    wides: usize,
    min_ratio: f32,
) -> Option<(u16, f32)> {
    let narrow = sorted[..n - wides].iter().sum::<f32>() / crate::numeric::usize_f32(n - wides);
    let wide = sorted[n - wides..].iter().sum::<f32>() / crate::numeric::usize_f32(wides);
    if narrow < MIN_MODULE || wide / narrow < min_ratio || wide / narrow > 3.8 {
        return None;
    }
    let cut = f32::midpoint(sorted[n - wides - 1], sorted[n - wides]);
    let mut bits = 0;
    let mut e = 0.;
    for &x in &r[..n] {
        let w = x > cut;
        bits = (bits << 1) | u16::from(w);
        let expected = if w { wide } else { narrow };
        if (x - expected).abs() > narrow * 0.9 {
            return None;
        }
        e += (x - expected).abs();
    }
    Some((bits, e / r[..n].iter().sum::<f32>()))
}
/// Smallest measured wide/narrow ratio of a Code 39 character at about one pixel per
/// narrow module: blur and thresholding shrink wide elements towards narrow ones
/// (printed ratio 2 to 3). Tried only when the ordinary 1.45 bound fails; a symbol
/// with such a character must then fit [`code39_dense_model`].
const C39_DENSE_RATIO: f32 = 1.25;
/// A Code 39 character's pattern and error, and whether it needed the dense ratio.
fn code39_bits(r: &[f32; 9]) -> Option<(u16, f32, bool)> {
    if let Some((bits, e)) = wide_bits(r, 9, 3) {
        return Some((bits, e, false));
    }
    let mut sorted = *r;
    sorted.sort_by(f32::total_cmp);
    // Code 39 has no check character: with compressed widths, accept only a
    // clear split between the three wide and six narrow elements.
    let narrow = sorted[..6].iter().sum::<f32>() / 6.;
    let wide = sorted[6..].iter().sum::<f32>() / 3.;
    (sorted[6] - sorted[5] >= (wide - narrow) * C39_DENSE_SPLIT)
        .then(|| wide_bits_sorted_ratio(r, &sorted, 9, 3, C39_DENSE_RATIO))
        .flatten()
        .map(|(bits, e)| (bits, e, true))
}
/// Whether the characters of a Code 39 symbol (`patterns`, from the start character
/// at `start`, with the gain-corrected widths of `adjusted`) fit one narrow/wide
/// model, as a symbol read with the dense ratio must. Widths are taken relative to
/// their character's width, which perspective changes along the symbol. Per color,
/// the wide and narrow means must lie at least [`C39_DENSE_RATIO`] apart, and the
/// midpoint between them must classify every element as its character does, except
/// for at most one element per two characters (noise at about a pixel per module).
/// Each character fit alone could otherwise take its own split, and a blurred
/// character read as another one.
fn code39_dense_model(
    adjusted: impl Fn(usize) -> [f32; 9],
    start: usize,
    patterns: &[u16],
) -> bool {
    let elements = || {
        patterns.iter().enumerate().flat_map(|(k, &bits)| {
            let widths = adjusted(start + 10 * k);
            let total: f32 = widths.iter().sum();
            (0..9).map(move |i| {
                (
                    widths[i] / total,
                    (i % 2) * 2 + usize::from(bits & (256 >> i) != 0),
                )
            })
        })
    };
    let mut sums = [0f32; 4];
    let mut counts = [0f32; 4];
    for (width, class) in elements() {
        sums[class] += width;
        counts[class] += 1.;
    }
    let means: [f32; 4] = std::array::from_fn(|i| sums[i] / counts[i].max(1.));
    let cuts = [
        f32::midpoint(means[0], means[1]),
        f32::midpoint(means[2], means[3]),
    ];
    counts.iter().all(|&c| c > 0.)
        && [0, 2]
            .iter()
            .all(|&i| means[i + 1] >= means[i] * C39_DENSE_RATIO)
        && elements()
            .filter(|&(width, class)| (width > cuts[class / 2]) != (class % 2 == 1))
            .count()
            * 2
            <= patterns.len()
}
/// Required gap between the narrowest wide and the widest narrow element, as a
/// fraction of the wide-narrow difference, for the dense-ratio fallback.
const C39_DENSE_SPLIT: f32 = 0.2;
/// Unchecked Code 39 and Codabar symbols with at most this many data characters
/// need [`SHORT_QUIET_MODULES`] of quiet zone on both sides.
const SHORT_UNCHECKED_CHARACTERS: usize = 2;
/// The quiet zone both specifications require (ten narrow modules).
const SHORT_QUIET_MODULES: f32 = 10.;
fn code39(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    if r.len() < s + 9 {
        return None;
    }
    // Ink spread widens bars and narrows spaces. Screen the start pattern
    // within each color before estimating that shared additive displacement.
    let start = &r[s..s + 9];
    let black_max = start[0].max(start[2]).max(start[8]);
    let white_max = start[3].max(start[5]).max(start[7]);
    let black_min = start[4].min(start[6]);
    let white_min = start[1];
    if black_max >= black_min || white_max >= white_min {
        return None;
    }
    if black_max.max(white_max) < black_min.min(white_min)
        && let Some(read) = code39_with_gain(r, s, 0., limited)
    {
        return Some(read);
    }
    let black = (start[0] + start[2] + start[8]) / 3.;
    let white = (start[3] + start[5] + start[7]) / 3.;
    let gain = (black - white) * 0.5;
    let narrow = f32::midpoint(black, white);
    if gain.abs() < narrow * 0.1 || gain.abs() > narrow * 0.75 {
        return None;
    }
    code39_with_gain(r, s, gain, limited)
}
fn code39_with_gain(
    runs: &[f32],
    start_index: usize,
    gain: f32,
    limited: &mut bool,
) -> Option<Read> {
    let adjusted = |at: usize| -> [f32; 9] {
        std::array::from_fn(|i| runs[at + i] - if i % 2 == 0 { gain } else { -gain })
    };
    let start = adjusted(start_index);
    let module = start.iter().copied().fold(f32::INFINITY, f32::min);
    if start_index > 1 && runs[start_index - 1] < module * 5. {
        return None;
    }
    let (p, mut err, mut dense) = code39_bits(&start)?;
    if p != C39[43] {
        return None;
    }
    let mut patterns = vec![p];
    let mut at = start_index + 10;
    let mut out = Vec::new();
    while at + 9 <= runs.len() && out.len() < 128 {
        let (p, e, compressed) = code39_bits(&adjusted(at))?;
        let d = C39.iter().position(|&x| x == p)?;
        err += e;
        dense |= compressed;
        patterns.push(p);
        if d == 43 {
            // An image boundary can crop a quiet zone, but never a guard bar.
            // Interior symbols still require quiet space on both sides.
            let end = at + 9;
            let left = start_index == 0
                || runs[start_index - 1] >= module * if start_index == 1 { 0.5 } else { 5. };
            let right = end == runs.len()
                || runs
                    .get(end)
                    .is_some_and(|&v| v >= module * if end + 1 == runs.len() { 0.5 } else { 5. });
            if out.is_empty() || !left || !right {
                return None;
            }
            // One or two characters between guards is also what a misread inside a
            // longer symbol or a crop edge produces: require the full ten-module
            // quiet zones of the specification on both sides, measured in the image.
            if out.len() <= SHORT_UNCHECKED_CHARACTERS
                && !full_quiet_zones(runs, start_index, end, module, SHORT_QUIET_MODULES)
            {
                return None;
            }
            if dense && !code39_dense_model(adjusted, start_index, &patterns) {
                return None;
            }
            return Some(Read {
                decoded: true,
                addon: None,
                codabar_start_stop: None,
                format: "Code39",
                text: String::from_utf8(out).ok()?,
                start: start_index,
                end: at + 9,
                error: err / (1. + crate::numeric::usize_f32((at - start_index) / 10)),
                gs1: false,
            });
        }
        out.push(C39_ALPHABET[d]);
        if at + 9 >= runs.len() || runs[at + 9] > module * 3. {
            return None;
        }
        at += 10;
    }
    *limited |= at + 9 <= runs.len() && out.len() >= 128;
    None
}
fn itf(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    // Ink spread widens every bar and narrows every space (or the reverse). Digits are
    // unaffected (each holds only bars or only spaces), so only the guards need the
    // bias, measured on the start pattern when it fails the plain check.
    let corrected = if error(&r[s..], &[1, 1, 1, 1]) <= 0.18 {
        false
    } else if error_adjusted(&r[s..], &[1, 1, 1, 1]) <= 0.18 {
        true
    } else {
        return None;
    };
    let bias = if corrected {
        (r[s] + r[s + 2] - r[s + 1] - r[s + 3]) / 4.
    } else {
        0.
    };
    let module = r[s..s + 4].iter().sum::<f32>() / 4.;
    if s == 0 || (s != 1 && r[s - 1] < module * 7.) {
        return None;
    }
    let stop = |at: usize| {
        let guard = [r[at] - bias, r[at + 1] + bias, r[at + 2] - bias];
        error(&guard, &[3, 1, 1]) < 0.18 || error(&guard, &[2, 1, 1]) < 0.18
    };
    let mut at = s + 4;
    let mut out = Vec::new();
    let mut err = 0.;
    while at + 3 <= r.len() && out.len() < 80 {
        // A bias-corrected start is weaker evidence: require ITF-6 or longer.
        let shortest = if corrected { 6 } else { 4 };
        if out.len() >= shortest && stop(at) && quiet(r, s, at + 3, module, 7.) {
            let error = err / crate::numeric::usize_f32(out.len());
            // A quiet zone cut by the edge of the runs is weaker evidence: then no
            // element may leave the model.
            let cut = !full_quiet_zones(r, s, at + 3, module, 7.);
            if !itf_consistent(r, s, &out, cut) || code39_fits_better(r, s, at + 3, error) {
                return None;
            }
            return Some(Read {
                decoded: true,
                addon: None,
                codabar_start_stop: None,
                format: "ITF",
                text: out.iter().map(|d| (b'0' + d) as char).collect(),
                start: s,
                end: at + 3,
                error,
                gs1: false,
            });
        }
        if at + 10 > r.len() {
            return None;
        }
        for parity in 0..2 {
            let seq: [f32; 5] = std::array::from_fn(|i| r[at + 2 * i + parity]);
            let (p, e) = wide_bits(&seq, 5, 2)?;
            out.push((ITF_DIGITS.iter().position(|&x| u16::from(x) == p)?).to_le_bytes()[0]);
            err += e;
        }
        at += 10;
    }
    *limited |= at + 3 <= r.len() && out.len() >= 80;
    None
}
/// ITF has no per-character structure: any ten runs with two wide bars and two wide
/// spaces form a digit pair, so fragments of other symbols can parse as ITF. A real
/// symbol prints every element from one narrow/wide model: fit narrow and wide bar and
/// space widths to the data characters. A local print defect (a stain, a stamp over
/// the bars) may move a few elements, at most one per three digit pairs, by more than
/// a narrow module from their class; the others must lie on average within a quarter
/// module. A misparsed Code 39 symbol puts narrow elements into wide classes across
/// the whole symbol, which moves the class means and most of their members.
fn itf_consistent(r: &[f32], s: usize, digits: &[u8], cut: bool) -> bool {
    // Per class: bar narrow, bar wide, space narrow, space wide.
    let mut sums = [0f32; 4];
    let mut counts = [0u8; 4];
    let classify = |pair: usize, i: usize| -> [(f32, usize); 2] {
        let at = s + 4 + pair * 10 + 2 * i;
        let bars = ITF_DIGITS[usize::from(digits[2 * pair])];
        let spaces = ITF_DIGITS[usize::from(digits[2 * pair + 1])];
        [
            (r[at], usize::from(bars & (16 >> i) != 0)),
            (r[at + 1], 2 + usize::from(spaces & (16 >> i) != 0)),
        ]
    };
    let pairs = digits.len() / 2;
    for pair in 0..pairs {
        for i in 0..5 {
            for (width, class) in classify(pair, i) {
                sums[class] += width;
                counts[class] = counts[class].saturating_add(1);
            }
        }
    }
    let mean: [f32; 4] = std::array::from_fn(|k| sums[k] / f32::from(counts[k].max(1)));
    let narrow = f32::midpoint(mean[0], mean[2]);
    if narrow <= 0. {
        return false;
    }
    let mut total = 0f32;
    let mut outliers = 0;
    for pair in 0..pairs {
        for i in 0..5 {
            for (width, class) in classify(pair, i) {
                let deviation = (width - mean[class]).abs();
                if deviation > narrow {
                    outliers += 1;
                } else {
                    total += deviation;
                }
            }
        }
    }
    outliers <= if cut { 0 } else { 1.max(pairs / 3) }
        && total <= narrow * 0.25 * crate::numeric::usize_f32(pairs * 10 - outliers)
}
/// On overlap prefer the format with more structure. Code 39 characters are nine
/// elements, three of them wide, from a fixed table, separated by a narrow gap; they
/// occupy ten runs, the length of an ITF digit pair. Runs of a Code 39 symbol whose
/// quiet zone was cut off (by a crop or the image edge) can therefore parse as ITF
/// pairs. Reject the ITF read when consecutive Code 39 characters span all but at
/// most one of its pairs and fit their widths better than the ITF pairs do.
fn code39_fits_better(r: &[f32], s: usize, end: usize, itf_error: f32) -> bool {
    let pairs = (end - s - 7) / 10;
    let character = |p: usize| -> Option<f32> {
        let (bits, e) = wide_bits(r.get(p..p + 10)?, 9, 3)?;
        if !C39.contains(&bits) {
            return None;
        }
        let wide = (0..9)
            .filter(|i| bits & (256 >> i) != 0)
            .map(|i| r[p + i])
            .fold(f32::INFINITY, f32::min);
        (r[p + 9] < wide * 0.75).then_some(e)
    };
    (0..5).any(|phase| {
        let (mut run, mut sum) = (0usize, 0f32);
        let mut p = s + 2 * phase;
        while p + 9 <= end {
            if let Some(e) = character(p) {
                run += 1;
                sum += e;
                if run >= 3.max(pairs.saturating_sub(1))
                    && sum / crate::numeric::usize_f32(run) < itf_error
                {
                    return true;
                }
            } else {
                (run, sum) = (0, 0.);
            }
            p += 10;
        }
        false
    })
}
fn codabar_one(r: &[f32]) -> Option<(usize, f32)> {
    if r.len() < 7 {
        return None;
    }
    let mut sorted = [0f32; 7];
    sorted.copy_from_slice(&r[..7]);
    sorted.sort_by(f32::total_cmp);
    let mut best = None;
    for wides in [2, 3] {
        if let Some((bits, e)) = wide_bits_sorted(r, &sorted, 7, wides)
            && let Some(i) = CODA.iter().position(|&p| u16::from(p) == bits)
            && best.is_none_or(|(_, old)| e < old)
        {
            best = Some((i, e));
        }
    }
    best
}
fn codabar(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    let one = codabar_one;
    let (first, mut err) = one(&r[s..])?;
    if first < 16 {
        return None;
    }
    let module = r[s..s + 7].iter().copied().fold(f32::INFINITY, f32::min);
    if s == 0 || (s != 1 && r[s - 1] < module * 5.) {
        return None;
    }
    let mut at = s + 8;
    let mut out = Vec::new();
    let mut characters = vec![first];
    while at + 7 <= r.len() && out.len() < 128 {
        let (c, e) = one(&r[at..])?;
        err += e;
        characters.push(c);
        if c >= 16 {
            if out.is_empty() {
                return None;
            }
            // A checkless reader must fit one physical narrow/wide model
            // across the complete symbol. Independent per-character fits can
            // otherwise reinterpret a short fragment of another symbology.
            let elements = || {
                characters
                    .iter()
                    .enumerate()
                    .flat_map(move |(index, &character)| {
                        (0..7).map(move |element| {
                            let wide = usize::from(CODA[character] & (1 << (6 - element)) != 0);
                            (r[s + index * 8 + element], (element % 2) * 2 + wide)
                        })
                    })
            };
            let (sums, means, deviation) = two_width_model(elements, &(1.4..=4.1), 0.75)?;
            let global_error = deviation / sums.iter().sum::<f32>();
            let module = f32::midpoint(means[0], means[2]);
            if global_error > 0.19 || !quiet(r, s, at + 7, module, 5.) {
                return None;
            }
            // As for Code 39: a one- or two-character symbol needs full quiet zones.
            if out.len() <= SHORT_UNCHECKED_CHARACTERS
                && !full_quiet_zones(r, s, at + 7, module, SHORT_QUIET_MODULES)
            {
                return None;
            }
            return Some(Read {
                decoded: true,
                addon: None,
                codabar_start_stop: Some(CODA_START_STOP[(first - 16) * 4 + c - 16]),
                format: "Codabar",
                text: String::from_utf8(out).ok()?,
                start: s,
                end: at + 7,
                error: err / (1. + crate::numeric::usize_f32((at - s) / 8)),
                gs1: false,
            });
        }
        out.push(CODA_ALPHABET[c]);
        at += 8;
    }
    *limited |= at + 7 <= r.len() && out.len() >= 128;
    None
}

/// One narrow/wide model for a whole symbol of `elements` (width and class: narrow
/// bar 0, wide bar 1, narrow space 2, wide space 3), in symbol order. Returns the
/// class sums, the class means and the total distance of the elements from their
/// class means; `None` when a class is empty, a wide/narrow mean ratio lies outside
/// `ratio`, or an element lies further than `deviation` times its color's narrow
/// mean (at least [`MIN_MODULE`]) from its class mean.
fn two_width_model<I: Iterator<Item = (f32, usize)>>(
    elements: impl Fn() -> I,
    ratio: &std::ops::RangeInclusive<f32>,
    deviation: f32,
) -> Option<([f32; 4], [f32; 4], f32)> {
    let mut sums = [0f32; 4];
    let mut counts = [0usize; 4];
    for (width, class) in elements() {
        sums[class] += width;
        counts[class] += 1;
    }
    if counts.contains(&0) {
        return None;
    }
    let means: [f32; 4] = std::array::from_fn(|i| sums[i] / crate::numeric::usize_f32(counts[i]));
    if [0, 2]
        .iter()
        .any(|&i| !ratio.contains(&(means[i + 1] / means[i])))
    {
        return None;
    }
    let mut total = 0.;
    for (width, class) in elements() {
        let distance = (width - means[class]).abs();
        if distance > (means[class & !1] * deviation).max(MIN_MODULE) {
            return None;
        }
        total += distance;
    }
    Some((sums, means, total))
}

/// Mod-10 (Luhn) check digit over MSI data digits, rightmost digit doubled.
fn msi_check(data: &[u8]) -> u8 {
    let sum: u32 = data
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let v = u32::from(d) * if i % 2 == 0 { 2 } else { 1 };
            v / 10 + v % 10
        })
        .sum();
    ((10 - sum % 10) % 10).to_le_bytes()[0]
}
/// Quiet zone required on both sides of an MSI Plessey symbol, in modules.
const MSI_QUIET_MODULES: f32 = 8.;
/// Accepted wide/narrow ratio of the bar and space class means: nominally 2, with
/// allowance for print gain and blur.
const MSI_WIDE_NARROW_RATIO: std::ops::RangeInclusive<f32> = 1.4..=3.5;
/// Largest distance of one element from its class mean, in narrow widths.
const MSI_ELEMENT_DEVIATION: f32 = 0.7;
/// Largest mean deviation per unit of symbol width: Codabar allows 0.19, a code
/// without per-character redundancy less.
const MSI_MEAN_DEVIATION: f32 = 0.17;
/// MSI Plessey: each bit is one bar/space pair of three modules, a wide bar
/// (2:1) for 1 and a narrow bar (1:2) for 0. A 1 starts the symbol; four bits
/// per digit follow, most significant first; a 0 pair and a narrow bar stop it.
/// The symbology has no per-character redundancy, so the reader requires a
/// mod-10 check digit, wide quiet zones and one narrow/wide model for all pairs.
fn msi(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    let pair = |at: usize| -> Option<bool> {
        let (bar, space) = (*r.get(at)?, *r.get(at + 1)?);
        let fraction = bar / (bar + space);
        if (0.2..=0.43).contains(&fraction) {
            Some(false)
        } else if (0.57..=0.8).contains(&fraction) {
            Some(true)
        } else {
            None
        }
    };
    if pair(s) != Some(true) {
        return None;
    }
    let module = (r[s] + r[s + 1]) / 3.;
    // Unlike checked symbologies, a symbol cut by the image or crop edge is not
    // accepted: any '1' pair inside a symbol looks like a start, and the check
    // digit alone would pass one in ten such fragments.
    // The caller only tries starts after a quiet-zone run (`s > 0`).
    let quiet = *r.get(s.checked_sub(1)?)?;
    if quiet < module * MSI_QUIET_MODULES {
        return None;
    }
    let mut digits = Vec::new();
    let mut at = s + 2;
    let mut width = r[s] + r[s + 1];
    while at + 3 <= r.len() && digits.len() < 64 {
        let stop_quiet = r
            .get(at + 3)
            .is_some_and(|&quiet| quiet >= module * MSI_QUIET_MODULES);
        if digits.len() >= 4 && stop_quiet && pair(at) == Some(false) {
            let bar = r[at + 2];
            let narrow = r[at];
            if (bar - narrow).abs() > narrow.max(module) * 0.6 {
                return None;
            }
            return msi_finish(r, s, at + 3, &digits);
        }
        let mut value = 0u8;
        for bit in 0..4 {
            let i = at + bit * 2;
            let one = pair(i)?;
            let w = r[i] + *r.get(i + 1)?;
            // Adjacent pairs share a printing scale; a jump marks another symbol.
            if (w - width).abs() > width * 0.35 {
                return None;
            }
            width = w;
            value = value * 2 + u8::from(one);
        }
        if value > 9 {
            return None;
        }
        digits.push(value);
        at += 8;
    }
    *limited |= at + 3 <= r.len() && digits.len() >= 64;
    None
}
fn msi_finish(r: &[f32], s: usize, end: usize, digits: &[u8]) -> Option<Read> {
    let (data, check) = digits.split_at(digits.len() - 1);
    if msi_check(data) != check[0] {
        return None;
    }
    // One physical model: narrow and wide bars and spaces each cluster, as in
    // Codabar, so pairs from unrelated bars cannot combine into a symbol.
    let mut bits = vec![true];
    for &d in digits {
        bits.extend((0..4).rev().map(|b| d & (1 << b) != 0));
    }
    bits.push(false);
    let elements = || {
        bits.iter().enumerate().flat_map(|(i, &one)| {
            [
                (r[s + 2 * i], usize::from(one)),
                (r[s + 2 * i + 1], 2 + usize::from(!one)),
            ]
        })
    };
    let (_, _, deviation) =
        two_width_model(elements, &MSI_WIDE_NARROW_RATIO, MSI_ELEMENT_DEVIATION)?;
    let total: f32 = r[s..end - 1].iter().sum();
    if deviation / total > MSI_MEAN_DEVIATION {
        return None;
    }
    Some(Read {
        decoded: true,
        addon: None,
        codabar_start_stop: None,
        format: "MSIPlessey",
        text: digits.iter().map(|&d| char::from(b'0' + d)).collect(),
        start: s,
        end,
        error: deviation / total,
        gs1: false,
    })
}

#[must_use]
pub fn decode(r: &[f32], first_black: bool, mask: u32) -> Vec<Read> {
    decode_impl(r, first_black, mask, false, &mut false)
}
/// `decoded: false` denotes a structurally parsed symbol with a failed checksum.
/// The image scanner requires repeated spatial evidence before retaining it.
#[must_use]
pub(crate) fn decode_candidates(
    r: &[f32],
    first_black: bool,
    mask: u32,
    limited: &mut bool,
) -> Vec<Read> {
    decode_impl(r, first_black, mask, true, limited)
}
fn decode_impl(
    r: &[f32],
    first_black: bool,
    mask: u32,
    retain_failed: bool,
    limited: &mut bool,
) -> Vec<Read> {
    let mut out = Vec::new();
    let mut s = usize::from(!first_black);
    let minimum_runs = if mask & !(15 | ADDON_READ | ADDON_REQUIRE) == 0 {
        if mask & UPCE != 0 {
            33
        } else if mask & EAN8 != 0 {
            43
        } else {
            59
        }
    } else if mask & PHARMACODE != 0 {
        // The shortest Pharmacode: its bars and the spaces between them.
        crate::pharmacode::MIN_BARS * 2 - 1
    } else {
        16
    };
    while s + minimum_runs <= r.len() {
        // GS1 DataBar has no required quiet zone; its finder and checksum
        // validate starts even when the first white guard touches the image edge.
        let quiet = s > 0 && (s == 1 || r[s - 1] >= r[s] * 2.);
        let mut found = None;
        let mut failed = None;
        let mut accepted = |read: Option<Read>| {
            if read.as_ref().is_some_and(|r| !r.decoded) {
                if failed.is_none() {
                    failed = read;
                }
                None
            } else {
                read
            }
        };
        if quiet && mask & crate::formats::RETAIL_MASK != 0 {
            found = accepted(ean(r, s, mask, retain_failed));
        }
        if found.is_none()
            && mask & CODE128 != 0
            && s > 0
            && (quiet || r[s - 1] >= (r[s..s + 6].iter().sum::<f32>() / 11.) * 2.5)
        {
            found = accepted(code128(r, s, retain_failed, limited));
        }
        if (quiet || s == 0) && found.is_none() && mask & CODE39 != 0 {
            found = code39(r, s, limited);
        }
        if quiet && found.is_none() && mask & CODE93 != 0 {
            found = accepted(code93(r, s, retain_failed, limited));
        }
        if found.is_none() && mask & DATABAR_EXPANDED != 0 {
            found = crate::expanded::decode(r, s);
        }
        if found.is_none() && mask & DATABAR != 0 {
            found = crate::databar::decode(r, s);
        }
        if found.is_none() && mask & DATABAR_LIMITED != 0 {
            found = crate::databar_limited::decode(r, s);
        }
        if quiet && found.is_none() && mask & ITF != 0 {
            found = itf(r, s, limited);
        }
        if quiet && found.is_none() && mask & CODABAR != 0 {
            found = codabar(r, s, limited);
        }
        if quiet && found.is_none() && mask & MSI_PLESSEY != 0 {
            found = msi(r, s, limited);
        }
        if quiet && found.is_none() && mask & INDUSTRIAL25 != 0 {
            found = crate::industrial25::decode(r, s, limited);
        }
        if quiet && found.is_none() && mask & IATA25 != 0 {
            found = crate::two_of_five::iata(r, s, limited);
        }
        if quiet && found.is_none() && mask & MATRIX25 != 0 {
            found = crate::two_of_five::matrix(r, s, limited);
        }
        if quiet && found.is_none() && mask & PHARMACODE != 0 {
            found = crate::pharmacode::decode(r, s);
        }
        if let Some(read) = found {
            s = read.end;
            if (s % 2 == 0) != first_black {
                s += 1;
            }
            out.push(read);
        } else {
            if let Some(read) = failed {
                out.push(read);
            }
            s += 2;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digit_reference(r: &[f32], even: bool, gain: f32) -> Option<(u8, f32)> {
        let module = r[..4].iter().sum::<f32>() / 7.;
        let adjusted: [f32; 4] =
            std::array::from_fn(|i| r[i] - gain * module * if i % 2 == 0 { 1. } else { -1. });
        let mut best = (0, 10.);
        let mut second = 10.;
        for (i, p) in DIGITS.iter().enumerate() {
            let mut q = *p;
            if even {
                q.reverse();
            }
            let e = error(&adjusted, &q);
            if e < best.1 {
                second = best.1;
                best = ((i).to_le_bytes()[0], e);
            } else {
                second = second.min(e);
            }
        }
        (best.1 < 0.23 && second - best.1 > 0.012).then_some(best)
    }
    #[test]
    fn retail_digit_kernel_matches_original() {
        let mut state = 0x9160_0011_u32;
        for trial in 0..100_000 {
            let widths: [f32; 4] = std::array::from_fn(|i| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if trial % 2 == 0 {
                    f32::from(DIGITS[(trial / 2) % 10][i]) * 2.
                        + f32::from(state.to_be_bytes()[0]) / 255.
                        - 0.5
                } else {
                    f32::from(state.to_be_bytes()[0]) / 17.
                }
            });
            for gain in [-0.5, -0.25, 0., 0.25, 0.5] {
                for even in [false, true] {
                    assert_eq!(
                        digit(&widths, even, gain),
                        digit_reference(&widths, even, gain)
                    );
                }
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        reason = "Test-only immutable reference clone preserves the prechange EAN implementation for exact parity checks."
    )]
    fn ean_reference(r: &[f32], s: usize, mask: u32, retain_failed: bool) -> Option<Read> {
        if error_adjusted(&r[s..], &[1, 1, 1]) > 0.25 {
            return None;
        }
        let mut failed = None;
        for count in [12usize, 8, 6] {
            if (count == 12 && mask & (EAN13 | UPCA) == 0)
                || (count == 8 && mask & EAN8 == 0)
                || (count == 6 && mask & UPCE == 0)
            {
                continue;
            }
            let end = s + if count == 6 { 33 } else { count * 4 + 11 };
            if end > r.len() {
                continue;
            }
            let module = r[s..s + 3].iter().sum::<f32>() / 3.;
            if !quiet(r, s, end, module, 4.) {
                continue;
            }
            let black = f32::midpoint(r[s], r[s + 2]);
            let white = r[s + 1];
            let measured_gain = ((black - white) / (black + white)).clamp(-0.45, 0.45);
            let gains: &[f32] = if count == 8 {
                &[measured_gain, 0., 0.25, -0.25, 0.5, -0.5]
            } else {
                &[measured_gain, 0.]
            };
            for &gain in gains {
                let mut digits = Vec::new();
                let mut parity = 0u8;
                let mut at = s + 3;
                let mut err = 0.;
                let mut ok = true;
                for i in 0..count {
                    if count != 6 && i == count / 2 {
                        if error_adjusted(&r[at..], &[1, 1, 1, 1, 1]) > 0.25 {
                            ok = false;
                            break;
                        }
                        at += 5;
                    }
                    let color_gain = gain * if count == 6 || i < count / 2 { -1. } else { 1. };
                    let odd = digit(&r[at..], false, color_gain);
                    let even = if count != 8 && i < 6 {
                        digit(&r[at..], true, color_gain)
                    } else {
                        None
                    };
                    let selected = match (odd, even) {
                        (Some(a), Some(b)) => {
                            if a.1 <= b.1 {
                                (a, false)
                            } else {
                                (b, true)
                            }
                        }
                        (Some(a), None) => (a, false),
                        (None, Some(b)) => (b, true),
                        _ => {
                            ok = false;
                            break;
                        }
                    };
                    if count != 8 && i < 6 {
                        parity = (parity << 1) | u8::from(selected.1);
                    }
                    digits.push(selected.0.0);
                    err += selected.0.1;
                    at += 4;
                }
                if !ok {
                    continue;
                }
                let guard: &[u8] = if count == 6 {
                    &[1, 1, 1, 1, 1, 1]
                } else {
                    &[1, 1, 1]
                };
                if error_adjusted(&r[at..], guard) > 0.25 {
                    continue;
                }
                let format;
                let valid;
                if count == 12 {
                    let Some(first) = PARITY.iter().position(|&p| p == parity) else {
                        continue;
                    };
                    digits.insert(0, first as u8);
                    valid = checksum(&digits);
                    if first == 0 && mask & UPCA != 0 {
                        digits.remove(0);
                        format = "UPCA";
                    } else if mask & EAN13 != 0 {
                        format = "EAN13";
                    } else {
                        continue;
                    }
                } else if count == 8 {
                    valid = checksum(&digits);
                    format = "EAN8";
                } else {
                    let mut sys = 0;
                    let check = if let Some(c) = UPC_PARITY.iter().position(|&p| p == parity) {
                        c
                    } else if let Some(c) = UPC_PARITY.iter().position(|&p| p ^ 63 == parity) {
                        sys = 1;
                        c
                    } else {
                        continue;
                    };
                    digits.insert(0, sys);
                    digits.push(check as u8);
                    valid = checksum(&expand_upce(&digits));
                    format = "UPCE";
                }
                if !valid {
                    // The left half of EAN13 can mimic UPC-E without checksum
                    // validation. A genuine trailing quiet zone disambiguates an
                    // unread UPC-E candidate from the continuing EAN13 data bars.
                    let unambiguous = count != 6 || r.get(end).is_some_and(|&v| v >= module * 7.);
                    if retain_failed && failed.is_none() && unambiguous {
                        failed = Some(Read {
                            decoded: false,
                            addon: None,
                            codabar_start_stop: None,
                            format,
                            text: String::new(),
                            start: s,
                            end,
                            error: err / count as f32,
                            gs1: false,
                        });
                    }
                    continue;
                }
                let addon = if mask & (ADDON_READ | ADDON_REQUIRE) != 0 {
                    ean_addon(r, end, module, gain)
                } else {
                    None
                };
                if mask & ADDON_REQUIRE != 0 && addon.is_none() {
                    if retain_failed && failed.is_none() {
                        failed = Some(Read {
                            decoded: false,
                            addon: None,
                            codabar_start_stop: None,
                            format,
                            text: String::new(),
                            start: s,
                            end,
                            error: err / count as f32,
                            gs1: false,
                        });
                    }
                    continue;
                }
                return Some(Read {
                    decoded: true,
                    addon,
                    codabar_start_stop: None,
                    format,
                    text: digits.iter().map(|d| (b'0' + d) as char).collect(),
                    start: s,
                    end,
                    error: err / count as f32,
                    gs1: false,
                });
            }
        }
        failed
    }

    fn wide_bits_reference(r: &[f32], n: usize, wides: usize) -> Option<(u16, f32)> {
        if r.len() < n {
            return None;
        }
        let mut storage = [0f32; 9];
        let sorted = &mut storage[..n];
        sorted.copy_from_slice(&r[..n]);
        sorted.sort_by(f32::total_cmp);
        let narrow = sorted[..n - wides].iter().sum::<f32>() / crate::numeric::usize_f32(n - wides);
        let wide = sorted[n - wides..].iter().sum::<f32>() / crate::numeric::usize_f32(wides);
        if narrow < 0.65 || wide / narrow < 1.45 || wide / narrow > 3.8 {
            return None;
        }
        let cut = f32::midpoint(sorted[n - wides - 1], sorted[n - wides]);
        let mut bits = 0;
        let mut e = 0.;
        for &x in &r[..n] {
            let w = x > cut;
            bits = (bits << 1) | u16::from(w);
            let expected = if w { wide } else { narrow };
            if (x - expected).abs() > narrow * 0.9 {
                return None;
            }
            e += (x - expected).abs();
        }
        Some((bits, e / r[..n].iter().sum::<f32>()))
    }

    fn codabar_one_reference(r: &[f32]) -> Option<(usize, f32)> {
        let mut best = None;
        for wides in [2, 3] {
            if let Some((bits, e)) = wide_bits_reference(r, 7, wides)
                && let Some(i) = CODA.iter().position(|&p| u16::from(p) == bits)
                && best.is_none_or(|(_, old)| e < old)
            {
                best = Some((i, e));
            }
        }
        best
    }

    #[test]
    fn shared_wide_sort_matches_original_at_boundaries_and_random_inputs() {
        let boundary_inputs = [
            [0.65, 0.65, 0.65, 0.65, 0.95, 0.95, 0.95, 0.95, 0.95],
            [0.64, 0.65, 0.66, 0.94, 0.95, 0.96, 1.45, 1.46, 1.47],
            [1., 1., 1., 1., 1., 1., 1., 3.8, 3.8],
        ];
        for input in boundary_inputs {
            for &(n, wides) in &[(5, 2), (7, 2), (7, 3), (9, 3)] {
                let actual = wide_bits(&input, n, wides).map(|(b, e)| (b, e.to_bits()));
                let expected = wide_bits_reference(&input, n, wides).map(|(b, e)| (b, e.to_bits()));
                assert_eq!(actual, expected);
            }
            assert_eq!(
                codabar_one(&input[..7]).map(|(i, e)| (i, e.to_bits())),
                codabar_one_reference(&input[..7]).map(|(i, e)| (i, e.to_bits()))
            );
        }
        let mut seed = 1u32;
        for _ in 0..2000 {
            let mut input = [0f32; 9];
            for x in &mut input {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *x = f32::from(u16::try_from(seed % 1000).unwrap()) / 100.;
            }
            for &(n, wides) in &[(5, 2), (7, 2), (7, 3), (9, 3)] {
                let actual = wide_bits(&input, n, wides).map(|(b, e)| (b, e.to_bits()));
                let expected = wide_bits_reference(&input, n, wides).map(|(b, e)| (b, e.to_bits()));
                assert_eq!(actual, expected);
            }
            assert_eq!(
                codabar_one(&input[..7]).map(|(i, e)| (i, e.to_bits())),
                codabar_one_reference(&input[..7]).map(|(i, e)| (i, e.to_bits()))
            );
        }
    }

    #[test]
    fn code39_cropped_quiet_zones_keep_guards_and_interior_checks() {
        let mut runs = Vec::new();
        for &c in b"*TEST-SHEET*" {
            if !runs.is_empty() {
                runs.push(2.);
            }
            let pattern = C39[C39_ALPHABET.iter().position(|&v| v == c).unwrap()];
            runs.extend(
                (0..9)
                    .rev()
                    .map(|bit| if pattern & (1 << bit) == 0 { 2. } else { 5. }),
            );
        }
        let reads = decode(&runs, true, CODE39);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, "TEST-SHEET");
        assert!(decode(&runs[2..], true, CODE39).is_empty());
        assert!(decode(&runs[..runs.len() - 2], true, CODE39).is_empty());
        let mut clutter = vec![4., 1.];
        clutter.extend_from_slice(&runs);
        assert!(decode(&clutter, true, CODE39).is_empty());
        runs.extend([1., 4.]);
        assert!(decode(&runs, true, CODE39).is_empty());
    }
    #[test]
    fn long_code128_reports_cap_without_losing_short_reads() {
        for count in [20, 252, 256] {
            let mut values = vec![104];
            values.extend(std::iter::repeat_n(33, count));
            let check = (104 + (1..=count).map(|i| i * 33).sum::<usize>()) % 103;
            values.push(check);
            let mut runs = vec![10];
            for value in values {
                runs.extend(C128[value].map(|v| usize::from(v - b'0')));
            }
            runs.extend([2, 3, 3, 1, 1, 1, 2, 10]);
            let (image, w, h) = checksum_scene(&runs);
            let result = crate::scan(&image, w, h, CODE128, 0);
            assert_eq!(result.unfinished, count == 256);
            if count < 256 {
                assert_eq!(result.barcodes.len(), 2);
                assert!(result.barcodes.iter().all(|b| b.text == "A".repeat(count)));
            } else {
                assert!(result.barcodes.is_empty());
            }
        }
    }
    fn checksum_scene(runs: &[usize]) -> (Vec<u8>, usize, usize) {
        let width = runs.iter().sum::<usize>() * 3;
        let height = 180;
        let mut image = vec![255; width * height];
        let mut x = 0;
        for (index, &length) in runs.iter().enumerate() {
            let end = x + length * 3;
            if index % 2 == 1 {
                for y in (20..65).chain(110..155) {
                    image[y * width + x..y * width + end].fill(0);
                }
            }
            x = end;
        }
        (image, width, height)
    }
    #[test]
    fn unread_linear_regions_keep_distinct_instances() {
        let mut ean = vec![10, 1, 1, 1];
        for digit in [1, 2, 3, 4] {
            ean.extend(DIGITS[digit].map(usize::from));
        }
        ean.extend([1, 1, 1, 1, 1]);
        for digit in [5, 6, 7, 1] {
            ean.extend(DIGITS[digit].map(usize::from));
        }
        ean.extend([1, 1, 1, 10]);
        let mut ean_required = ean.clone();
        ean_required[37..41].copy_from_slice(&DIGITS[0].map(usize::from));
        let mut code128 = vec![10];
        let mut upce = vec![10, 1, 1, 1];
        for (i, digit) in [4, 2, 1, 0, 0, 0].into_iter().enumerate() {
            let mut pattern = DIGITS[digit];
            if UPC_PARITY[6] & (1 << (5 - i)) != 0 {
                pattern.reverse();
            }
            upce.extend(pattern.map(usize::from));
        }
        upce.extend([1, 1, 1, 1, 1, 1, 10]);
        for value in [104, 33, 34, 35, 0] {
            code128.extend(C128[value].map(|v| (v - b'0') as usize));
        }
        code128.extend([2, 3, 3, 1, 1, 1, 2, 10]);
        let mut code93 = vec![10];
        for value in [47, 10, 11, 12, 17, 21, 47] {
            code93.extend(c93_pattern(C93[value]).map(usize::from));
        }
        code93.extend([1, 10]);
        for (runs, mask, format) in [
            (ean, EAN8, "EAN8"),
            (ean_required, EAN8 | ADDON_REQUIRE, "EAN8"),
            (upce, UPCE, "UPCE"),
            (code128, CODE128, "Code128"),
            (code93, CODE93, "Code93"),
        ] {
            assert!(
                decode(
                    &runs
                        .iter()
                        .map(|&r| crate::numeric::usize_f32(r))
                        .collect::<Vec<_>>(),
                    false,
                    mask
                )
                .is_empty()
            );
            let (image, w, h) = checksum_scene(&runs);
            let result = crate::scan(&image, w, h, mask, 1);
            assert!(result.barcodes.is_empty(), "{format}");
            assert_eq!(result.regions.len(), 2, "{format}");
            assert!(
                result
                    .regions
                    .iter()
                    .all(|r| r.format == format && r.text.is_empty())
            );
        }
    }
    #[test]
    fn upce_expansion() {
        assert_eq!(
            expand_upce(&[0, 4, 2, 1, 0, 0, 0, 7]),
            vec![0, 4, 2, 0, 0, 0, 0, 0, 1, 0, 0, 7]
        );
        assert!(checksum(&expand_upce(&[0, 4, 2, 1, 0, 0, 0, 7])));
    }
    #[test]
    fn code128_switches() {
        assert_eq!(c128_text(&[105, 12, 34, 100, 33, 34]).unwrap().0, "1234AB");
        assert_eq!(
            c128_text(&[104, 102, 33, 102, 34]).unwrap(),
            ("A\u{1d}B".into(), true)
        );
    }
    #[test]
    fn rejects_short_noise() {
        assert!(decode(&[1.; 100], false, 511).is_empty());
    }
    fn msi_runs(text: &str, module: f32, bleed: f32) -> Vec<f32> {
        let pair = |one: bool| {
            if one {
                [2. * module + bleed, module - bleed]
            } else {
                [module + bleed, 2. * module - bleed]
            }
        };
        let mut runs = vec![20. * module];
        runs.extend(pair(true));
        for digit in text.bytes().map(|b| b - b'0') {
            for bit in (0..4).rev() {
                runs.extend(pair(digit & (1 << bit) != 0));
            }
        }
        runs.extend(pair(false));
        runs.extend([module + bleed, 20. * module]);
        runs
    }
    #[test]
    fn msi_plessey_reads_mod10_symbols() {
        assert_eq!(msi_check(&[1, 2, 3, 4, 5, 6]), 6);
        assert_eq!(msi_check(&[1, 2, 3, 4, 5, 6, 7]), 4);
        for (text, module, bleed) in [
            ("1234566", 2., 0.),
            ("12345674", 3., 0.6),
            ("00000", 1.5, -0.3),
            ("9876543210987658", 2.5, 0.4),
        ] {
            let reads = decode(&msi_runs(text, module, bleed), false, MSI_PLESSEY);
            assert_eq!(reads.len(), 1, "{text}");
            assert_eq!(
                (reads[0].format, reads[0].text.as_str()),
                ("MSIPlessey", text)
            );
        }
        // A wrong check digit, a too-short symbol and an unselected format read nothing.
        for (text, mask) in [
            ("1234567", MSI_PLESSEY),
            ("125", MSI_PLESSEY),
            ("1234566", 511),
        ] {
            assert!(decode(&msi_runs(text, 2., 0.), false, mask).is_empty());
        }
        // Without its quiet zone the start pair is not a symbol boundary.
        let mut crowded = vec![20., 2.];
        crowded.extend(msi_runs("1234566", 2., 0.));
        crowded[2] = 6.;
        assert!(decode(&crowded, false, MSI_PLESSEY).is_empty());
    }
    #[test]
    fn msi_plessey_rejects_other_symbologies_and_noise() {
        let mut state = 0x5eed_u32;
        for _ in 0..20_000 {
            let mut runs = vec![40.];
            runs.extend((0..60).map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                1. + f32::from(state.to_be_bytes()[0] % 4)
            }));
            runs.push(40.);
            assert!(decode(&runs, false, MSI_PLESSEY).is_empty());
        }
        let mut code39 = vec![20.];
        for &c in b"*CODE39*" {
            let bits = C39[C39_ALPHABET.iter().position(|&x| x == c).unwrap()];
            code39.extend(
                (0..9)
                    .rev()
                    .map(|i| if bits & (1 << i) != 0 { 6. } else { 2. }),
            );
            code39.push(2.);
        }
        code39.push(20.);
        assert!(decode(&code39, false, MSI_PLESSEY).is_empty());
    }
    #[test]
    fn ean_guards_are_checked_before_digit_work() {
        let mut runs = vec![1.; 70];
        assert!(ean_guards(&runs, 0, 59, 12));
        runs[27] = 10.;
        assert!(!ean_guards(&runs, 0, 59, 12));

        let mut upce = vec![1.; 40];
        assert!(ean_guards(&upce, 0, 33, 6));
        upce[27] = 10.;
        assert!(!ean_guards(&upce, 0, 33, 6));
    }

    #[expect(
        clippy::type_complexity,
        reason = "The tuple compares every observable Read field in the test-only parity oracle."
    )]
    fn parity_read(
        read: Option<Read>,
    ) -> Option<(
        bool,
        Option<String>,
        Option<&'static str>,
        &'static str,
        String,
        usize,
        usize,
        u32,
        bool,
    )> {
        read.map(|r| {
            (
                r.decoded,
                r.addon,
                r.codabar_start_stop,
                r.format,
                r.text,
                r.start,
                r.end,
                r.error.to_bits(),
                r.gs1,
            )
        })
    }

    #[test]
    fn ean_guard_reordering_matches_original_fixtures_and_perturbations() {
        let mut ean13 = vec![10., 1., 1., 1.];
        let left = [0, 0, 6, 3, 8, 1];
        for (i, &digit) in left.iter().enumerate() {
            let mut pattern = DIGITS[digit];
            if PARITY[4] & (1 << (5 - i)) != 0 {
                pattern.reverse();
            }
            ean13.extend(pattern.map(f32::from));
        }
        ean13.extend([1., 1., 1., 1., 1.]);
        for digit in [3, 3, 3, 9, 3, 1] {
            ean13.extend(DIGITS[digit].map(f32::from));
        }
        ean13.extend([1., 1., 1., 10.]);

        let mut ean8 = vec![10., 1., 1., 1.];
        for digit in [5, 5, 1, 2] {
            ean8.extend(DIGITS[digit].map(f32::from));
        }
        ean8.extend([1., 1., 1., 1., 1.]);
        for digit in [3, 4, 5, 7] {
            ean8.extend(DIGITS[digit].map(f32::from));
        }
        ean8.extend([1., 1., 1., 10.]);

        let mut upce = vec![10., 1., 1., 1.];
        for (i, digit) in [4, 2, 1, 0, 0, 0].into_iter().enumerate() {
            let mut pattern = DIGITS[digit];
            if UPC_PARITY[7] & (1 << (5 - i)) != 0 {
                pattern.reverse();
            }
            upce.extend(pattern.map(f32::from));
        }
        upce.extend([1., 1., 1., 1., 1., 1., 10.]);

        let mut invalid = ean8.clone();
        invalid[37..41].copy_from_slice(&DIGITS[0].map(f32::from));
        let failed = ean_reference(&invalid, 1, EAN8, true).expect("invalid checksum retained");
        assert!(!failed.decoded);
        assert_eq!(
            parity_read(ean(&invalid, 1, EAN8, true)),
            parity_read(Some(failed))
        );
        for (mut runs, mask) in [(ean13, EAN13 | UPCA), (ean8, EAN8), (upce, UPCE)] {
            assert!(
                ean_reference(&runs, 1, mask, false)
                    .expect("valid fixture")
                    .decoded
            );
            assert!(
                ean(&runs, 1, mask, false)
                    .expect("valid reordered fixture")
                    .decoded
            );
            for leading in [0., 0.49, 0.5, 1., 3.99, 4., 10.] {
                for prefix in [0, 2] {
                    let mut trial = vec![1.; prefix];
                    trial.extend_from_slice(&runs);
                    trial[prefix] = leading;
                    assert_eq!(
                        parity_read(ean(&trial, prefix + 1, mask, true)),
                        parity_read(ean_reference(&trial, prefix + 1, mask, true))
                    );
                }
            }
            for perturbation in 0..3 {
                if perturbation == 1 {
                    runs[10] += 0.25;
                } else if perturbation == 2 {
                    runs[20] += 1.;
                }
                assert_eq!(
                    parity_read(ean(&runs, 1, mask, true)),
                    parity_read(ean_reference(&runs, 1, mask, true))
                );
            }
        }
    }
}

#[cfg(test)]
mod itf_tests {
    use super::*;

    fn itf_runs(text: &str, module: f32, ratio: f32, spread: f32) -> Vec<f32> {
        let element = |wide: bool, bar: bool| {
            (if wide { ratio } else { 1. }) * module + if bar { spread } else { -spread }
        };
        let mut runs = vec![12. * module];
        for i in 0..4 {
            runs.push(element(false, i % 2 == 0));
        }
        for pair in text.as_bytes().chunks(2) {
            let bars = ITF_DIGITS[usize::from(pair[0] - b'0')];
            let spaces = ITF_DIGITS[usize::from(pair[1] - b'0')];
            for i in 0..5 {
                runs.push(element(bars & (16 >> i) != 0, true));
                runs.push(element(spaces & (16 >> i) != 0, false));
            }
        }
        runs.extend([
            element(true, true),
            element(false, false),
            element(false, true),
        ]);
        runs.push(12. * module);
        runs
    }

    #[test]
    fn reads_itf_with_ink_spread_and_a_local_defect() {
        for (text, module, ratio, spread) in [
            ("123456", 2., 2.5, 0.),
            ("00123456789012", 2.5, 3., 0.5),
            ("98765432", 1.5, 2.2, -0.3),
        ] {
            let runs = itf_runs(text, module, ratio, spread);
            let reads = decode(&runs, false, ITF);
            assert_eq!(reads.len(), 1, "{text}");
            assert_eq!(reads[0].text, text);
            // A stain widening one narrow bar halfway to a wide bar is still read.
            let mut stained = runs;
            let narrow_bar = (5..stained.len() - 4)
                .step_by(2)
                .find(|&i| stained[i] < module * 1.5)
                .unwrap();
            stained[narrow_bar] += (ratio - 1.) * module * 0.5;
            assert_eq!(decode(&stained, false, ITF).len(), 1, "{text}");
        }
    }

    /// Runs of a Code 39 symbol cut by a crop at random places, starting with the
    /// partial white run the crop leaves.
    fn cut_code39(rnd: &mut impl FnMut() -> f32) -> Vec<f32> {
        let ratio = 2. + rnd();
        let bias = (rnd() - 0.5) * 0.6;
        let noise = rnd() * 0.25;
        let mut text = vec![b'*'];
        let n = 6 + crate::numeric::f32_usize((rnd() * 4.).floor());
        text.extend(
            (0..n).map(|_| C39_ALPHABET[crate::numeric::f32_usize((rnd() * 43.).floor()) % 43]),
        );
        text.push(b'*');
        let mut runs = Vec::new();
        for (k, &c) in text.iter().enumerate() {
            let pattern = C39[C39_ALPHABET.iter().position(|&x| x == c).unwrap()];
            for i in (0..9).rev() {
                let wide = if pattern & (1 << i) != 0 { ratio } else { 1. };
                let ink = if i % 2 == 0 { bias } else { -bias };
                runs.push((wide + ink + (rnd() - 0.5) * noise) * 2.);
            }
            if k + 1 < text.len() {
                runs.push((1. - bias + (rnd() - 0.5) * noise) * 2.);
            }
        }
        let left = 2 * (1 + crate::numeric::f32_usize((rnd() * 11.).floor()));
        let mut out = vec![1. + 8. * rnd()];
        out.extend_from_slice(&runs[left..]);
        out.push(1. + 40. * rnd());
        out
    }

    /// Runs of a Code 39 symbol of 1 to 12 random characters at about a pixel per
    /// narrow module, with wide elements blurred to 1.3 to 1.7 narrow widths and
    /// element noise of up to `noise` / 2 narrow widths, and its value.
    fn dense_code39(rnd: &mut impl FnMut() -> f32, noise: f32) -> (Vec<f32>, String) {
        let n = 1 + crate::numeric::f32_usize(rnd() * 12.);
        let mut text = vec![b'*'];
        text.extend((0..n).map(|_| C39_ALPHABET[crate::numeric::f32_usize(rnd() * 43.) % 43]));
        text.push(b'*');
        let narrow = 1. + rnd() * 0.6;
        let ratio = 1.3 + rnd() * 0.4;
        let mut runs = vec![narrow * 14.];
        for (k, &c) in text.iter().enumerate() {
            let pattern = C39[C39_ALPHABET.iter().position(|&x| x == c).unwrap()];
            for i in (0..9).rev() {
                let width = if pattern & (1 << i) != 0 { ratio } else { 1. } * narrow;
                runs.push((width + (rnd() - 0.5) * noise * narrow).max(0.3));
            }
            if k + 1 < text.len() {
                runs.push((narrow + (rnd() - 0.5) * noise * narrow).max(0.3));
            }
        }
        runs.push(narrow * 14.);
        (
            runs,
            String::from_utf8(text[1..text.len() - 1].to_vec()).unwrap(),
        )
    }

    #[test]
    fn dense_code39_misreads_are_rare() {
        let mut state = 0x77_u32;
        let mut rnd = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f32::from(u16::try_from(state >> 16).unwrap()) / 65536.
        };
        let (mut right, mut wrong) = (0, 0);
        for _ in 0..20_000 {
            let (runs, text) = dense_code39(&mut rnd, 0.6);
            for read in decode(&runs, false, CODE39) {
                if read.text == text {
                    right += 1;
                } else {
                    wrong += 1;
                }
            }
        }
        // Blurred wide elements need the dense ratio; Code 39 has no check
        // character, so the whole-symbol model is what limits substitutions.
        assert!(
            right >= 6_000 && wrong <= 90,
            "{right} read, {wrong} misread"
        );
    }

    #[test]
    fn cut_code39_symbols_rarely_parse_as_itf() {
        let mut state = 0x1234_5678_u32;
        let mut rnd = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f32::from(u16::try_from(state >> 16).unwrap()) / 65535.
        };
        let mut itf = 0;
        for _ in 0..50_000 {
            let runs = cut_code39(&mut rnd);
            itf += decode(&runs, false, ITF | CODE39)
                .iter()
                .filter(|read| read.format == "ITF")
                .count();
        }
        // The narrow/wide model and the Code 39 structure check leave at most 1 in
        // 10,000 of these lines reading as ITF.
        assert!(itf <= 5, "{itf} ITF reads");
    }
}

#[cfg(test)]
mod symbol_tests;

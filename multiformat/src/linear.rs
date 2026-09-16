//! Independent run-length decoders. Tables describe standardized symbol patterns;
//! no `ZXing` reader implementation or runtime is used.

pub const EAN13: u32 = 1;
pub const UPCA: u32 = 2;
pub const EAN8: u32 = 4;
pub const UPCE: u32 = 8;
pub const CODE128: u32 = 16;
pub const CODE39: u32 = 32;
pub const ITF: u32 = 64;
pub const CODABAR: u32 = 128;
pub const CODE93: u32 = 256;
pub const DATABAR: u32 = 8192;
pub const DATABAR_EXPANDED: u32 = 16384;
pub const ADDON_READ: u32 = 32768;
pub const ADDON_REQUIRE: u32 = 65536;

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
const PARITY: [u8; 10] = [0, 11, 13, 14, 19, 25, 28, 21, 22, 26];
const UPC_PARITY: [u8; 10] = [56, 52, 50, 49, 44, 38, 35, 42, 41, 37];
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
const ITF_DIGITS: [u8; 10] = [6, 17, 9, 24, 5, 20, 12, 3, 18, 10];
const CODA: [u8; 20] = [
    3, 6, 9, 96, 18, 66, 33, 36, 48, 72, 12, 24, 69, 81, 84, 21, 26, 41, 11, 14,
];
const CODA_ALPHABET: &[u8] = b"0123456789-$:/.+ABCD";
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
fn c93_match(r: &[f32]) -> Option<(usize, f32)> {
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
    (best.1 < 0.23 && second - best.1 > 0.012).then_some(best)
}
fn code93(
    runs: &[f32],
    start_index: usize,
    retain_failed: bool,
    limited: &mut bool,
) -> Option<Read> {
    if error(&runs[start_index..], &c93_pattern(C93[47])) > 0.2 {
        return None;
    }
    let module = runs[start_index..start_index + 6].iter().sum::<f32>() / 9.;
    if start_index == 0 || (start_index != 1 && runs[start_index - 1] < module * 5.) {
        return None;
    }
    let mut at = start_index + 6;
    let mut values = Vec::new();
    let mut err = 0.;
    while at + 7 <= runs.len() && values.len() < 256 {
        let (v, e) = c93_match(&runs[at..])?;
        err += e;
        if v == 47 {
            if values.len() < 3
                || !quiet(runs, start_index, at + 7, module, 5.)
                || (runs[at + 6] - module).abs() > module * 0.8
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
                return retain_failed.then_some(Read {
                    decoded: false,
                    addon: None,
                    format: "Code93",
                    text: String::new(),
                    start: start_index,
                    end: at + 7,
                    error: err / (values.len() + 3) as f32,
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
                    let n = *values.get(i + 1)?;
                    if !(10..36).contains(&n) {
                        return None;
                    }
                    let letter = b'A' + (n - 10) as u8;
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
                format: "Code93",
                text: out,
                start: start_index,
                end: at + 7,
                error: err / (values.len() + 3) as f32,
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
    if module < 0.65 {
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
            / p.len() as f32)
            .clamp(-module * 0.5, module * 0.5)
    } else if p.len() > 1 && p.iter().all(|&x| x == 1) {
        let even = r.iter().take(p.len()).step_by(2).sum::<f32>() / p.len().div_ceil(2) as f32;
        let odd = r.iter().skip(1).take(p.len() - 1).step_by(2).sum::<f32>() / (p.len() / 2) as f32;
        module = (even + odd) * 0.5;
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
    if module < 0.65 {
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
            best = (i as u8, e);
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
fn ean(r: &[f32], s: usize, mask: u32, retain_failed: bool) -> Option<Read> {
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
        let black = (r[s] + r[s + 2]) * 0.5;
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
                digits.push(selected.0 .0);
                err += selected.0 .1;
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
        let mut parity = 0;
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
            digits.push(selected.0 .0 as usize);
            parity = parity * 2 + usize::from(selected.1);
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
        let expected = if count == 2 {
            (digits[0] * 10 + digits[1]) % 4
        } else {
            let check =
                (3 * (digits[0] + digits[2] + digits[4]) + 9 * (digits[1] + digits[3])) % 10;
            [24, 20, 18, 17, 12, 6, 3, 10, 9, 5][check]
        };
        if parity == expected {
            return Some(digits.iter().map(|&d| (b'0' + d as u8) as char).collect());
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
    if r.len() < 6 {
        return None;
    }
    let sum = r[..6].iter().sum::<f32>();
    let module = sum / 11.;
    if module < 0.65 {
        return None;
    }
    // Every six-element Code128 character spans eleven modules. Reuse its
    // scale and four possible element errors instead of recomputing them
    // independently for all 107 patterns.
    let distances: [[f32; 4]; 6] = if lo >= 103 {
        [[0.; 4]; 6]
    } else {
        std::array::from_fn(|j| std::array::from_fn(|n| (r[j] - (n + 1) as f32 * module).abs()))
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
    (best.1 < 0.23 && second - best.1 > 0.012).then_some(best)
}
fn code128(r: &[f32], s: usize, retain_failed: bool, limited: &mut bool) -> Option<Read> {
    let (start, mut err) = c128_match(&r[s..], 103, 106)?;
    let module = r[s..s + 6].iter().sum::<f32>() / 11.;
    if s == 0 || (s != 1 && r[s - 1] < module * 2.5) {
        return None;
    }
    let mut at = s + 6;
    let mut values = vec![start];
    while at + 7 <= r.len() && values.len() < 256 {
        let (v, e) = c128_match(&r[at..], 0, 107)?;
        err += e;
        if v == 106 {
            if error(&r[at..], &[2, 3, 3, 1, 1, 1, 2]) > 0.23
                || !quiet(r, s, at + 7, module, 2.5)
                || values.len() < 3
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
            if !valid && !retain_failed {
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
                format: "Code128",
                text,
                start: s,
                end: at + 7,
                error: err / values.len() as f32,
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
                    out.extend([b'0' + (c / 10) as u8, b'0' + (c % 10) as u8]);
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
            out.push(x as u8 + if upper ^ upper_once { 128 } else { 0 });
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
fn wide_bits(r: &[f32], n: usize, wides: usize) -> Option<(u16, f32)> {
    if r.len() < n {
        return None;
    }
    let mut storage = [0f32; 9];
    let sorted = &mut storage[..n];
    sorted.copy_from_slice(&r[..n]);
    sorted.sort_by(f32::total_cmp);
    let narrow = sorted[..n - wides].iter().sum::<f32>() / (n - wides) as f32;
    let wide = sorted[n - wides..].iter().sum::<f32>() / wides as f32;
    if narrow < 0.65 || wide / narrow < 1.45 || wide / narrow > 3.8 {
        return None;
    }
    let cut = (sorted[n - wides - 1] + sorted[n - wides]) * 0.5;
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
    if black_max.max(white_max) < black_min.min(white_min) {
        if let Some(read) = code39_with_gain(r, s, 0., limited) {
            return Some(read);
        }
    }
    let black = (start[0] + start[2] + start[8]) / 3.;
    let white = (start[3] + start[5] + start[7]) / 3.;
    let gain = (black - white) * 0.5;
    let narrow = (black + white) * 0.5;
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
    let (p, mut err) = wide_bits(&start, 9, 3)?;
    if p != C39[43] {
        return None;
    }
    let module = start.iter().copied().fold(f32::INFINITY, f32::min);
    if start_index == 0 || (start_index != 1 && runs[start_index - 1] < module * 5.) {
        return None;
    }
    let mut at = start_index + 10;
    let mut out = Vec::new();
    while at + 9 <= runs.len() && out.len() < 128 {
        let (p, e) = wide_bits(&adjusted(at), 9, 3)?;
        let d = C39.iter().position(|&x| x == p)?;
        err += e;
        if d == 43 {
            if out.len() < 2 || !quiet(runs, start_index, at + 9, module, 5.) {
                return None;
            }
            return Some(Read {
                decoded: true,
                addon: None,
                format: "Code39",
                text: String::from_utf8(out).ok()?,
                start: start_index,
                end: at + 9,
                error: err / (1. + ((at - start_index) / 10) as f32),
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
    if error(&r[s..], &[1, 1, 1, 1]) > 0.18 {
        return None;
    }
    let module = r[s..s + 4].iter().sum::<f32>() / 4.;
    if s == 0 || (s != 1 && r[s - 1] < module * 7.) {
        return None;
    }
    let mut at = s + 4;
    let mut out = Vec::new();
    let mut err = 0.;
    while at + 3 <= r.len() && out.len() < 80 {
        if out.len() >= 4
            && (error(&r[at..], &[3, 1, 1]) < 0.18 || error(&r[at..], &[2, 1, 1]) < 0.18)
            && quiet(r, s, at + 3, module, 7.)
        {
            return Some(Read {
                decoded: true,
                addon: None,
                format: "ITF",
                text: out.iter().map(|d| (b'0' + d) as char).collect(),
                start: s,
                end: at + 3,
                error: err / out.len() as f32,
                gs1: false,
            });
        }
        if at + 10 > r.len() {
            return None;
        }
        for parity in 0..2 {
            let seq: Vec<f32> = (0..5).map(|i| r[at + 2 * i + parity]).collect();
            let (p, e) = wide_bits(&seq, 5, 2)?;
            out.push(ITF_DIGITS.iter().position(|&x| u16::from(x) == p)? as u8);
            err += e;
        }
        at += 10;
    }
    *limited |= at + 3 <= r.len() && out.len() >= 80;
    None
}
fn codabar(r: &[f32], s: usize, limited: &mut bool) -> Option<Read> {
    fn one(r: &[f32]) -> Option<(usize, f32)> {
        let mut best = None;
        for wides in [2, 3] {
            if let Some((bits, e)) = wide_bits(r, 7, wides) {
                if let Some(i) = CODA.iter().position(|&p| u16::from(p) == bits) {
                    if best.is_none_or(|(_, old)| e < old) {
                        best = Some((i, e));
                    }
                }
            }
        }
        best
    }
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
            if out.len() < 3 {
                return None;
            }
            // A checkless reader must fit one physical narrow/wide model
            // across the complete symbol. Independent per-character fits can
            // otherwise reinterpret a short fragment of another symbology.
            let mut sums = [0f32; 4];
            let mut counts = [0usize; 4];
            for (index, &character) in characters.iter().enumerate() {
                for element in 0..7 {
                    let wide = usize::from(CODA[character] & (1 << (6 - element)) != 0);
                    let bucket = (element % 2) * 2 + wide;
                    sums[bucket] += r[s + index * 8 + element];
                    counts[bucket] += 1;
                }
            }
            if counts.contains(&0) {
                return None;
            }
            let means: [f32; 4] = std::array::from_fn(|i| sums[i] / counts[i] as f32);
            if [0, 2]
                .iter()
                .any(|&i| !(1.4..=4.1).contains(&(means[i + 1] / means[i])))
            {
                return None;
            }
            let mut deviation = 0.;
            for (index, &character) in characters.iter().enumerate() {
                for element in 0..7 {
                    let wide = usize::from(CODA[character] & (1 << (6 - element)) != 0);
                    let bucket = (element % 2) * 2 + wide;
                    let distance = (r[s + index * 8 + element] - means[bucket]).abs();
                    if distance > (means[bucket & !1] * 0.75).max(0.65) {
                        return None;
                    }
                    deviation += distance;
                }
            }
            let global_error = deviation / sums.iter().sum::<f32>();
            let module = (means[0] + means[2]) * 0.5;
            if global_error > 0.19 || !quiet(r, s, at + 7, module, 5.) {
                return None;
            }
            return Some(Read {
                decoded: true,
                addon: None,
                format: "Codabar",
                text: String::from_utf8(out).ok()?,
                start: s,
                end: at + 7,
                error: err / (1. + ((at - s) / 8) as f32),
                gs1: false,
            });
        }
        out.push(CODA_ALPHABET[c]);
        at += 8;
    }
    *limited |= at + 7 <= r.len() && out.len() >= 128;
    None
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
    while s + 16 <= r.len() {
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
        if quiet && mask & 15 != 0 {
            found = accepted(ean(r, s, mask, retain_failed));
        }
        if quiet && found.is_none() && mask & CODE128 != 0 {
            found = accepted(code128(r, s, retain_failed, limited));
        }
        if quiet && found.is_none() && mask & CODE39 != 0 {
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
        if quiet && found.is_none() && mask & ITF != 0 {
            found = itf(r, s, limited);
        }
        if quiet && found.is_none() && mask & CODABAR != 0 {
            found = codabar(r, s, limited);
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
            assert!(decode(
                &runs.iter().map(|&r| r as f32).collect::<Vec<_>>(),
                false,
                mask
            )
            .is_empty());
            let (image, w, h) = checksum_scene(&runs);
            let result = crate::scan(&image, w, h, mask, 1);
            assert!(result.barcodes.is_empty(), "{format}");
            assert_eq!(result.regions.len(), 2, "{format}");
            assert!(result
                .regions
                .iter()
                .all(|r| r.format == format && r.text.is_empty()));
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
}

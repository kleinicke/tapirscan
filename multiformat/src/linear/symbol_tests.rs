//! Generated-symbol regressions for Code 93, Code 39, Codabar and Code 128.
use super::*;

/// Element widths in pixels: `module` pixels per module, every bar widened and every
/// space narrowed by `spread` pixels (printing gain or thresholding bias).
fn widths(modules: &[u8], module: f32, spread: f32, first_bar: bool) -> Vec<f32> {
    modules
        .iter()
        .enumerate()
        .map(|(i, &m)| {
            let bar = (i % 2 == 0) == first_bar;
            f32::from(m) * module + if bar { spread } else { -spread }
        })
        .collect()
}

/// Code 93 values for ASCII text, with the standard full-ASCII shift pairs.
fn c93_values(text: &str) -> Vec<usize> {
    let letter = |c: u8| usize::from(c - b'A') + 10;
    let mut values = Vec::new();
    for &c in text.as_bytes() {
        if let Some(i) = C93_ALPHABET.iter().position(|&a| a == c) {
            values.push(i);
            continue;
        }
        let (shift, l) = match c {
            1..=26 => (43, b'A' + c - 1),
            0 => (44, b'U'),
            27..=31 => (44, c + 38),
            b';'..=b'?' => (44, c + 11),
            b'['..=b'_' => (44, c - 16),
            b'{'..=127 => (44, c - 43),
            b'@' => (44, b'V'),
            b'`' => (44, b'W'),
            b'!'..=b',' => (45, c + 32),
            b':' => (45, b'Z'),
            b'a'..=b'z' => (46, c - 32),
            _ => unreachable!("{c}"),
        };
        values.extend([shift, letter(l)]);
    }
    for cap in [20, 15] {
        let check = values
            .iter()
            .rev()
            .enumerate()
            .map(|(i, &v)| v * (i % cap + 1))
            .sum::<usize>()
            % 47;
        values.push(check);
    }
    values
}

fn code93_runs(text: &str, module: f32, spread: f32) -> Vec<f32> {
    let mut modules = vec![14];
    for v in std::iter::once(47)
        .chain(c93_values(text))
        .chain(std::iter::once(47))
    {
        modules.extend(c93_pattern(C93[v]));
    }
    modules.extend([1, 14]);
    widths(&modules, module, spread, false)
}

#[test]
fn code93_full_ascii_round_trip() {
    let all: Vec<u8> = (0..128).collect();
    for chunk in all.chunks(16) {
        let text = String::from_utf8(chunk.to_vec()).unwrap();
        let reads = decode(&code93_runs(&text, 3., 0.), false, CODE93);
        assert_eq!(reads.len(), 1, "{text:?}");
        assert_eq!(reads[0].text, text);
    }
}

#[test]
fn long_code93_with_ink_spread_and_special_characters() {
    let text = "SHIP $12.50/BOX-7 %OFF+ref:a1b2c3d4e5f6";
    for spread in [-1.35, -0.6, 0., 0.6, 1.35] {
        let reads = decode(&code93_runs(text, 3., spread), false, CODE93);
        assert_eq!(reads.len(), 1, "spread {spread}");
        assert_eq!(reads[0].text, text);
    }
}

#[test]
fn code93_judges_the_termination_bar_at_the_stop_scale() {
    // Perspective: the module grows from 2 to 4.4 pixels along the symbol.
    let flat = code93_runs("PERSPECTIVE TEST 1234", 1., 0.);
    let n = crate::numeric::usize_f32(flat.len());
    let runs: Vec<f32> = flat
        .iter()
        .enumerate()
        .map(|(i, &w)| w * (2. + 2.4 * crate::numeric::usize_f32(i) / n))
        .collect();
    let reads = decode(&runs, false, CODE93);
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].text, "PERSPECTIVE TEST 1234");
}

#[test]
fn code93_weak_characters_need_both_checks() {
    let mut values = c93_values("WEAKCHECK");
    let last = values.len() - 1;
    values[last] = (values[last] + 1) % 47;
    let mut modules = vec![14];
    for v in std::iter::once(47).chain(values).chain(std::iter::once(47)) {
        modules.extend(c93_pattern(C93[v]));
    }
    modules.extend([1, 14]);
    // Strong spread forces edge matching; a wrong K check must reject the symbol
    // without retaining a failed candidate.
    let runs = widths(&modules, 3., 1.35, false);
    let mut limited = false;
    assert!(
        decode_candidates(&runs, false, CODE93, &mut limited)
            .iter()
            .all(|r| !r.decoded)
    );
}

fn code39_runs(text: &[u8], quiet: f32) -> Vec<f32> {
    let mut runs = vec![quiet];
    for (i, &c) in [b"*".as_slice(), text, b"*"].concat().iter().enumerate() {
        if i > 0 {
            runs.push(2.);
        }
        let pattern = C39[C39_ALPHABET.iter().position(|&v| v == c).unwrap()];
        runs.extend(
            (0..9)
                .rev()
                .map(|bit| if pattern & (1 << bit) == 0 { 2. } else { 5. }),
        );
    }
    runs.push(quiet);
    runs
}

#[test]
fn single_character_code39_needs_full_quiet_zones() {
    let reads = decode(&code39_runs(b"4", 24.), false, CODE39);
    assert_eq!(reads.len(), 1);
    assert_eq!(reads[0].text, "4");
    assert_eq!(
        decode(&code39_runs(b"AB", 24.), false, CODE39)[0].text,
        "AB"
    );
    // Five narrow modules are enough for longer symbols, not for one character.
    assert!(decode(&code39_runs(b"4", 12.), false, CODE39).is_empty());
    assert_eq!(
        decode(&code39_runs(b"ABC", 12.), false, CODE39)[0].text,
        "ABC"
    );
}

fn codabar_runs(text: &[u8], quiet: f32) -> Vec<f32> {
    let mut runs = vec![quiet];
    for (i, &c) in text.iter().enumerate() {
        if i > 0 {
            runs.push(2.);
        }
        let pattern = CODA[CODA_ALPHABET.iter().position(|&v| v == c).unwrap()];
        runs.extend(
            (0..7)
                .rev()
                .map(|bit| if pattern & (1 << bit) == 0 { 2. } else { 5. }),
        );
    }
    runs.push(quiet);
    runs
}

#[test]
fn short_codabar_needs_full_quiet_zones() {
    for (symbol, text) in [(&b"A4B"[..], "4"), (b"C12D", "12"), (b"A123B", "123")] {
        let reads = decode(&codabar_runs(symbol, 24.), false, CODABAR);
        assert_eq!(reads.len(), 1, "{text}");
        assert_eq!(reads[0].text, text);
    }
    assert!(decode(&codabar_runs(b"A4B", 12.), false, CODABAR).is_empty());
    assert_eq!(
        decode(&codabar_runs(b"A123B", 12.), false, CODABAR)[0].text,
        "123"
    );
}

fn code128_runs(values: &[usize], module: f32, spread: f32) -> Vec<f32> {
    let check = (values[0]
        + values
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, &v)| i * v)
            .sum::<usize>())
        % 103;
    let mut modules = vec![12];
    for &v in values.iter().chain(std::iter::once(&check)) {
        modules.extend(C128[v].map(|d| d - b'0'));
    }
    modules.extend([2, 3, 3, 1, 1, 1, 2, 12]);
    widths(&modules, module, spread, false)
}

#[test]
fn code128_edge_matching_tolerates_ink_spread() {
    // Start B, "Tapir-128", ...
    let values: Vec<usize> = std::iter::once(104)
        .chain(b"Tapir-128 ink".iter().map(|&c| usize::from(c - 32)))
        .collect();
    for spread in [-1.35, 0., 1.35] {
        let reads = decode(&code128_runs(&values, 3., spread), false, CODE128);
        assert_eq!(reads.len(), 1, "spread {spread}");
        assert_eq!(reads[0].text, "Tapir-128 ink");
    }
    // A wrong check character still rejects the symbol.
    let mut runs = code128_runs(&values, 3., 1.35);
    let check_at = 1 + 6 * values.len();
    runs[check_at..check_at + 6].copy_from_slice(&widths(
        &C128[7].map(|d| d - b'0'),
        3.,
        1.35,
        true,
    ));
    let mut limited = false;
    assert!(
        decode_candidates(&runs, false, CODE128, &mut limited)
            .iter()
            .all(|r| !r.decoded)
    );
}

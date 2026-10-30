use super::*;

// Standardized module patterns independently emitted by Zint 2.16.0, TELEPEN,
// binary input 0..63 and 64..127. Each word excludes start/check/stop.
const WORDS: [u16; 128] = [
    0xeeee, 0xbbba, 0xe3ba, 0xaeee, 0xebba, 0xb8ee, 0x88ee, 0xabba, 0xee3a, 0xbaee, 0xe2ee, 0xae3a,
    0xeaee, 0xa23a, 0x8a3a, 0xaaee, 0xeeba, 0xbb8e, 0xe38e, 0xaeba, 0xeb8e, 0xb8ba, 0x88ba, 0xab8e,
    0xe88e, 0xbaba, 0xe2ba, 0xa88e, 0xeaba, 0xa28e, 0x8a8e, 0xaaba, 0xeee2, 0xbbae, 0xe3ae, 0xaee2,
    0xebae, 0xb8e2, 0x88e2, 0xabae, 0xee2e, 0xbae2, 0xe2e2, 0xae2e, 0xeae2, 0xa22e, 0x8a2e, 0xaae2,
    0xeeae, 0xba22, 0xe222, 0xaeae, 0xea22, 0xb8ae, 0x88ae, 0xaa22, 0xe8a2, 0xbaae, 0xe2ae, 0xa8a2,
    0xeaae, 0xa2a2, 0x8aa2, 0xaaae, 0xeeea, 0xbbb8, 0xe3b8, 0xaeea, 0xebb8, 0xb8ea, 0x88ea, 0xabb8,
    0xee38, 0xbaea, 0xe2ea, 0xae38, 0xeaea, 0xa238, 0x8a38, 0xaaea, 0xeeb8, 0xbb8a, 0xe38a, 0xaeb8,
    0xeb8a, 0xb8b8, 0x88b8, 0xab8a, 0xe88a, 0xbab8, 0xe2b8, 0xa88a, 0xeab8, 0xa28a, 0x8a8a, 0xaab8,
    0xee88, 0xbbaa, 0xe3aa, 0xae88, 0xebaa, 0xb888, 0x8888, 0xabaa, 0xee2a, 0xba88, 0xe288, 0xae2a,
    0xea88, 0xa22a, 0x8a2a, 0xaa88, 0xeeaa, 0xba28, 0xe228, 0xaeaa, 0xea28, 0xb8aa, 0x88aa, 0xaa28,
    0xe8a8, 0xbaaa, 0xe2aa, 0xa8a8, 0xeaaa, 0xa2a8, 0x8aa8, 0xaaaa,
];

fn word_runs(word: u16) -> Vec<f32> {
    let mut out = Vec::new();
    let mut prev = true;
    let mut width = 0.;
    for bit in (0..16).rev() {
        let black = word & (1 << bit) != 0;
        if black != prev {
            out.push(width);
            width = 0.;
            prev = black;
        }
        width += 1.;
    }
    out.push(width);
    out
}

pub(super) fn symbol(data: &[u8], variant: usize) -> Vec<f32> {
    let headers = [
        vec![1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 3., 3.],
        vec![1., 1., 1., 1., 1., 1., 1., 1., 3., 1., 1., 3.],
        vec![1., 1., 1., 1., 1., 1., 3., 1., 1., 1., 1., 3.],
    ];
    let mut r = vec![12.];
    r.extend(&headers[variant]);
    let sum: usize = data.iter().map(|&c| usize::from(c)).sum();
    for c in data.iter().copied().chain(std::iter::once(
        u8::try_from((127 - sum % 127) % 127).unwrap(),
    )) {
        r.extend(word_runs(WORDS[usize::from(c)]));
    }
    r.extend(headers[variant].iter().rev().take(11));
    r.push(12.);
    r
}

#[test]
fn every_ascii_character_and_binary_controls() {
    for c in 0u8..128 {
        for (module, spread) in [(1., 0.), (2., 0.4), (3.5, -0.8)] {
            let mut r = symbol(&[c], 0);
            for (i, w) in r.iter_mut().enumerate() {
                *w = *w * module + if i % 2 == 1 { spread } else { -spread };
            }
            let read =
                decode(&r, 1, false, &mut false).unwrap_or_else(|| panic!("{c} {module} {spread}"));
            assert_eq!(read.text.as_bytes(), &[c]);
            assert_eq!(read.end, r.len() - 1);
        }
    }
    let data: Vec<u8> = (0..64).collect();
    assert_eq!(
        decode(&symbol(&data, 0), 1, false, &mut false)
            .unwrap()
            .text
            .as_bytes(),
        data
    );
}

#[test]
fn explicit_numeric_policy_never_guesses_ascii() {
    let words = [27, 39, 61, 83]; // 00 12 34 56
    let r = symbol(&words, 0);
    assert_eq!(
        decode(&r, 1, false, &mut false).unwrap().text.as_bytes(),
        words
    );
    let read = decode(&r, 1, true, &mut false).unwrap();
    assert_eq!(read.text, "00123456");
    assert_eq!(read.format, "TelepenNumeric");
    assert_eq!(
        decode(&symbol(&[17, 26], 0), 1, true, &mut false)
            .unwrap()
            .text,
        "0X9X"
    );
}

#[test]
fn aim_modes_and_single_direction_switch() {
    assert_eq!(
        decode(&symbol(&[27, 39, 16, b'A'], 1), 1, false, &mut false)
            .unwrap()
            .text,
        "0012A"
    );
    assert_eq!(
        decode(&symbol(&[b'A', 16, 27, 39], 2), 1, false, &mut false)
            .unwrap()
            .text,
        "A0012"
    );
    assert!(decode(&symbol(&[27, 16, b'A', 16, 27], 1), 1, false, &mut false).is_none());
    assert_eq!(
        decode(&symbol(&[16], 0), 1, false, &mut false)
            .unwrap()
            .text,
        "\x10"
    );
}

#[test]
fn rejects_checksum_parity_stop_and_truncation() {
    let good = symbol(b"TELEPEN", 0);
    for len in 0..good.len() {
        assert!(decode(&good[..len], 1, false, &mut false).is_none());
    }
    let mut bad = good.clone();
    bad[0] = 1.;
    assert!(decode(&bad, 1, false, &mut false).is_none());
    let mut bad = symbol(b"A", 0);
    let at = 13 + word_runs(WORDS[65]).len();
    let end = at + word_runs(WORDS[62]).len(); // checksum for 'A'
    bad.splice(at..end, word_runs(WORDS[61]));
    assert!(decode(&bad, 1, false, &mut false).is_none());
    for variant in 1..3 {
        let mut bad = symbol(b"AB", variant);
        let len = bad.len();
        bad[len - 12..len - 1].copy_from_slice(&good[good.len() - 12..good.len() - 1]);
        assert!(decode(&bad, 1, false, &mut false).is_none());
    }
    // A printed-module flip is visual damage, not necessarily a changed data
    // bit: edge fitting may recover the original word. Across the full ASCII
    // alphabet it must never turn that damage into a different payload.
    for c in 0u8..128 {
        for bit in 0..16 {
            let r = word_runs(WORDS[usize::from(c)] ^ (1 << bit));
            if r.contains(&0.) {
                continue;
            }
            let mut damaged = symbol(&[c], 0);
            damaged.splice(13..13 + word_runs(WORDS[usize::from(c)]).len(), r);
            if let Some(read) = decode(&damaged, 1, false, &mut false) {
                assert_eq!(
                    read.text.as_bytes(),
                    &[c],
                    "character {c}, flipped module {bit}"
                );
                assert_eq!(read.end, damaged.len() - 1);
            }
        }
    }
}

#[test]
fn bounded_work_and_no_cross_symbol_join() {
    let mut limited = false;
    assert!(decode(&symbol(&[b'A'; 128], 0), 1, false, &mut limited).is_none());
    assert!(limited);
    let first = symbol(b"FIRST", 0);
    let second = symbol(b"SECOND", 0);
    let mut both = first.clone();
    both.extend_from_slice(&second[1..]);
    let read = decode(&both, 1, false, &mut false).unwrap();
    assert_eq!(read.text, "FIRST");
    assert_eq!(read.end, first.len() - 1);
    assert_eq!(
        decode(&both, first.len(), false, &mut false).unwrap().text,
        "SECOND"
    );
    for n in 32..300 {
        assert!(decode(&vec![1.; n], 1, false, &mut false).is_none());
        assert!(decode(&vec![3.; n], 1, false, &mut false).is_none());
    }
}

#[test]
fn allows_gradual_perspective_but_rejects_scale_jump() {
    let mut r = symbol(b"PERSPECTIVE", 0);
    let len = r.len();
    for (i, w) in r.iter_mut().enumerate() {
        *w *= 2. + crate::numeric::usize_f32(i) / crate::numeric::usize_f32(len);
    }
    assert_eq!(
        decode(&r, 1, false, &mut false).unwrap().text,
        "PERSPECTIVE"
    );
    let mut r = symbol(b"AB", 0);
    for w in &mut r[13..] {
        *w *= 2.;
    }
    assert!(decode(&r, 1, false, &mut false).is_none());
}

#[test]
fn rounded_prefix_does_not_bias_the_payload_ink_estimate() {
    let mut r = symbol(b"A", 0);
    for (i, width) in r.iter_mut().enumerate().take(7).skip(1) {
        *width += if i % 2 == 1 { 0.2 } else { -0.2 };
    }
    for (i, width) in r.iter_mut().enumerate().take(15).skip(13) {
        *width += if i % 2 == 1 { -0.45 } else { 0.45 };
    }
    assert_eq!(decode(&r, 1, false, &mut false).unwrap().text, "A");
}

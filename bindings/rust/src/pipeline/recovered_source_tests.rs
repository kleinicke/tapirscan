use super::*;

const DIGITS: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
const OTHER: [u8; 13] = [9, 7, 8, 0, 2, 0, 1, 3, 7, 9, 6, 2, 4];
const WIDTH: usize = 412;
const HEIGHT: usize = 100;
const SYMBOL: Quad = [[16., 10.], [396., 10.], [396., 90.], [16., 90.]];

fn fixture(black: u8, white: u8) -> Vec<u8> {
    let bits = barcode_research_core::ean::encode(&DIGITS);
    let mut pixels = vec![white; WIDTH * HEIGHT];
    for y in 8..92 {
        for (module, bit) in bits.iter().enumerate() {
            if *bit > 0.5 {
                for x in 16 + module * 4..20 + module * 4 {
                    pixels[y * WIDTH + x] = black;
                }
            }
        }
    }
    pixels
}

fn image(pixels: &[u8]) -> Image<'_> {
    Image {
        data: pixels,
        width: WIDTH,
        height: HEIGHT,
        channels: 1,
        stride: WIDTH,
    }
}

#[test]
fn raw_evidence_rejects_unrelated_payload_and_accepts_either_direction() {
    let pixels = fixture(20, 235);
    assert!(recovered_ean_source_agreement(
        image(&pixels),
        SYMBOL,
        &DIGITS
    ));
    assert!(!recovered_ean_source_agreement(
        image(&pixels),
        SYMBOL,
        &OTHER
    ));
    let reversed = [SYMBOL[2], SYMBOL[3], SYMBOL[0], SYMBOL[1]];
    assert!(recovered_ean_source_agreement(
        image(&pixels),
        reversed,
        &DIGITS
    ));
}

#[test]
fn raw_evidence_needs_contrast_and_separate_source_rows() {
    let faint = fixture(120, 130);
    assert!(!recovered_ean_source_agreement(
        image(&faint),
        SYMBOL,
        &DIGITS
    ));
    let flat = vec![128; WIDTH * HEIGHT];
    assert!(!recovered_ean_source_agreement(
        image(&flat),
        SYMBOL,
        &DIGITS
    ));
    let pixels = fixture(20, 235);
    let thin = [[16., 49.], [396., 49.], [396., 51.], [16., 51.]];
    assert!(!recovered_ean_source_agreement(
        image(&pixels),
        thin,
        &DIGITS
    ));
}

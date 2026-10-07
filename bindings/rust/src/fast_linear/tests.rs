use super::*;

#[test]
fn public_low_selection_preserves_explicit_recovery_requests() {
    use crate::formats::EanAddOnPolicy;
    let basic = ScanOptions::default();
    assert_eq!(
        enabled(basic, 1, EanAddOnPolicy::Ignore),
        crate::LOW_FAST_PATH
    );
    assert!(!enabled(basic, 512, EanAddOnPolicy::Ignore));
    assert!(!enabled(basic, 1, EanAddOnPolicy::Read));
    assert!(!enabled(basic, 1, EanAddOnPolicy::Require));
    assert!(!enabled(
        ScanOptions {
            finish_candidates: true,
            ..basic
        },
        1,
        EanAddOnPolicy::Ignore
    ));
}

/// Independent ZXing-writer module patterns, reproduced without an encoder dependency.
/// Small UPC-E modules previously aliased into another checksum-valid payload; a
/// large EAN-8 exposed a clipped sparse proposal and needs the bounded axis retry.
#[test]
fn source_density_and_axis_retry_preserve_clean_retail_values() {
    let upce = b"101001110100100110111001001101101011110011001010101";
    let ean8 = b"1010001011010111101111010110111010101001110111001010001001011100101";
    for (bits, scale, padding, expected) in [
        (&upce[..], 1, 20, "04252614"),
        (&upce[..], 1, 60, "04252614"),
        (&ean8[..], 6, 60, "96385074"),
    ] {
        for rotated in [false, true] {
            let width = bits.len() * scale + 2 * padding;
            let height = 55 * scale + 2 * padding;
            let (output_width, output_height) = if rotated {
                (height, width)
            } else {
                (width, height)
            };
            let mut pixels = vec![255; width * height];
            for y in padding..height - padding {
                for x in padding..width - padding {
                    let at = if rotated {
                        x * output_width + height - 1 - y
                    } else {
                        y * width + x
                    };
                    pixels[at] = if bits[(x - padding) / scale] == b'1' {
                        0
                    } else {
                        255
                    };
                }
            }
            let image = Image {
                data: &pixels,
                width: output_width,
                height: output_height,
                channels: 1,
                stride: output_width,
            };
            let result = scan(&mut Scanner::default(), image, ScanOptions::default(), 127).unwrap();
            assert_eq!(
                result
                    .barcodes
                    .iter()
                    .map(|read| read.text.as_str())
                    .collect::<Vec<_>>(),
                [expected],
                "tier={TIER}, scale={scale}, padding={padding}, rotated={rotated}"
            );
        }
    }
}

#[test]
fn axis_confirmation_preserves_two_nearby_equal_ean8_symbols() {
    let bits = b"1010001011010111101111010110111010101001110111001010001001011100101";
    let (width, height) = (340, 220);
    let mut pixels = vec![255; width * height];
    for top in [20, 120] {
        for y in top..top + 80 {
            for (i, bit) in bits.iter().enumerate() {
                if *bit == b'1' {
                    pixels[y * width + 36 + i * 4..y * width + 40 + i * 4].fill(0);
                }
            }
        }
    }
    let image = Image {
        data: &pixels,
        width,
        height,
        channels: 1,
        stride: width,
    };
    for mask in [4, 15, 127] {
        let result = scan(&mut Scanner::default(), image, ScanOptions::default(), mask).unwrap();
        assert_eq!(result.barcodes.len(), 2, "tier={TIER}, mask={mask}");
        assert!(result
            .barcodes
            .iter()
            .all(|r| r.text == "96385074" && r.format.as_str() == "EAN8"));
        let mut centers: Vec<_> = result
            .barcodes
            .iter()
            .map(|r| r.polygon.iter().map(|p| p[1]).sum::<f64>() / 4.)
            .collect();
        centers.sort_by(f64::total_cmp);
        assert!((20.0..100.0).contains(&centers[0]));
        assert!((120.0..200.0).contains(&centers[1]));
    }
}

#[test]
fn short_clean_bars_keep_three_rows_and_do_not_invent_itf() {
    if TIER != 16 {
        return;
    }
    let itf = b"101011101000101011100011101110100010100011101000111000101010001010111000111011101";
    let code39 = b"1001011011010101101011001011011010010101101010010110101101010011011010110010101011001010110101001101011010100110110101011001011010100101101101";
    for (bits, scale, padding, expected) in [
        (&itf[..], 1, 60, "12345678"),
        (&code39[..], 1, 60, "SCALE2409"),
        (&code39[..], 4, 60, "SCALE2409"),
    ] {
        for rotated in [false, true] {
            let width = bits.len() * scale + 2 * padding;
            let height = 50 * scale + 2 * padding;
            let (output_width, output_height) = if rotated {
                (height, width)
            } else {
                (width, height)
            };
            let mut pixels = vec![255; width * height];
            for y in padding..height - padding {
                for x in padding..width - padding {
                    let at = if rotated {
                        x * output_width + height - 1 - y
                    } else {
                        y * width + x
                    };
                    pixels[at] = if bits[(x - padding) / scale] == b'1' {
                        0
                    } else {
                        255
                    };
                }
            }
            let image = Image {
                data: &pixels,
                width: output_width,
                height: output_height,
                channels: 1,
                stride: output_width,
            };
            let result = scan(&mut Scanner::default(), image, ScanOptions::default(), 127).unwrap();
            assert_eq!(
                result
                    .barcodes
                    .iter()
                    .map(|read| read.text.as_str())
                    .collect::<Vec<_>>(),
                [expected],
                "tier={TIER}, scale={scale}, padding={padding}, rotated={rotated}"
            );
        }
    }
}

#[test]
fn nearby_confirmation_counts_distinct_rows_and_preserves_extent() {
    let mut pixels = vec![255; 100 * 60];
    for y in 0..60 {
        for x in 10..90 {
            if x / 3 % 2 == 0 {
                pixels[y * 100 + x] = 0;
            }
        }
    }
    let image = Image {
        data: &pixels,
        width: 100,
        height: 60,
        channels: 1,
        stride: 100,
    };
    let mut candidate = Candidate {
        image,
        im: ImageView::new(&pixels, 100, 60, 1, 100).unwrap(),
        quad: [[10., 10.], [90., 10.], [90., 50.], [10., 50.]],
        mask: linear::CODE128,
        localized: true,
        dense: false,
        restored: false,
        profile: SourceProfile::Gray(0.),
        remaining: 100_000,
        observations: Vec::new(),
        row_positions: Vec::new(),
    };
    let read = || linear::Read {
        decoded: true,
        addon: None,
        format: "Code128",
        text: "example".into(),
        start: 0,
        end: 40,
        error: 0.,
        gs1: false,
    };
    let mut sampler = barcode_research_core::fast_profile::Sampler::default();
    let (mut budget, mut used, mut dense_budget, mut dense_used) = (100_000, 0, 100_000, 0);
    assert_eq!(
        candidate.refine(
            &mut sampler,
            &mut budget,
            &mut used,
            &mut dense_budget,
            &mut dense_used
        ),
        0
    );
    assert_eq!((used, dense_used), (0, 0));
    candidate.admit(read(), 0., 1., 6, 0.5);
    candidate.admit(read(), 0., 1., 19, 0.5);
    assert_eq!(candidate.observations[0].read.support, 1);
    candidate.admit(read(), 0., 1., 6, 0.482);
    candidate.admit(read(), 0., 1., 6, 0.518);
    assert_eq!(candidate.observations.len(), 1);
    candidate.admit(read(), 0., 1., 6, 0.5);
    candidate.admit(read(), 0., 1., 13, 0.482);
    let observation = &candidate.observations[0];
    assert_eq!(observation.read.support, 3);
    assert!((observation.first - 0.482).abs() < 1e-9);
    assert!((observation.last - 0.518).abs() < 1e-9);
}

#[test]
fn truncated_proposal_is_not_a_cropped_image() {
    let pixels = vec![255; 100 * 60];
    let image = ImageView::new(&pixels, 100, 60, 1, 100).unwrap();
    let read = linear::Read {
        decoded: true,
        addon: None,
        format: "ITF",
        text: "0240".into(),
        start: 1,
        end: 8,
        error: 0.,
        gs1: false,
    };
    let mut runs = vec![0.5, 1., 1., 1., 1., 3., 1., 1., 0.5];
    assert!(!source_quiet(&runs, &read, image, [[20., 30.], [80., 30.]]));
    assert!(source_quiet(&runs, &read, image, [[-1., 30.], [100., 30.]]));
    assert!(source_quiet(&runs, &read, image, [[20., -1.], [80., 60.]]));
    runs[0] = 7.;
    runs[8] = 7.;
    assert!(source_quiet(&runs, &read, image, [[20., 30.], [80., 30.]]));
}

#[test]
fn observation_join_respects_single_pixel_separator() {
    fn image(data: &[u8]) -> Image<'_> {
        Image {
            data,
            width: 100,
            height: 60,
            channels: 1,
            stride: 100,
        }
    }
    let mut pixels = vec![255; 100 * 60];
    for y in 0..60 {
        for x in 10..90 {
            if (x / 3) % 2 == 0 {
                pixels[y * 100 + x] = 0;
            }
        }
    }
    let q = [[10., 10.], [90., 10.], [90., 11.], [10., 11.]];
    let r = [[10., 40.], [90., 40.], [90., 41.], [10., 41.]];
    assert!(crate::linear_duplicates::connect_fast(
        q,
        r,
        image(&pixels),
        &mut 100_000
    ));
    pixels[25 * 100..26 * 100].fill(255);
    assert!(!crate::linear_duplicates::connect_fast(
        q,
        r,
        image(&pixels),
        &mut 100_000
    ));
}

/// A small-module EAN-13 (two pixels per module) blurred by 0.7 modules leaves no
/// decodable ordinary row. Public Low and Turbo2 recover it on the strongest proposal
/// with the contrast and high-pass hypotheses after an empty scan.
#[test]
// Blurred pixel values stay within 68..=188, so truncating them to u8 is exact enough.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn empty_scan_recovery_reads_blurred_small_ean13() {
    // EAN-13 4002575076905: start, LGLLGG digits, centre, R digits, end.
    let bits = b"10100011010100111001001101100010010001011100101010111001010001001010000111010011100101001110101";
    let (scale, padding, sigma) = (2_usize, 40_usize, 1.4_f64);
    let width = bits.len() * scale + 2 * padding;
    let height = 120 + 2 * padding;
    let sharp: Vec<f64> = (0..width)
        .map(|x| {
            let dark =
                (padding..width - padding).contains(&x) && bits[(x - padding) / scale] == b'1';
            if dark {
                68.
            } else {
                188.
            }
        })
        .collect();
    let row: Vec<u8> = (0..width)
        .map(|x| {
            let (mut sum, mut weights) = (0., 0.);
            for d in -5_i32..=5 {
                let at = x.saturating_add_signed(d as isize).min(width - 1);
                let weight = (-f64::from(d * d) / (2. * sigma * sigma)).exp();
                sum += weight * sharp[at];
                weights += weight;
            }
            (sum / weights) as u8
        })
        .collect();
    let mut pixels = vec![188; width * height];
    for y in padding..height - padding {
        pixels[y * width..(y + 1) * width].copy_from_slice(&row);
    }
    let image = Image {
        data: &pixels,
        width,
        height,
        channels: 1,
        stride: width,
    };
    let result = scan(&mut Scanner::default(), image, ScanOptions::default(), 15).unwrap();
    let texts: Vec<_> = result
        .barcodes
        .iter()
        .map(|read| read.text.as_str())
        .collect();
    if matches!(TIER, 0 | 2) {
        assert_eq!(texts, ["4002575076905"], "tier={TIER}");
    }
}

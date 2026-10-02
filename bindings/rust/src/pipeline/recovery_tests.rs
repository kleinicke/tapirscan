use super::restoration::restore_source_image;
use super::*;

#[test]
fn region_restoration_preserves_padded_rgb_rgba_and_gray_crops() {
    for (width, height) in [(1, 1), (3, 5), (17, 9)] {
        let gray: Vec<u8> = (0..width * height)
            .map(|i| u8::try_from((i * 73 + 19) % 256).unwrap())
            .collect();
        let expected = restore_source_image(Image {
            data: &gray,
            width,
            height,
            channels: 1,
            stride: width,
        });
        for channels in [1, 3, 4] {
            let stride = (width + 4) * channels + 7;
            let offset = stride + 2 * channels;
            let mut padded = vec![173; offset + (height - 1) * stride + width * channels];
            for y in 0..height {
                for x in 0..width {
                    for c in 0..channels.min(3) {
                        padded[offset + y * stride + x * channels + c] = gray[y * width + x];
                    }
                    if channels == 4 {
                        padded[offset + y * stride + x * channels + 3] = 11;
                    }
                }
            }
            assert_eq!(
                restore_source_image(Image {
                    data: &padded[offset..],
                    width,
                    height,
                    channels,
                    stride,
                }),
                expected
            );
        }
    }
    for value in [0, 1, 127, 254, 255] {
        let data = vec![value; 21];
        assert_eq!(
            restore_source_image(Image {
                data: &data,
                width: 7,
                height: 3,
                channels: 1,
                stride: 7,
            }),
            data
        );
    }
}

#[test]
fn optional_bands_keep_two_separated_copies_but_not_two_bands_of_one_copy() {
    let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let bits = barcode_research_core::ean::encode(&digits);
    let width = 400;
    let height = 100;
    let mut pixels = vec![255; width * height];
    for y in (5..35).chain(55..85) {
        for (module, bit) in bits.iter().enumerate() {
            if *bit > 0.5 {
                pixels[y * width + 50 + module * 3..y * width + 53 + module * 3].fill(0);
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
    let mut scan = super::super::ScanResult {
        frame: barcode_research_core::frame::Frame {
            candidates: Vec::new(),
            barcodes: Vec::new(),
            reconciliation: barcode_research_core::frame::ReconciliationWork::default(),
            unfinished: false,
        },
        errors: Vec::new(),
        candidate_timings_available: false,
    };
    let reads = [(13., 6), (23., 5), (63., 4)].map(|(top, support)| {
        super::super::read::Read::primary(
            digits,
            [[50., top], [335., top], [335., top + 4.], [50., top + 4.]],
            support,
            0,
            Vec::new(),
        )
    });
    admit_primary_reads(image, &mut scan, Vec::new(), reads).unwrap();
    assert_eq!(scan.frame.barcodes.len(), 2);
    assert_eq!(scan.frame.barcodes[0].detection.support, 6);
    assert_eq!(scan.frame.barcodes[1].detection.support, 4);
}

#[test]
fn source_veto_requires_a_conflicting_symbol_at_the_same_location() {
    let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
    let other = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    let bits = barcode_research_core::ean::encode(&digits);
    let width = 400;
    let height = 40;
    let mut pixels = vec![255; width * height];
    for y in 0..height {
        for (module, bit) in bits.iter().enumerate() {
            if *bit > 0.5 {
                pixels[y * width + 50 + module * 3..y * width + 53 + module * 3].fill(0);
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
    let q = [[50., 0.], [335., 0.], [335., 39.], [50., 39.]];
    let mut sampler = barcode_research_core::fast_profile::Sampler::default();
    assert!(!source_contradiction(image, q, digits, &mut sampler).unwrap());
    assert!(source_contradiction(image, q, other, &mut sampler).unwrap());
    pixels.fill(255);
    let blank = Image {
        data: &pixels,
        width,
        height,
        channels: 1,
        stride: width,
    };
    assert!(!source_contradiction(blank, q, other, &mut sampler).unwrap());
}

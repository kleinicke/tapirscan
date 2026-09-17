use tapirscan::{
    EanAddOnPolicy, Error, Format, Formats, Image, Mode, ScanOptions, Scanner, ScannerOptions,
};

const TEXT: &str = "4006381333931";
const BITS: &str = "10100011010100111010111101111010001001011001101010100001010000101000010111010010000101100110101";

fn fixture(pair: bool) -> (Vec<u8>, usize, usize) {
    let (width, height) = (480, if pair { 420 } else { 180 });
    let mut pixels = vec![255; width * height];
    for top in if pair { vec![30, 270] } else { vec![30] } {
        for y in top..top + 120 {
            for (x, bit) in BITS.bytes().enumerate() {
                if bit == b'1' {
                    pixels[y * width + 50 + x * 4..y * width + 54 + x * 4].fill(0);
                }
            }
        }
    }
    (pixels, width, height)
}

#[test]
fn all_modes_preserve_instances_and_direct_debug_parity() {
    assert_eq!(BITS.len(), 95);
    for mode in [Mode::Low, Mode::Medium, Mode::High, Mode::VeryHigh] {
        let mut scanner = Scanner::new(ScannerOptions {
            mode,
            ..ScannerOptions::default()
        });
        for pair in [false, true] {
            let (pixels, width, height) = fixture(pair);
            let image = Image::gray(&pixels, width, height);
            let result = scanner.scan(image).unwrap();
            assert_eq!(
                result.values().collect::<Vec<_>>(),
                vec![TEXT; if pair { 2 } else { 1 }],
                "{mode:?}"
            );
            assert_eq!(result.image_size, [width, height]);
            assert_eq!(result.mode, mode);
            assert!(result.debug.is_none());
            for complete in [false, true] {
                let plain = scanner
                    .scan_with_options(
                        image,
                        ScanOptions {
                            extended_budget: complete,
                            ..ScanOptions::default()
                        },
                    )
                    .unwrap();
                let debug = scanner
                    .scan_with_options(
                        image,
                        ScanOptions {
                            debug: true,
                            extended_budget: complete,
                            ..ScanOptions::default()
                        },
                    )
                    .unwrap();
                assert_eq!(plain.unfinished, debug.unfinished);
                assert_eq!(plain.undecoded.len(), debug.undecoded.len());
                for (a, b) in plain.undecoded.iter().zip(&debug.undecoded) {
                    assert_eq!(a.polygon, b.polygon);
                    assert_eq!(a.format, b.format);
                }
                assert_eq!(plain.barcodes.len(), debug.barcodes.len());
                for (a, b) in plain.barcodes.iter().zip(&debug) {
                    assert_eq!(a.text, b.text);
                    assert_eq!(a.format, b.format);
                    assert_eq!(a.polygon, b.polygon);
                    assert_eq!(a, b);
                    assert_eq!(a.rect().map(f64::to_bits), b.rect().map(f64::to_bits));
                    let [x, y, w, h] = a.rect();
                    assert!(x >= 0. && y >= 0. && w > 0. && h > 0.);
                }
            }
        }
    }
}

#[test]
fn padded_rgb_rgba_and_rotated_pixels() {
    let (pixels, width, height) = fixture(false);
    let mut scanner = Scanner::default();
    for channels in [3, 4] {
        let stride = width * channels + 7;
        let mut rgb = vec![37; (height - 1) * stride + width * channels];
        for y in 0..height {
            for x in 0..width {
                let i = y * stride + x * channels;
                rgb[i..i + 3].fill(pixels[y * width + x]);
                if channels == 4 {
                    rgb[i + 3] = 0;
                }
            }
        }
        let image = if channels == 3 {
            Image::rgb(&rgb, width, height)
        } else {
            Image::rgba(&rgb, width, height)
        }
        .with_stride(stride);
        assert_eq!(
            scanner.scan(image).unwrap().values().collect::<Vec<_>>(),
            [TEXT]
        );
    }
    let mut rotated = vec![255; pixels.len()];
    for y in 0..height {
        for x in 0..width {
            rotated[x * height + height - 1 - y] = pixels[y * width + x];
        }
    }
    assert_eq!(
        scanner
            .scan_with_options(Image::gray(&rotated, height, width), ScanOptions::default())
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
        [TEXT]
    );
}

#[test]
fn errors_do_not_poison_scanner_and_results_own_data() {
    let mut scanner = Scanner::default();
    for image in [
        Image::gray(&[], 0, 0),
        Image::rgb(&[], usize::MAX, 10),
        Image::gray(&[0; 9], 3, 3).with_stride(2),
        Image::gray(&[], 3, 3),
        Image::gray(&[], 8192, 8192),
        Image::rgba(&[], 3, 3).with_stride(usize::MAX),
    ] {
        assert!(matches!(scanner.scan(image), Err(Error::InvalidImage(_))));
    }
    let white = [255; 64 * 64];
    let image = Image::gray(&white, 64, 64);
    assert!(scanner
        .scan_with_options(
            image,
            ScanOptions {
                formats: Some(Format::QrCode.into()),
                extended_budget: true,
                ..ScanOptions::default()
            }
        )
        .unwrap()
        .barcodes
        .is_empty());
    let empty = scanner.scan(image).unwrap();
    assert!(empty.best().is_none());
    assert_eq!(empty.values().len(), 0);
    assert!(Formats::try_from(0).is_err());
    assert!(Formats::try_from(65536).is_err());
    let result = {
        let (pixels, width, height) = fixture(false);
        scanner
            .scan_with_options(Image::gray(&pixels, width, height), ScanOptions::default())
            .unwrap()
    };
    drop(scanner);
    assert_eq!(result.into_iter().next().unwrap().text, TEXT);
}

#[test]
fn formats_and_supplement_policy_are_independent_of_effort() {
    let (pixels, width, height) = fixture(false);
    let image = Image::gray(&pixels, width, height);
    let mut scanner = Scanner::new(ScannerOptions {
        formats: Formats::RETAIL,
        ean_add_on_policy: EanAddOnPolicy::Read,
        ..ScannerOptions::default()
    });
    let result = scanner.scan(image).unwrap();
    assert_eq!(result.values().collect::<Vec<_>>(), [TEXT]);
    assert!(result.best().unwrap().ean_add_on.is_none());
    let qr = scanner
        .scan_with_options(
            image,
            ScanOptions {
                formats: Some(Format::QrCode.into()),
                ..ScanOptions::default()
            },
        )
        .unwrap();
    assert!(qr.barcodes.is_empty());
    assert_eq!(scanner.options().formats, Formats::RETAIL);
    assert_eq!(
        (Format::Ean13 | Format::QrCode) | Format::Code128,
        Formats::try_from(529).unwrap()
    );
}

#[cfg(feature = "image")]
#[test]
fn image_crate_buffers_work_without_pixel_copies() {
    let (pixels, width, height) = fixture(false);
    let image = image::GrayImage::from_raw(
        u32::try_from(width).unwrap(),
        u32::try_from(height).unwrap(),
        pixels,
    )
    .unwrap();
    assert_eq!(
        tapirscan::scan_with_options(&image, ScanOptions::default())
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
        [TEXT]
    );
}

#[test]
fn one_shot_returns_owned_results_and_work_status() {
    let result = {
        let (pixels, width, height) = fixture(true);
        tapirscan::scan(Image::gray(&pixels, width, height)).unwrap()
    };
    assert_eq!(result.barcodes.len(), 2);
    assert_ne!(result.barcodes[0].polygon, result.barcodes[1].polygon);
    assert_eq!(result.best().unwrap().text, TEXT);
    let blank = [255; 64 * 64];
    assert!(
        tapirscan::scan_with_options(Image::gray(&blank, 64, 64), ScanOptions::default())
            .unwrap()
            .barcodes
            .is_empty()
    );
}

#[test]
fn metadata_absence_and_enclosing_pixel_bounds() {
    let barcode: tapirscan::Barcode = serde_json::from_value(serde_json::json!({
        "text": "example", "format": "QRCode", "support": 1,
        "polygon": [[1.2, 2.7], [5.8, 2.7], [5.8, 9.1], [1.2, 9.1]]
    }))
    .unwrap();
    assert_eq!(barcode.gs1, None);
    assert_eq!(barcode.reader_initialization, None);
    assert_eq!(
        barcode.rect().map(f64::to_bits),
        [1.0_f64, 2.0, 5.0, 8.0].map(f64::to_bits)
    );
    let known: tapirscan::Barcode = serde_json::from_value(serde_json::json!({
        "text": "example", "format": "QRCode", "support": 1,
        "gs1": false, "readerInitialization": true,
        "polygon": [[1.0, 2.0], [5.0, 2.0], [5.0, 9.0], [1.0, 9.0]]
    }))
    .unwrap();
    assert_eq!(known.gs1, Some(false));
    assert_eq!(known.reader_initialization, Some(true));
}

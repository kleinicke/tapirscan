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
fn default_formats_are_retail() {
    assert_eq!(ScannerOptions::default().formats, Formats::RETAIL);
}

/// Modes compiled into this test build: all four by default, fewer with
/// `--no-default-features --features mode-...`.
fn compiled_modes() -> Vec<Mode> {
    [
        (Mode::Low, cfg!(feature = "mode-low")),
        (Mode::Medium, cfg!(feature = "mode-medium")),
        (Mode::High, cfg!(feature = "mode-high")),
        (Mode::VeryHigh, cfg!(feature = "mode-very-high")),
    ]
    .into_iter()
    .filter_map(|(mode, compiled)| compiled.then_some(mode))
    .collect()
}

#[test]
fn modes_left_out_of_the_build_fail_clearly() {
    let (pixels, width, height) = fixture(false);
    for mode in [Mode::Low, Mode::Medium, Mode::High, Mode::VeryHigh] {
        let result = Scanner::new(ScannerOptions {
            mode,
            ..ScannerOptions::default()
        })
        .scan(Image::gray(&pixels, width, height));
        if compiled_modes().contains(&mode) {
            assert!(result.is_ok(), "{mode:?}");
        } else {
            assert!(matches!(result, Err(Error::InvalidOptions(_))), "{mode:?}");
        }
    }
}

#[test]
fn all_modes_preserve_instances_and_inspection_parity() {
    assert_eq!(BITS.len(), 95);
    for mode in compiled_modes() {
        let mut scanner = Scanner::new(ScannerOptions {
            mode,
            ..ScannerOptions::default()
        });
        for pair in [false, true] {
            let (pixels, width, height) = fixture(pair);
            let image = Image::gray(&pixels, width, height);
            let result = scanner.scan(image).unwrap();
            assert_eq!(
                result
                    .barcodes
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>(),
                vec![TEXT; if pair { 2 } else { 1 }],
                "{mode:?}"
            );
            let report = scanner.inspect(image).unwrap();
            assert_eq!(report.image_size, [width, height]);
            assert_eq!(report.mode, mode);
            assert!(report.diagnostics.is_some());
            let plain = scanner.scan(image).unwrap();
            assert_eq!(plain.barcodes, report.barcodes);
            assert_eq!(
                plain.values().collect::<Vec<_>>(),
                report.values().collect::<Vec<_>>()
            );
            assert_eq!(plain.best(), report.best());
            for (a, b) in plain.barcodes.iter().zip(&report.barcodes) {
                assert_eq!(a.text, b.text);
                assert_eq!(a.format, b.format);
                assert_eq!(a.polygon, b.polygon);
                assert_eq!(a, b);
                assert_eq!(a.rect(), b.rect());
                let rect = a.rect();
                assert!(rect.left >= 0 && rect.top >= 0 && rect.width > 0 && rect.height > 0);
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
            scanner
                .scan(image)
                .unwrap()
                .barcodes
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>(),
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
            .barcodes
            .iter()
            .map(|b| b.text.as_str())
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
    assert!(
        scanner
            .scan_with_options(
                image,
                ScanOptions {
                    formats: Some(Format::QrCode.into()),
                }
            )
            .unwrap()
            .barcodes
            .is_empty(),
        "expected no barcodes"
    );
    let empty = scanner.scan(image).unwrap();
    assert!(empty.barcodes.is_empty(), "{:?}", empty.barcodes);
    assert_eq!(empty.barcodes.len(), 0);
    assert!(Formats::try_from(0).is_err());
    assert!(Formats::try_from(65536).is_err());
    let result = {
        let (pixels, width, height) = fixture(false);
        scanner
            .scan_with_options(Image::gray(&pixels, width, height), ScanOptions::default())
            .unwrap()
    };
    drop(scanner);
    assert_eq!(result.barcodes.into_iter().next().unwrap().text, TEXT);
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
    assert_eq!(
        result
            .barcodes
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>(),
        [TEXT]
    );
    assert!(result.barcodes.first().unwrap().ean_add_on.is_none());
    let qr = scanner
        .scan_with_options(
            image,
            ScanOptions {
                formats: Some(Format::QrCode.into()),
            },
        )
        .unwrap();
    assert!(qr.barcodes.is_empty(), "{:?}", qr.barcodes);
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
            .barcodes
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>(),
        [TEXT]
    );
    // A decoded DynamicImage in an 8-bit format is borrowed, not copied.
    let dynamic = image::DynamicImage::ImageLuma8(image);
    let borrowed = Image::try_from(&dynamic).unwrap();
    assert_eq!(
        tapirscan::scan(borrowed)
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
        [TEXT]
    );
    let rgb = image::DynamicImage::ImageRgb8(dynamic.to_rgb8());
    assert_eq!(
        tapirscan::scan(Image::try_from(&rgb).unwrap())
            .unwrap()
            .values()
            .collect::<Vec<_>>(),
        [TEXT]
    );
    let deep = image::DynamicImage::ImageLuma16(dynamic.to_luma16());
    assert!(matches!(
        Image::try_from(&deep),
        Err(Error::InvalidImage(_))
    ));
}

#[test]
fn one_shot_returns_owned_results_and_work_status() {
    let result = {
        let (pixels, width, height) = fixture(true);
        tapirscan::scan(Image::gray(&pixels, width, height)).unwrap()
    };
    assert_eq!(result.barcodes.len(), 2);
    assert_ne!(result.barcodes[0].polygon, result.barcodes[1].polygon);
    assert_eq!(result.barcodes.first().unwrap().text, TEXT);
    let blank = [255; 64 * 64];
    assert!(
        tapirscan::scan_with_options(Image::gray(&blank, 64, 64), ScanOptions::default())
            .unwrap()
            .barcodes
            .is_empty(),
        "expected no barcodes"
    );
}

#[test]
fn format_operators_combine_in_either_order() {
    use tapirscan::{Format, Formats};
    assert_eq!(
        Format::QrCode | Formats::RETAIL,
        Formats::RETAIL | Format::QrCode
    );
}

#[test]
fn images_above_the_working_size_report_source_coordinates() {
    // 20× the fixture is 34.6 MP, above the 32 MP working size.
    let (small, small_width, small_height) = fixture(false);
    let (width, height) = (small_width * 20, small_height * 20);
    let pixels: Vec<u8> = (0..height)
        .flat_map(|y| {
            let row = &small[y / 20 * small_width..][..small_width];
            (0..width).map(move |x| row[x / 20])
        })
        .collect();
    let result = tapirscan::inspect(Image::gray(&pixels, width, height)).unwrap();
    assert_eq!(result.image_size, [width, height]);
    assert_eq!(result.values().collect::<Vec<_>>(), [TEXT]);
    // The bars span x 1000..8600 and y 600..3000 in the supplied image; the
    // polygon follows the symbol's width and lies within its bar height.
    let rect = result.barcodes[0].rect();
    assert!((rect.left - 1000).abs() <= 40, "{rect:?}");
    assert!((rect.left + rect.width - 8600).abs() <= 40, "{rect:?}");
    assert!(
        rect.top >= 560 && rect.top + rect.height <= 3040,
        "{rect:?}"
    );
    assert!(rect.height >= 1200, "{rect:?}");
    let raw = &result.diagnostics.as_ref().unwrap().raw;
    let scanned = raw["scannedImageSize"].as_array().unwrap();
    assert!(scanned[0].as_u64().unwrap() * scanned[1].as_u64().unwrap() <= 32 << 20);
    // Rows delivered in uneven chunks give the same report as the whole image.
    let mut rows = tapirscan::RowInput::new(width, height, 1).unwrap().unwrap();
    for chunk in pixels.chunks(width * 777) {
        rows.push_rows(chunk).unwrap();
    }
    assert_eq!(rows.remaining_rows(), 0);
    let streamed = Scanner::default()
        .inspect_rows(rows, ScanOptions::default(), true)
        .unwrap();
    assert_eq!(streamed.barcodes, result.barcodes);
    assert_eq!(
        format!("{:?}", streamed.undecoded),
        format!("{:?}", result.undecoded)
    );
    assert_eq!(streamed.image_size, result.image_size);
    assert_eq!(
        streamed.diagnostics.unwrap().raw["scannedImageSize"],
        raw["scannedImageSize"]
    );
    // Images that fit are scanned whole; incomplete or excess rows are rejected.
    assert!(tapirscan::RowInput::new(4000, 3000, 4).unwrap().is_none());
    assert!(tapirscan::RowInput::new(width, height, 2).is_err());
    let mut partial = tapirscan::RowInput::new(width, height, 1).unwrap().unwrap();
    assert!(partial.push_rows(&pixels[..=width]).is_err());
    partial.push_rows(&pixels[..width]).unwrap();
    assert!(
        Scanner::default()
            .inspect_rows(partial, ScanOptions::default(), false)
            .is_err()
    );
}

#[test]
fn formats_display_their_schema_names() {
    use tapirscan::Format;
    for format in Format::ALL {
        assert_eq!(format.to_string(), format.as_str());
    }
    assert_eq!(Format::QrCode.to_string(), "QRCode");
}

#[test]
fn caller_built_extreme_geometry_saturates() {
    let barcode: tapirscan::Barcode = serde_json::from_value(serde_json::json!({
        "text": "example", "format": "QRCode", "support": 1,
        "polygon": [[-3e9, -3e9], [3e9, -3e9], [3e9, 3e9], [-3e9, 3e9]]
    }))
    .unwrap();
    let rect = barcode.rect();
    assert_eq!((rect.left, rect.top), (i32::MIN, i32::MIN));
    assert_eq!((rect.width, rect.height), (i32::MAX, i32::MAX));
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
        barcode.rect(),
        tapirscan::Rect {
            left: 1,
            top: 2,
            width: 5,
            height: 8
        }
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

#[test]
fn best_selects_first_highest_support() {
    let (pixels, width, height) = fixture(true);
    let image = Image::gray(&pixels, width, height);
    let mut result = tapirscan::scan(image).unwrap();
    let inspected = tapirscan::inspect(image).unwrap();
    assert_eq!(result.best(), inspected.best());
    result.barcodes[0].support = 7;
    result.barcodes[1].support = 7;
    // Ties keep the first read.
    assert!(std::ptr::eq(
        result.best().unwrap(),
        &raw const result.barcodes[0]
    ));
    result.barcodes[1].support = 8;
    assert!(std::ptr::eq(
        result.best().unwrap(),
        &raw const result.barcodes[1]
    ));
    result.barcodes.clear();
    assert!(result.best().is_none());
}

#[test]
fn barcode_serialization_round_trips_and_omits_absent_metadata() {
    let json = serde_json::json!({
        "text": "example", "format": "QRCode", "support": 1,
        "polygon": [[1.0, 2.0], [5.0, 2.0], [5.0, 9.0], [1.0, 9.0]],
        "bytes": [0, 255], "gs1": false,
        "structuredAppend": {"index": 1, "count": 2, "id": null, "parity": 7}
    });
    let barcode: tapirscan::Barcode = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&barcode).unwrap(), json);
}

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

/// Every polygon in reduced-scan diagnostics lies in the supplied image, the raw
/// read matches the public barcode and full-frame windows span the supplied size.
fn assert_diagnostics_in_source_coordinates(
    raw: &serde_json::Value,
    barcode: &tapirscan::Barcode,
    [width, height]: [usize; 2],
) {
    fn polygons<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        match value {
            serde_json::Value::Array(items) => items.iter().for_each(|item| polygons(item, out)),
            serde_json::Value::Object(object) => {
                for (key, item) in object {
                    match key.as_str() {
                        "polygon" | "coverage" => out.push(item),
                        "frame" => {}
                        _ => polygons(item, out),
                    }
                }
            }
            _ => {}
        }
    }
    #[allow(clippy::cast_precision_loss)] // test image sizes are far below 2^52
    let (width, height) = (width as f64, height as f64);
    let points = |polygon: &serde_json::Value| -> Vec<[f64; 2]> {
        polygon
            .as_array()
            .unwrap()
            .iter()
            .map(|point| [point[0].as_f64().unwrap(), point[1].as_f64().unwrap()])
            .collect()
    };
    let mut all = Vec::new();
    polygons(raw, &mut all);
    let proposals = raw["localization"]["proposals"].as_array().unwrap();
    assert!(!proposals.is_empty(), "expected localization proposals");
    for [x, y] in all.into_iter().flat_map(points) {
        assert!((-1.0..=width + 1.0).contains(&x), "x {x} outside {width}");
        assert!((-1.0..=height + 1.0).contains(&y), "y {y} outside {height}");
    }
    let read = raw["scan"]["barcodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|read| read["text"] == barcode.text.as_str())
        .unwrap();
    for (a, b) in points(&read["polygon"]).iter().zip(barcode.polygon) {
        assert!(
            (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6,
            "{a:?} {b:?}"
        );
    }
    for window in raw["searchWindows"].as_array().into_iter().flatten() {
        if window["kind"] == "full_frame_search" {
            let corners = points(&window["polygon"]);
            assert_eq!(corners[2], [width, height], "{corners:?}");
        }
    }
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
    assert_diagnostics_in_source_coordinates(raw, &result.barcodes[0], [width, height]);
    // Rows delivered in uneven chunks give the same report as the whole image.
    let delivery = std::time::Instant::now();
    let mut rows = tapirscan::RowInput::new(width, height, 1).unwrap().unwrap();
    for chunk in pixels.chunks(width * 777) {
        rows.push_rows(chunk).unwrap();
        // The caller's time between pushes is not scan time.
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(rows.remaining_rows(), 0);
    let streamed = Scanner::default()
        .inspect_rows(rows, ScanOptions::default(), true)
        .unwrap();
    let waited = std::time::Duration::from_millis(100) * height.div_ceil(777).try_into().unwrap();
    assert!(
        streamed.elapsed + waited <= delivery.elapsed(),
        "{:?} includes the delivery",
        streamed.elapsed
    );
    assert_eq!(streamed.barcodes, result.barcodes);
    assert_eq!(
        format!("{:?}", streamed.undecoded),
        format!("{:?}", result.undecoded)
    );
    assert_eq!(streamed.image_size, result.image_size);
    let streamed_raw = streamed.diagnostics.unwrap().raw;
    assert_eq!(streamed_raw["scannedImageSize"], raw["scannedImageSize"]);
    assert_diagnostics_in_source_coordinates(&streamed_raw, &result.barcodes[0], [width, height]);
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

/// Code 39 modules for `0CSSBD` (Code 32 of 012345676) and `-02532876` (PZN).
const CODE32_BITS: &str = "1001011011010101001101101011011010010101011010110010101101011001010110100101101010110010110100101101101";
const PZN_BITS: &str = "1001011011010100101011011010100110110101011001010110110100110101011011001010101011001010110110100101101010100101101101011001101010100101101101";

fn code39_image(bits: &str) -> (Vec<u8>, usize, usize) {
    let (width, height) = (bits.len() * 3 + 120, 90);
    let mut pixels = vec![255; width * height];
    for y in 15..75 {
        for (x, bit) in bits.bytes().enumerate() {
            if bit == b'1' {
                pixels[y * width + 60 + x * 3..y * width + 63 + x * 3].fill(0);
            }
        }
    }
    (pixels, width, height)
}

fn reads(formats: Formats, bits: &str) -> Vec<(Format, String, Option<Vec<u8>>)> {
    let (pixels, width, height) = code39_image(bits);
    let mut scanner = Scanner::new(ScannerOptions {
        formats,
        ..ScannerOptions::default()
    });
    let result = scanner.scan(Image::gray(&pixels, width, height)).unwrap();
    result
        .barcodes
        .into_iter()
        .map(|b| (b.format, b.text, b.payload_bytes))
        .collect()
}

#[test]
fn opt_in_pharmacy_formats_convert_code39_reads() {
    let code32 = reads(Format::Code32.into(), CODE32_BITS);
    assert_eq!(
        code32,
        [(Format::Code32, "012345676".into(), Some(b"0CSSBD".to_vec()))]
    );
    let pzn = reads(Format::Pzn.into(), PZN_BITS);
    assert_eq!(
        pzn,
        [(Format::Pzn, "02532876".into(), Some(b"-02532876".to_vec()))]
    );
    // Without the opt-in formats the same symbols stay plain Code 39 ...
    let plain = reads(Format::Code39.into(), PZN_BITS);
    assert_eq!(plain.len(), 1);
    assert_eq!(
        (plain[0].0, plain[0].1.as_str()),
        (Format::Code39, "-02532876")
    );
    // ... `ALL` does not include them, and a Code 39 read that is not a
    // pharmacy number is dropped when only the opt-in formats are selected.
    assert_eq!(reads(Formats::ALL, CODE32_BITS)[0].0, Format::Code39);
    assert_eq!(reads(Format::Code32.into(), PZN_BITS), Vec::new());
    // It is reported as an unread region of the selected format, never as Code 39.
    let (pixels, width, height) = code39_image(PZN_BITS);
    let mut scanner = Scanner::new(ScannerOptions {
        formats: Format::Code32.into(),
        ..ScannerOptions::default()
    });
    let inspection = scanner
        .inspect(Image::gray(&pixels, width, height))
        .unwrap();
    assert_eq!(inspection.barcodes.len(), 0);
    assert!(!inspection.undecoded.is_empty());
    for region in &inspection.undecoded {
        assert_eq!(region.format, Some(Format::Code32), "{region:?}");
    }
    assert_ne!(Formats::ALL | Format::Code32, Formats::ALL);
    assert_ne!(Formats::ALL | Format::Pzn, Formats::ALL);
    assert!(Formats::try_from(Format::Code32 as u32 | Format::Pzn as u32).is_ok());
}

/// MSI Plessey modules: a `1` start bit, four bits per digit and a `0` stop bit,
/// each bit three modules (`110` or `100`), then the closing bar.
fn msi_bits(text: &str) -> String {
    let bit = |one: bool| if one { "110" } else { "100" };
    let mut bits = String::from(bit(true));
    for digit in text.bytes().map(|b| b - b'0') {
        for shift in (0..4).rev() {
            bits.push_str(bit(digit & (1 << shift) != 0));
        }
    }
    bits.push_str(bit(false));
    bits.push('1');
    bits
}

#[test]
fn msi_plessey_is_read_in_every_mode_and_only_when_selected() {
    let (pixels, width, height) = code39_image(&msi_bits("80523442"));
    for mode in compiled_modes() {
        let scan = |formats: Formats| {
            let mut scanner = Scanner::new(ScannerOptions {
                mode,
                formats,
                ..ScannerOptions::default()
            });
            let result = scanner.scan(Image::gray(&pixels, width, height)).unwrap();
            result
                .barcodes
                .into_iter()
                .map(|b| (b.format, b.text))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            scan(Format::MsiPlessey.into()),
            [(Format::MsiPlessey, "80523442".to_string())],
            "{mode:?}"
        );
        assert_eq!(scan(Formats::ALL), Vec::new(), "{mode:?}");
    }
    assert_ne!(Formats::ALL | Format::MsiPlessey, Formats::ALL);
    assert_eq!(Format::MsiPlessey.as_str(), "MSIPlessey");
}

/// A 16x16 Data Matrix ("TAPIRSCAN-DM-42") drawn at 80 pixels per module, low
/// contrast and heavily blurred, as in close-up phone photos. At this scale the
/// module-scale localizer finds nothing; High and Very High read it on a reduced copy.
#[test]
fn large_blurred_data_matrix_reads_from_high_up() {
    const ROWS: [&str; 16] = [
        "1010101010101010",
        "1111010001001111",
        "1110010101000000",
        "1000010100110011",
        "1001010101000100",
        "1100101100010011",
        "1010010111010010",
        "1100110110100101",
        "1000000011001000",
        "1100110000010111",
        "1110100010011000",
        "1100011100110001",
        "1010110111001110",
        "1111100111000101",
        "1011000000100010",
        "1111111111111111",
    ];
    const MODULE: usize = 80;
    const RADIUS: usize = 30;
    let size = 22 * MODULE;
    let mut plane = vec![255_f32; size * size];
    for y in 0..16 * MODULE {
        for x in 0..16 * MODULE {
            let dark = ROWS[y / MODULE].as_bytes()[x / MODULE] == b'1';
            plane[(y + 3 * MODULE) * size + x + 3 * MODULE] = if dark { 60. } else { 190. };
        }
    }
    for _ in 0..2 {
        for vertical in [false, true] {
            let source = plane.clone();
            for line in 0..size {
                for at in 0..size {
                    let (lo, hi) = (at.saturating_sub(RADIUS), (at + RADIUS).min(size - 1));
                    let sum: f32 = (lo..=hi)
                        .map(|i| {
                            if vertical {
                                source[i * size + line]
                            } else {
                                source[line * size + i]
                            }
                        })
                        .sum();
                    let value = sum / f32::from(u16::try_from(hi - lo + 1).unwrap());
                    if vertical {
                        plane[at * size + line] = value;
                    } else {
                        plane[line * size + at] = value;
                    }
                }
            }
        }
    }
    // The blurred values stay within 60..=190, so the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pixels: Vec<u8> = plane.iter().map(|&v| v.round() as u8).collect();
    for mode in compiled_modes() {
        if matches!(mode, Mode::Low | Mode::Medium) {
            continue;
        }
        let mut scanner = Scanner::new(ScannerOptions {
            mode,
            formats: Format::DataMatrix.into(),
            ..ScannerOptions::default()
        });
        let result = scanner.scan(Image::gray(&pixels, size, size)).unwrap();
        assert_eq!(
            result
                .barcodes
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>(),
            vec!["TAPIRSCAN-DM-42"],
            "{mode:?}"
        );
    }
}

/// The 16x16 Data Matrix "TAPIRSCAN-DM-42" with a three-module quiet zone, drawn with
/// `module` pixels per module along each axis and blurred by `passes` box filters of `radius`.
fn blurred_data_matrix(
    module: [usize; 2],
    radius: usize,
    passes: usize,
) -> (Vec<u8>, usize, usize) {
    const ROWS: [&str; 16] = [
        "1010101010101010",
        "1111010001001111",
        "1110010101000000",
        "1000010100110011",
        "1001010101000100",
        "1100101100010011",
        "1010010111010010",
        "1100110110100101",
        "1000000011001000",
        "1100110000010111",
        "1110100010011000",
        "1100011100110001",
        "1010110111001110",
        "1111100111000101",
        "1011000000100010",
        "1111111111111111",
    ];
    let (width, height) = (22 * module[0], 22 * module[1]);
    let mut plane = vec![255_f32; width * height];
    for y in 0..16 * module[1] {
        for x in 0..16 * module[0] {
            let dark = ROWS[y / module[1]].as_bytes()[x / module[0]] == b'1';
            plane[(y + 3 * module[1]) * width + x + 3 * module[0]] = if dark { 60. } else { 190. };
        }
    }
    for vertical in [false, true].repeat(passes) {
        let source = plane.clone();
        let (lines, length) = if vertical {
            (width, height)
        } else {
            (height, width)
        };
        for line in 0..lines {
            for at in 0..length {
                let (lo, hi) = (at.saturating_sub(radius), (at + radius).min(length - 1));
                let sum: f32 = (lo..=hi)
                    .map(|i| {
                        if vertical {
                            source[i * width + line]
                        } else {
                            source[line * width + i]
                        }
                    })
                    .sum();
                let value = sum / f32::from(u16::try_from(hi - lo + 1).unwrap());
                if vertical {
                    plane[at * width + line] = value;
                } else {
                    plane[line * width + at] = value;
                }
            }
        }
    }
    // The blurred values stay within 60..=190, so the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pixels = plane.iter().map(|&v| v.round() as u8).collect();
    (pixels, width, height)
}

fn read_texts(mode: Mode, pixels: &[u8], width: usize, height: usize) -> Vec<String> {
    let mut scanner = Scanner::new(ScannerOptions {
        mode,
        formats: Format::DataMatrix.into(),
        ..ScannerOptions::default()
    });
    let result = scanner.scan(Image::gray(pixels, width, height)).unwrap();
    result.barcodes.iter().map(|b| b.text.clone()).collect()
}

/// Strong perspective can shorten one axis of a square symbol to about 0.57 of the
/// other (here 14 by 8 pixels per module); the first localizer pass accepts only
/// 0.65 to 1.55, so a wider retry has to read it in every mode.
#[test]
fn foreshortened_data_matrix_reads_in_every_mode() {
    let (pixels, width, height) = blurred_data_matrix([14, 8], 2, 1);
    for mode in compiled_modes() {
        assert_eq!(
            read_texts(mode, &pixels, width, height),
            vec!["TAPIRSCAN-DM-42"],
            "{mode:?}"
        );
    }
}

#[test]
fn parallel_option_leaves_results_unchanged() {
    let (pixels, width, height) = fixture(true);
    let image = Image::gray(&pixels, width, height);
    for mode in compiled_modes() {
        let reports: Vec<_> = [true, false]
            .into_iter()
            .map(|parallel| {
                let mut scanner = Scanner::new(ScannerOptions {
                    mode,
                    formats: Formats::ALL,
                    parallel,
                    ..ScannerOptions::default()
                });
                assert_eq!(scanner.options().parallel, parallel);
                scanner.scan(image).unwrap().barcodes
            })
            .collect();
        assert_eq!(reports[0], reports[1], "{mode:?}");
        assert_eq!(reports[0].len(), 2, "{mode:?}");
    }
}

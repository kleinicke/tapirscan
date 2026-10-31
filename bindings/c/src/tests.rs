use super::*;
use std::ffi::CStr;

const L: [&str; 10] = [
    "0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011",
    "0110111", "0001011",
];
const PARITY: [&str; 10] = [
    "LLLLLL", "LLGLGG", "LLGGLG", "LLGGGL", "LGLLGG", "LGGLLG", "LGGGLL", "LGLGLG", "LGLGGL",
    "LGGLGL",
];

/// Render a clean, horizontal EAN-13 with three-pixel modules.
fn ean13(text: &str) -> (Vec<u8>, u64, u64) {
    let digits: Vec<usize> = text.bytes().map(|b| usize::from(b - b'0')).collect();
    let mut bits = String::from("101");
    for (i, &d) in digits[1..7].iter().enumerate() {
        let left = L[d];
        bits.push_str(&if PARITY[digits[0]].as_bytes()[i] == b'L' {
            left.to_owned()
        } else {
            // G codes are the reversed complement of L codes.
            left.chars()
                .rev()
                .map(|c| if c == '0' { '1' } else { '0' })
                .collect()
        });
    }
    bits.push_str("01010");
    for &d in &digits[7..] {
        bits.extend(L[d].chars().map(|c| if c == '0' { '1' } else { '0' }));
    }
    bits.push_str("101");
    let (width, height) = (bits.len() * 3 + 60, 80);
    let mut pixels = vec![255_u8; width * height];
    for row in pixels.chunks_mut(width).skip(10).take(60) {
        for (i, bit) in bits.bytes().enumerate() {
            if bit == b'1' {
                row[30 + i * 3..33 + i * 3].fill(0);
            }
        }
    }
    (pixels, width as u64, height as u64)
}

/// Codabar characters as bar/space/bar/space/bar/space/bar, 1 marking a wide element.
const CODABAR: [(u8, &str); 20] = [
    (b'0', "0000011"),
    (b'1', "0000110"),
    (b'2', "0001001"),
    (b'3', "1100000"),
    (b'4', "0010010"),
    (b'5', "1000010"),
    (b'6', "0100001"),
    (b'7', "0100100"),
    (b'8', "0110000"),
    (b'9', "1001000"),
    (b'-', "0001100"),
    (b'$', "0011000"),
    (b':', "1000101"),
    (b'/', "1010001"),
    (b'.', "1010100"),
    (b'+', "0010101"),
    (b'A', "0011010"),
    (b'B', "0101001"),
    (b'C', "0001011"),
    (b'D', "0001110"),
];

/// Render Codabar symbols, given with their start and stop letters, stacked in
/// 120-pixel bands; narrow elements are 2 px, wide ones 5 px.
fn codabar(symbols: &[&str]) -> (Vec<u8>, u64, u64) {
    let (width, band) = (480, 120);
    let mut pixels = vec![255_u8; width * band * symbols.len()];
    for (index, symbol) in symbols.iter().enumerate() {
        let mut x = 40;
        for character in symbol.bytes() {
            let pattern = CODABAR.iter().find(|(c, _)| *c == character).unwrap().1;
            // A narrow space separates characters.
            for (element, wide) in pattern.bytes().chain(Some(b'0')).enumerate() {
                let w = if wide == b'1' { 5 } else { 2 };
                if element % 2 == 0 && element < pattern.len() {
                    for row in pixels.chunks_mut(width).skip(index * band + 20).take(80) {
                        row[x..x + w].fill(0);
                    }
                }
                x += w;
            }
        }
    }
    (pixels, width as u64, (band * symbols.len()) as u64)
}

fn gray(pixels: &[u8], width: u64, height: u64) -> ImageC {
    ImageC {
        data: pixels.as_ptr(),
        length: pixels.len() as u64,
        width,
        height,
        channels: 1,
        stride: 0,
    }
}

unsafe fn create(options: *const ScannerOptionsC) -> u64 {
    let mut scanner = 0;
    assert_eq!(
        unsafe { tapirscan_scanner_create(options, &raw mut scanner, std::ptr::null_mut()) },
        0
    );
    scanner
}

unsafe fn scan(scanner: u64, image: &ImageC, options: *const ScanOptionsC) -> (i32, u64) {
    let mut result = 99;
    let status = unsafe {
        tapirscan_inspect(
            scanner,
            image,
            options,
            &raw mut result,
            std::ptr::null_mut(),
        )
    };
    (status, result)
}

unsafe fn info(result: u64) -> ResultInfoC {
    let mut info = ResultInfoC::default();
    assert_eq!(unsafe { tapirscan_result_info(result, &raw mut info) }, 0);
    info
}

unsafe fn json(result: u64) -> String {
    let mut length = 0;
    assert_eq!(
        unsafe { tapirscan_result_json_length(result, &raw mut length) },
        0
    );
    let length = usize::try_from(length).unwrap();
    let mut bytes = vec![0; length + 1];
    assert_eq!(
        unsafe { tapirscan_result_copy_json(result, bytes.as_mut_ptr(), length as u64) },
        BUFFER
    );
    assert_eq!(
        unsafe { tapirscan_result_copy_json(result, bytes.as_mut_ptr(), bytes.len() as u64) },
        0
    );
    assert_eq!(bytes.pop(), Some(0));
    String::from_utf8(bytes).unwrap()
}

#[test]
fn layouts_match_the_header() {
    assert_eq!(std::mem::size_of::<ScannerOptionsC>(), 16);
    assert_eq!(std::mem::size_of::<ImageC>(), 48);
    assert_eq!(std::mem::size_of::<ResultInfoC>(), 48);
    assert_eq!(std::mem::size_of::<BarcodeC>(), 136);
    assert_eq!(std::mem::size_of::<RegionC>(), 72);
    assert_eq!(std::mem::size_of::<ContentSummaryC>(), 16);
    assert_eq!(std::mem::size_of::<ContentLengthsC>(), 16);
    assert_eq!(tapirscan_abi_version(), 7);
}

fn content_field(handle: u64, position: u64, part: u32) -> String {
    let mut lengths = ContentLengthsC::default();
    assert_eq!(
        unsafe { tapirscan_content_field(handle, position, &raw mut lengths) },
        0
    );
    let length = if part == CONTENT_FIELD_NAME {
        lengths.name_length
    } else {
        lengths.value_length
    };
    let mut bytes = vec![0u8; usize::try_from(length).unwrap() + 1];
    let capacity = bytes.len() as u64;
    assert_eq!(
        unsafe { tapirscan_content_copy(handle, position, part, bytes.as_mut_ptr(), capacity - 1) },
        BUFFER
    );
    assert_eq!(
        unsafe { tapirscan_content_copy(handle, position, part, bytes.as_mut_ptr(), capacity) },
        0
    );
    assert_eq!(bytes.pop(), Some(0));
    String::from_utf8(bytes).unwrap()
}

#[test]
fn parses_content_through_owned_handles() {
    let text = b"WIFI:S:Cafe;T:WPA;P:secret;;";
    let mut handle = 99;
    let mut error = ErrorC { message: [1; 512] };
    unsafe {
        assert_eq!(
            tapirscan_parse_content(
                text.as_ptr(),
                text.len() as u64,
                -1,
                &raw mut handle,
                &raw mut error
            ),
            0
        );
        assert_eq!(error.message[0], 0);
        let mut summary = ContentSummaryC::default();
        assert_eq!(tapirscan_content_info(handle, &raw mut summary), 0);
        assert_eq!((summary.kind, summary.field_count), (7, 3));
        assert_eq!(
            std::ffi::CStr::from_ptr(tapirscan_content_kind_name(summary.kind)),
            c"WiFi"
        );
        assert_eq!(content_field(handle, 0, CONTENT_FIELD_NAME), "ssid");
        assert_eq!(content_field(handle, 2, CONTENT_FIELD_VALUE), "secret");
        let mut lengths = ContentLengthsC::default();
        assert_eq!(tapirscan_content_field(handle, 3, &raw mut lengths), ARG);
        let mut byte = 0u8;
        assert_eq!(tapirscan_content_copy(handle, 0, 2, &raw mut byte, 1), ARG);
        assert_eq!(tapirscan_content_destroy(handle), 0);
        assert_eq!(tapirscan_content_destroy(handle), HANDLE);
        assert_eq!(tapirscan_content_info(handle, &raw mut summary), HANDLE);

        // The GS1 flag selects element-string parsing; plain text has no content.
        let gs1 = b"0109506000134352";
        assert_eq!(
            tapirscan_parse_content(
                gs1.as_ptr(),
                gs1.len() as u64,
                1,
                &raw mut handle,
                std::ptr::null_mut()
            ),
            0
        );
        assert_ne!(handle, 0);
        assert_eq!(content_field(handle, 0, CONTENT_FIELD_NAME), "01");
        assert_eq!(tapirscan_content_destroy(handle), 0);
        for flag in [0, -1] {
            assert_eq!(
                tapirscan_parse_content(
                    gs1.as_ptr(),
                    gs1.len() as u64,
                    flag,
                    &raw mut handle,
                    std::ptr::null_mut()
                ),
                0
            );
            assert_eq!(handle, 0);
        }
        assert_eq!(
            tapirscan_parse_content(
                std::ptr::null(),
                0,
                0,
                &raw mut handle,
                std::ptr::null_mut()
            ),
            0
        );
        assert_eq!(handle, 0);
        let invalid = [0xff_u8, 0xfe];
        assert_eq!(
            tapirscan_parse_content(invalid.as_ptr(), 2, 0, &raw mut handle, &raw mut error),
            ARG
        );
        assert!(
            std::ffi::CStr::from_bytes_until_nul(&error.message)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("UTF-8")
        );
        assert_eq!(
            tapirscan_parse_content(
                std::ptr::null(),
                1,
                0,
                &raw mut handle,
                std::ptr::null_mut()
            ),
            ARG
        );
        assert!(tapirscan_content_kind_name(0).is_null());
        assert_eq!(
            CStr::from_ptr(tapirscan_content_kind_name(15)).to_str(),
            Ok("ISBN")
        );
        assert!(tapirscan_content_kind_name(16).is_null());
    }
}

#[test]
fn codabar_start_stop_is_a_separate_field() {
    let (pixels, width, height) = codabar(&["A123456B", "C40156D", "D7-$:/.+9A"]);
    unsafe {
        let scanner = create(&ScannerOptionsC {
            mode: 1,
            formats: Format::Codabar as u32,
            ean_add_on_policy: 0,
            parallel: PARALLEL_DEFAULT,
        });
        let (status, result) = scan(scanner, &gray(&pixels, width, height), std::ptr::null());
        assert_eq!(status, 0);
        let mut count = 0;
        assert_eq!(tapirscan_result_count(result, &raw mut count), 0);
        let mut reads = Vec::new();
        for index in 0..count {
            let mut barcode = BarcodeC::default();
            assert_eq!(tapirscan_result_barcode(result, index, &raw mut barcode), 0);
            assert_eq!(barcode.format, Format::Codabar as u32);
            let mut text = vec![0_u8; usize::try_from(barcode.text_length).unwrap() + 1];
            let capacity = text.len() as u64;
            assert_eq!(
                tapirscan_result_copy(result, index, FIELD_TEXT, text.as_mut_ptr(), capacity),
                0
            );
            text.pop();
            let mut start_stop = [0_u8; 3];
            let field = FIELD_CODABAR_START_STOP;
            assert_eq!(
                tapirscan_result_copy(result, index, field, start_stop.as_mut_ptr(), 2),
                BUFFER
            );
            assert_eq!(
                tapirscan_result_copy(result, index, field, start_stop.as_mut_ptr(), 3),
                0
            );
            reads.push((String::from_utf8(text).unwrap(), start_stop));
        }
        reads.sort();
        assert_eq!(
            reads,
            [
                ("123456".into(), *b"AB\0"),
                ("40156".into(), *b"CD\0"),
                ("7-$:/.+9".into(), *b"DA\0"),
            ]
        );
        // Inspection JSON is the engine report, whose reads carry the field too.
        let json: serde_json::Value = serde_json::from_str(&json(result)).unwrap();
        let mut letters: Vec<_> = json["scan"]["barcodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["codabarStartStop"].as_str().unwrap())
            .collect();
        letters.sort_unstable();
        assert_eq!(letters, ["AB", "CD", "DA"]);
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
    }
}

#[test]
fn decodes_with_typed_fields_and_owned_results() {
    let (pixels, width, height) = ean13("4006381333931");
    unsafe {
        let scanner = create(std::ptr::null());
        let (status, result) = scan(scanner, &gray(&pixels, width, height), std::ptr::null());
        assert_eq!(status, 0);
        // Results remain valid after their scanner is destroyed.
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), HANDLE);
        let summary = info(result);
        assert_eq!(summary.barcode_count, 1);
        assert_eq!(
            (summary.width, summary.height, summary.mode),
            (width, height, 1)
        );
        assert!(summary.elapsed_ms >= 0.0);
        let mut barcode = BarcodeC::default();
        assert_eq!(tapirscan_result_barcode(result, 0, &raw mut barcode), 0);
        assert_eq!(barcode.format, TAPIRSCAN_FORMAT_EAN13);
        assert_eq!(barcode.text_length, 13);
        assert_eq!(barcode.payload_bytes_length, ABSENT);
        assert_eq!(barcode.ean_add_on_length, ABSENT);
        assert_eq!(barcode.structured_append_count, 0);
        assert!(
            barcode
                .polygon
                .iter()
                .all(|p| p.x > 20.0 && p.x < f64::from(u32::try_from(width).unwrap()))
        );
        assert_eq!(tapirscan_result_barcode(result, 1, &raw mut barcode), ARG);
        let mut text = [0_u8; 14];
        assert_eq!(
            tapirscan_result_copy(result, 0, FIELD_TEXT, text.as_mut_ptr(), 13),
            BUFFER
        );
        assert_eq!(
            tapirscan_result_copy(result, 0, FIELD_TEXT, text.as_mut_ptr(), 14),
            0
        );
        assert_eq!(&text, b"4006381333931\0");
        for field in [
            FIELD_PAYLOAD_BYTES,
            FIELD_EAN_ADD_ON,
            FIELD_STRUCTURED_APPEND_ID,
            FIELD_CODABAR_START_STOP,
            5,
        ] {
            assert_eq!(
                tapirscan_result_copy(result, 0, field, text.as_mut_ptr(), 14),
                ARG
            );
        }
        let compact = json(result);
        assert!(compact.contains("\"4006381333931\""));
        assert!(compact.contains("\"scan\""));
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_result_destroy(result), HANDLE);
        let mut summary = ResultInfoC::default();
        assert_eq!(tapirscan_result_info(result, &raw mut summary), HANDLE);
    }
}

#[test]
fn options_select_mode_formats_and_diagnostics() {
    let (pixels, width, height) = ean13("4006381333931");
    let image = gray(&pixels, width, height);
    unsafe {
        let scanner = create(&ScannerOptionsC {
            mode: 2,
            formats: TAPIRSCAN_FORMAT_EAN8,
            ean_add_on_policy: 1,
            parallel: PARALLEL_DEFAULT,
        });
        let (status, result) = scan(scanner, &image, std::ptr::null());
        assert_eq!(status, 0);
        assert_eq!((info(result).barcode_count, info(result).mode), (0, 2));
        assert_eq!(tapirscan_result_destroy(result), 0);
        let options = ScanOptionsC {
            formats: TAPIRSCAN_FORMAT_EAN13,
        };
        let (status, result) = scan(scanner, &image, &raw const options);
        assert_eq!(status, 0);
        assert_eq!(info(result).barcode_count, 1);
        assert!(json(result).contains("\"searchWindows\""));
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
    }
}

#[test]
fn rejects_invalid_arguments_without_partial_output() {
    unsafe {
        let mut scanner = 7;
        assert_eq!(
            tapirscan_scanner_create(std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut()),
            ARG
        );
        for options in [
            ScannerOptionsC {
                mode: 4,
                formats: 15,
                ean_add_on_policy: 0,
                parallel: PARALLEL_DEFAULT,
            },
            ScannerOptionsC {
                mode: 1,
                formats: 0,
                ean_add_on_policy: 0,
                parallel: PARALLEL_DEFAULT,
            },
            ScannerOptionsC {
                mode: 1,
                formats: 1 << 15,
                ean_add_on_policy: 0,
                parallel: PARALLEL_DEFAULT,
            },
            ScannerOptionsC {
                mode: 1,
                formats: 15,
                ean_add_on_policy: 3,
                parallel: PARALLEL_DEFAULT,
            },
        ] {
            assert_eq!(
                tapirscan_scanner_create(
                    &raw const options,
                    &raw mut scanner,
                    std::ptr::null_mut()
                ),
                ARG
            );
            assert_eq!(scanner, 0);
        }
    }
}

#[test]
fn rejects_invalid_images_and_scan_options() {
    let pixels = vec![255_u8; 64 * 64];
    unsafe {
        let scanner = create(std::ptr::null());
        let valid = gray(&pixels, 64, 64);
        let invalid = [
            ImageC {
                length: 1,
                ..gray(&pixels, 64, 64)
            },
            ImageC {
                channels: 2,
                ..gray(&pixels, 64, 64)
            },
            ImageC {
                stride: 63,
                ..gray(&pixels, 64, 64)
            },
            ImageC {
                width: 2,
                ..gray(&pixels, 64, 64)
            },
            ImageC {
                data: std::ptr::null(),
                ..gray(&pixels, 64, 64)
            },
        ];
        for image in &invalid {
            assert_eq!(scan(scanner, image, std::ptr::null()), (ARG, 0));
        }
        let options = ScanOptionsC { formats: 1 << 15 };
        assert_eq!(scan(scanner, &valid, &raw const options), (ARG, 0));
        let mut result = 9;
        assert_eq!(
            tapirscan_scan(
                scanner,
                std::ptr::null(),
                std::ptr::null(),
                &raw mut result,
                std::ptr::null_mut()
            ),
            ARG
        );
        assert_eq!(result, 0);
        assert_eq!(scan(scanner + 100, &valid, std::ptr::null()), (HANDLE, 0));
        let (status, result) = scan(scanner, &valid, std::ptr::null());
        assert_eq!(status, 0);
        assert_eq!(info(result).barcode_count, 0);
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
    }
}

#[test]
fn names_and_messages_are_static_strings() {
    let name = |bits| {
        let pointer = tapirscan_format_name(bits);
        (!pointer.is_null()).then(|| unsafe { CStr::from_ptr(pointer) }.to_str().unwrap())
    };
    assert_eq!(name(TAPIRSCAN_FORMAT_QR_CODE), Some("QRCode"));
    assert_eq!(name(TAPIRSCAN_FORMAT_EAN13 | TAPIRSCAN_FORMAT_UPCA), None);
    assert_eq!(name(0), None);
    for status in 0..=6 {
        assert!(!tapirscan_status_message(status).is_null());
    }
}

#[test]
fn catches_unwinding() {
    assert_eq!(boundary(|| panic!("boundary probe")), PANIC);
}

const TAPIRSCAN_FORMAT_EAN13: u32 = Format::Ean13 as u32;
const TAPIRSCAN_FORMAT_UPCA: u32 = Format::Upca as u32;
const TAPIRSCAN_FORMAT_EAN8: u32 = Format::Ean8 as u32;
const TAPIRSCAN_FORMAT_QR_CODE: u32 = Format::QrCode as u32;

#[test]
fn typed_scans_do_not_retain_diagnostics_or_serialize_json() {
    let (pixels, width, height) = ean13("4006381333931");
    unsafe {
        let scanner = create(std::ptr::null());
        let mut result = 0;
        let status = tapirscan_scan(
            scanner,
            &gray(&pixels, width, height),
            std::ptr::null(),
            &raw mut result,
            std::ptr::null_mut(),
        );
        assert_eq!(status, 0);
        let output = output(result).unwrap();
        assert!(output.report.is_none());
        assert!(output.json.get().is_none());
        let mut count = 0;
        assert_eq!(tapirscan_result_count(result, &raw mut count), 0);
        assert_eq!(count, 1);
        let mut summary = ResultInfoC::default();
        assert_eq!(tapirscan_result_info(result, &raw mut summary), ARG);
        assert!(output.json.get().is_none());
        let compact: serde_json::Value = serde_json::from_str(&json(result)).unwrap();
        assert_eq!(compact[0]["text"], "4006381333931");
        assert!(compact.get("localization").is_none());
        assert!(output.json.get().is_some());
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
    }
}

#[test]
fn detailed_errors_are_per_call_and_cleared_on_success() {
    unsafe {
        let mut error = ErrorC { message: [0; 512] };
        let mut scanner = 0;
        let options = ScannerOptionsC {
            mode: 1,
            formats: 0,
            ean_add_on_policy: 0,
            parallel: PARALLEL_DEFAULT,
        };
        assert_eq!(
            tapirscan_scanner_create(&raw const options, &raw mut scanner, &raw mut error),
            ARG
        );
        let original = error.message;
        assert!(
            CStr::from_ptr(error.message.as_ptr().cast())
                .to_str()
                .unwrap()
                .contains("format")
        );
        std::thread::spawn(|| {
            let mut separate = ErrorC { message: [0; 512] };
            assert_eq!(
                tapirscan_scanner_create(std::ptr::null(), std::ptr::null_mut(), &raw mut separate),
                ARG
            );
        })
        .join()
        .unwrap();
        assert_eq!(error.message, original);
        assert_eq!(
            tapirscan_scanner_create(std::ptr::null(), &raw mut scanner, &raw mut error),
            0
        );
        assert!(error.message.iter().all(|b| *b == 0));
        let pixels = [255; 9];
        let image = gray(&pixels, 3, 3);
        let mut result = 0;
        assert_eq!(
            tapirscan_scan(
                scanner,
                &raw const image,
                std::ptr::null(),
                &raw mut result,
                &raw mut error
            ),
            0
        );
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
        assert_eq!(
            detailed(Some(&mut error), || Err(invalid(&"é".repeat(512)))),
            ARG
        );
        let truncated = CStr::from_ptr(error.message.as_ptr().cast())
            .to_str()
            .unwrap();
        assert_eq!(truncated.len(), 510);
    }
}

#[test]
fn barcode_json_preserves_optional_metadata() {
    let pixels = [255; 9];
    let mut result = Scanner::default().scan(Image::gray(&pixels, 3, 3)).unwrap();
    // Result types are non-exhaustive outside the crate; build one from its schema.
    let barcode: Barcode = serde_json::from_value(serde_json::json!({
        "text": "a\0b",
        "format": "QRCode",
        "polygon": [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]],
        "support": 3,
        "bytes": [0, 255],
        "eanAddOn": "12",
        "codabarStartStop": "CD",
        "gs1": false,
        "readerInitialization": true,
        "structuredAppend": { "index": 1, "count": 2, "id": "id", "parity": 7 },
    }))
    .unwrap();
    result.barcodes.push(barcode);
    let output = Output {
        barcodes: result.barcodes,
        report: None,
        json: OnceLock::new(),
    };
    let json: serde_json::Value = serde_json::from_slice(output.json()).unwrap();
    let b = &json[0];
    assert_eq!(b["text"], "a\0b");
    assert_eq!(b["format"], "QRCode");
    assert_eq!(b["bytes"], serde_json::json!([0, 255]));
    assert_eq!(b["gs1"], false);
    assert_eq!(b["readerInitialization"], true);
    assert_eq!(b["eanAddOn"], "12");
    assert_eq!(b["codabarStartStop"], "CD");
    assert_eq!(b["structuredAppend"]["parity"], 7);
    assert!(b.get("content").is_none());
}

#[test]
fn json_carries_content_for_structured_text() {
    let mut barcode: Barcode = serde_json::from_value(serde_json::json!({
        "text": "WIFI:S:Cafe;T:WPA;P:secret;;",
        "format": "QRCode",
        "polygon": [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]],
        "support": 1,
    }))
    .unwrap();
    let output = Output {
        barcodes: vec![barcode.clone()],
        report: None,
        json: OnceLock::new(),
    };
    let json: serde_json::Value = serde_json::from_slice(output.json()).unwrap();
    assert_eq!(
        json[0]["content"],
        serde_json::json!({
            "kind": "WiFi",
            "fields": [
                {"name": "ssid", "value": "Cafe"},
                {"name": "security", "value": "WPA"},
                {"name": "password", "value": "secret"},
            ]
        })
    );
    // Plain text has no content member.
    barcode.text = "plain".into();
    let output = Output {
        barcodes: vec![barcode],
        report: None,
        json: OnceLock::new(),
    };
    let json: serde_json::Value = serde_json::from_slice(output.json()).unwrap();
    assert!(json[0].get("content").is_none());
}

#[test]
fn large_backing_buffers_scan_only_the_addressed_layout() {
    // A crop view into a frame larger than the 1 GiB layout limit. Only the
    // addressed rows count toward the limit, matching the Rust API.
    let frame = vec![0_u8; (1 << 30) + 1];
    let image = ImageC {
        stride: 4096,
        ..gray(&frame, 64, 64)
    };
    unsafe {
        let scanner = create(std::ptr::null());
        let (status, result) = scan(scanner, &image, std::ptr::null());
        assert_eq!(status, 0);
        assert_eq!(tapirscan_result_destroy(result), 0);
        let mut error = ErrorC { message: [0; 512] };
        let mut result = 9;
        let oversized = ImageC {
            width: 16384,
            height: 16385,
            stride: 0,
            channels: 3,
            ..image
        };
        assert_eq!(
            tapirscan_scan(
                scanner,
                &raw const oversized,
                std::ptr::null(),
                &raw mut result,
                &raw mut error
            ),
            ARG
        );
        assert_eq!(result, 0);
        let message = CStr::from_ptr(error.message.as_ptr().cast())
            .to_str()
            .unwrap();
        assert!(message.contains("megapixels"), "{message}");
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
    }
}

#[test]
#[cfg(target_arch = "x86_64")]
#[expect(
    clippy::assertions_on_constants,
    reason = "cfg! reports the target features this test binary was compiled with, which is what it checks"
)]
fn x86_64_builds_target_x86_64_v2() {
    // scripts/build_native.py compiles for x86-64-v2; losing the flag makes scans
    // 20-30% slower without failing anything else.
    assert!(
        cfg!(target_feature = "sse4.2") && cfg!(target_feature = "popcnt"),
        "build the native library with scripts/build_native.py (-C target-cpu=x86-64-v2)"
    );
}

#[test]
fn parallel_setting_leaves_results_unchanged() {
    let (pixels, width, height) = ean13("4006381333931");
    let image = gray(&pixels, width, height);
    let reads = |parallel: u32| unsafe {
        let scanner = create(&ScannerOptionsC {
            mode: 1,
            formats: 25_329_663, // every format except the opt-in ones
            ean_add_on_policy: 0,
            parallel,
        });
        let (status, result) = scan(scanner, &image, std::ptr::null());
        assert_eq!(status, 0);
        let count = info(result).barcode_count;
        let reads: Vec<_> = (0..count)
            .map(|index| {
                let mut barcode = BarcodeC::default();
                assert_eq!(tapirscan_result_barcode(result, index, &raw mut barcode), 0);
                let polygon = barcode.polygon.map(|point| (point.x, point.y));
                (barcode.format, barcode.support, polygon)
            })
            .collect();
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
        reads
    };
    let on = reads(PARALLEL_ON);
    assert_eq!(on.len(), 1);
    assert_eq!(on, reads(PARALLEL_OFF));
    assert_eq!(on, reads(PARALLEL_DEFAULT));
    let invalid = ScannerOptionsC {
        mode: 1,
        formats: 15,
        ean_add_on_policy: 0,
        parallel: PARALLEL_ON + 1,
    };
    let mut scanner = 0;
    assert_eq!(
        unsafe {
            tapirscan_scanner_create(&raw const invalid, &raw mut scanner, std::ptr::null_mut())
        },
        ARG
    );
}

#[test]
fn highest_format_bit_roundtrips_through_scan_and_result() {
    // Zint 2.16, rMQR R7x43-M, payload A.
    let rows = [
        "1111111010101010101011101010101010101010111",
        "1000001001010001101110100000110110011000101",
        "1011101010111001011111100100001101011111111",
        "1011101001101111001110111101111011100010001",
        "1011101000100000000011101010011000010010101",
        "1000001011101110000110111111001111011010001",
        "1111111010101010101011101010101010101011111",
    ];
    let (width, height) = (165usize, 57usize);
    let mut pixels = vec![255; width * height];
    for (y, row) in rows.iter().enumerate() {
        for (x, bit) in row.bytes().enumerate() {
            if bit == b'1' {
                for dy in 0..3 {
                    for dx in 0..3 {
                        pixels[(18 + y * 3 + dy) * width + 18 + x * 3 + dx] = 0;
                    }
                }
            }
        }
    }
    unsafe {
        let scanner = create(&ScannerOptionsC {
            mode: 1,
            formats: 1 << 31,
            ean_add_on_policy: 0,
            parallel: PARALLEL_DEFAULT,
        });
        let image = gray(&pixels, width as u64, height as u64);
        let (status, result) = scan(scanner, &image, std::ptr::null());
        assert_eq!(status, 0);
        assert_eq!(info(result).barcode_count, 1);
        let mut barcode = BarcodeC::default();
        assert_eq!(tapirscan_result_barcode(result, 0, &raw mut barcode), 0);
        assert_eq!(barcode.format, 1 << 31);
        assert_eq!(tapirscan_result_destroy(result), 0);
        assert_eq!(tapirscan_scanner_destroy(scanner), 0);
    }
}

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
    assert_eq!(tapirscan_scanner_create(options, &raw mut scanner), 0);
    scanner
}

unsafe fn scan(scanner: u64, image: &ImageC, options: *const ScanOptionsC) -> (i32, u64) {
    let mut result = 99;
    let status = tapirscan_scan(scanner, image, options, &raw mut result);
    (status, result)
}

unsafe fn info(result: u64) -> ResultInfoC {
    let mut info = ResultInfoC::default();
    assert_eq!(tapirscan_result_info(result, &raw mut info), 0);
    info
}

unsafe fn json(result: u64) -> String {
    let length = usize::try_from(info(result).json_length).unwrap();
    let mut bytes = vec![0; length + 1];
    assert_eq!(
        tapirscan_result_copy_json(result, bytes.as_mut_ptr(), length as u64),
        BUFFER
    );
    assert_eq!(
        tapirscan_result_copy_json(result, bytes.as_mut_ptr(), bytes.len() as u64),
        0
    );
    assert_eq!(bytes.pop(), Some(0));
    String::from_utf8(bytes).unwrap()
}

#[test]
fn layouts_match_the_header() {
    assert_eq!(std::mem::size_of::<ImageC>(), 48);
    assert_eq!(std::mem::size_of::<ResultInfoC>(), 64);
    assert_eq!(std::mem::size_of::<BarcodeC>(), 136);
    assert_eq!(std::mem::size_of::<RegionC>(), 72);
    assert_eq!(tapirscan_abi_version(), 5);
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
        assert_eq!((summary.barcode_count, summary.best_index), (1, 0));
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
        assert!(barcode
            .polygon
            .iter()
            .all(|p| p.x > 20.0 && p.x < f64::from(u32::try_from(width).unwrap())));
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
            4,
        ] {
            assert_eq!(
                tapirscan_result_copy(result, 0, field, text.as_mut_ptr(), 14),
                ARG
            );
        }
        let compact = json(result);
        assert!(compact.contains("\"4006381333931\""));
        assert!(!compact.contains("\"searchWindows\""));
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
        });
        let (status, result) = scan(scanner, &image, std::ptr::null());
        assert_eq!(status, 0);
        assert_eq!((info(result).barcode_count, info(result).mode), (0, 2));
        assert_eq!(tapirscan_result_destroy(result), 0);
        let options = ScanOptionsC {
            formats: TAPIRSCAN_FORMAT_EAN13,
            debug: 1,
            extended_budget: 1,
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
    let pixels = vec![255_u8; 64 * 64];
    unsafe {
        let mut scanner = 7;
        assert_eq!(
            tapirscan_scanner_create(std::ptr::null(), std::ptr::null_mut()),
            ARG
        );
        for options in [
            ScannerOptionsC {
                mode: 4,
                formats: 15,
                ean_add_on_policy: 0,
            },
            ScannerOptionsC {
                mode: 1,
                formats: 0,
                ean_add_on_policy: 0,
            },
            ScannerOptionsC {
                mode: 1,
                formats: 1 << 30,
                ean_add_on_policy: 0,
            },
            ScannerOptionsC {
                mode: 1,
                formats: 15,
                ean_add_on_policy: 3,
            },
        ] {
            assert_eq!(
                tapirscan_scanner_create(&raw const options, &raw mut scanner),
                ARG
            );
            assert_eq!(scanner, 0);
        }
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
        for options in [
            ScanOptionsC {
                formats: 1 << 30,
                debug: 0,
                extended_budget: 0,
            },
            ScanOptionsC {
                formats: 0,
                debug: 2,
                extended_budget: 0,
            },
            ScanOptionsC {
                formats: 0,
                debug: 0,
                extended_budget: 2,
            },
        ] {
            assert_eq!(scan(scanner, &valid, &raw const options), (ARG, 0));
        }
        let mut result = 9;
        assert_eq!(
            tapirscan_scan(scanner, std::ptr::null(), std::ptr::null(), &raw mut result),
            ARG
        );
        assert_eq!(result, 0);
        assert_eq!(scan(scanner + 100, &valid, std::ptr::null()), (HANDLE, 0));
        let (status, result) = scan(scanner, &valid, std::ptr::null());
        assert_eq!(status, 0);
        assert_eq!(info(result).best_index, -1);
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

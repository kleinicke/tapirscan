//! Optional cross-language corpus supplied by `scripts/test_rust_package.py`.
use serde::Deserialize;
use serde_json::Value;
use tapirscan::{EanAddOnPolicy, Formats, Image, Mode, ScanOptions, Scanner, ScannerOptions};

#[derive(Deserialize)]
struct Case {
    mode: usize,
    pixels: String,
    width: usize,
    height: usize,
    channels: usize,
    stride: usize,
    formats: u32,
    addons: usize,
    complete: bool,
    expected: Value,
    undecoded: usize,
}

#[test]
#[ignore = "requires a native reference corpus; run scripts/test_rust_package.py"]
fn native_parity() {
    let cases: Vec<Case> = serde_json::from_slice(
        &std::fs::read(std::env::var("TAPIRSCAN_PARITY_CASES").unwrap()).unwrap(),
    )
    .unwrap();
    assert!(!cases.is_empty());
    for case in &cases {
        let pixels = std::fs::read(&case.pixels).unwrap();
        let image = match case.channels {
            1 => Image::gray(&pixels, case.width, case.height),
            3 => Image::rgb(&pixels, case.width, case.height),
            4 => Image::rgba(&pixels, case.width, case.height),
            _ => panic!("bad test channel count"),
        }
        .with_stride(case.stride);
        let mut scanner = Scanner::new(ScannerOptions {
            mode: [Mode::Low, Mode::Medium, Mode::High, Mode::VeryHigh][case.mode],
            formats: Formats::try_from(case.formats).unwrap(),
            ean_add_on_policy: [
                EanAddOnPolicy::Ignore,
                EanAddOnPolicy::Read,
                EanAddOnPolicy::Require,
            ][case.addons],
        });
        let result = scanner
            .scan_with_options(
                image,
                ScanOptions {
                    debug: true,
                    extended_budget: case.complete,
                    ..ScanOptions::default()
                },
            )
            .unwrap();
        let plain = scanner
            .scan_with_options(
                image,
                ScanOptions {
                    extended_budget: case.complete,
                    ..ScanOptions::default()
                },
            )
            .unwrap();
        assert!(plain.debug.is_none());
        assert_eq!(plain.unfinished, result.unfinished);
        assert_eq!(plain.undecoded.len(), case.undecoded);
        for (a, b) in plain.undecoded.iter().zip(&result.undecoded) {
            assert_eq!(a.format, b.format);
            assert_eq!(a.polygon, b.polygon);
        }
        assert_eq!(plain.barcodes, result.barcodes);
        let debug = result.debug.as_ref().unwrap();
        let mut actual = debug.raw.clone();
        let mut expected = case.expected.clone();
        remove_timings(&mut actual);
        remove_timings(&mut expected);
        assert_eq!(actual, expected, "{} mode {}", case.pixels, case.mode);
        assert_eq!(result.undecoded.len(), case.undecoded, "{}", case.pixels);
        assert_eq!(
            result.unfinished,
            case.expected["scan"]["unfinished"].as_bool().unwrap()
                || case.expected["localizationLimited"].as_bool().unwrap()
        );
        assert_eq!(
            result.barcodes.len(),
            case.expected["scan"]["barcodes"].as_array().unwrap().len()
        );
        for (barcode, raw) in result
            .barcodes
            .iter()
            .zip(case.expected["scan"]["barcodes"].as_array().unwrap())
        {
            assert_eq!(barcode.text, raw["text"]);
            assert_eq!(barcode.gs1, raw["gs1"].as_bool());
            assert_eq!(
                barcode.reader_initialization,
                raw["readerInitialization"].as_bool()
            );
            assert_eq!(
                barcode.payload_bytes,
                serde_json::from_value::<Option<Vec<u8>>>(raw["bytes"].clone()).unwrap()
            );
            assert_eq!(barcode.ean_add_on.as_deref(), raw["eanAddOn"].as_str());
            assert_eq!(
                barcode.structured_append,
                serde_json::from_value(raw["structuredAppend"].clone()).unwrap()
            );
        }
    }
    println!("{} native parity cases passed", cases.len());
}

fn remove_timings(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for key in ["elapsedMs", "extraMs", "ms"] {
                fields.remove(key);
            }
            for value in fields.values_mut() {
                remove_timings(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                remove_timings(value);
            }
        }
        _ => {}
    }
}

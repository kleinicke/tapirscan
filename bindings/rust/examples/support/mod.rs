use tapirscan::{Formats, Image, Mode, ScanOptions, Scanner, ScannerOptions};

#[allow(dead_code)] // Used by scan_options; each example compiles this module separately.
pub fn mode(name: &str) -> Mode {
    match name {
        "low" => Mode::Low,
        "medium" => Mode::Medium,
        "high" => Mode::High,
        "very-high" => Mode::VeryHigh,
        _ => panic!("unknown mode"),
    }
}

pub fn scan_legacy(
    scanner: &mut Scanner,
    image: Image<'_>,
    formats: u32,
    multiple: bool,
    include_regions: bool,
) -> serde_json::Value {
    let result = scanner
        .scan_with_options(
            image,
            ScanOptions {
                formats: Some(Formats::try_from(formats).unwrap()),
                debug: true,
                extended_budget: false,
            },
        )
        .unwrap();
    let mut raw = result.debug.as_ref().unwrap().raw.clone();
    raw["elapsedMs"] = serde_json::json!(result.elapsed.as_secs_f64() * 1000.0);
    raw["multiple"] = serde_json::json!(multiple);
    if !multiple {
        let best = result.best();
        let index =
            best.and_then(|best| result.barcodes.iter().position(|b| std::ptr::eq(b, best)));
        raw["scan"]["barcodes"] = serde_json::json!(index
            .and_then(|index| raw["scan"]["barcodes"].get(index).cloned())
            .into_iter()
            .collect::<Vec<_>>());
    }
    if include_regions {
        return raw;
    }
    serde_json::json!({
        "schemaVersion": raw["schemaVersion"],
        "mode": raw["mode"],
        "multiple": raw["multiple"],
        "elapsedMs": raw["elapsedMs"],
        "localizationLimited": raw["localizationLimited"],
        "scan": {
            "unfinished": raw["scan"]["unfinished"],
            "barcodes": raw["scan"]["barcodes"],
        },
    })
}

pub fn scanner(mode: Mode) -> Scanner {
    Scanner::new(ScannerOptions {
        mode,
        ..ScannerOptions::default()
    })
}

#[allow(dead_code)] // Used by scan_raw; each example compiles this module separately.
pub fn compiled_mode() -> Mode {
    if cfg!(feature = "mode-low") {
        Mode::Low
    } else if cfg!(feature = "mode-high") {
        Mode::High
    } else if cfg!(feature = "mode-very-high") {
        Mode::VeryHigh
    } else {
        Mode::Medium
    }
}

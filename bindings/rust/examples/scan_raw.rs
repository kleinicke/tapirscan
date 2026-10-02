//! Scan a raw pixel file and print the typed result as JSON.
//! usage: `scan_raw mode width height channels stride pixels [debug formats]`
use serde_json::json;
use tapirscan::{Formats, Image, Mode, ScanOptions, Scanner, ScannerOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 6 && args.len() != 8 {
        return Err(
            "usage: scan_raw mode width height channels stride pixels [debug formats]".into(),
        );
    }
    let mode = match args[0].as_str() {
        "low" => Mode::Low,
        "high" => Mode::High,
        "very-high" => Mode::VeryHigh,
        _ => Mode::Medium,
    };
    let data = std::fs::read(&args[5])?;
    let (width, height) = (args[1].parse()?, args[2].parse()?);
    let image = match args[3].as_str() {
        "1" => Image::gray(&data, width, height),
        "3" => Image::rgb(&data, width, height),
        _ => Image::rgba(&data, width, height),
    }
    .with_stride(args[4].parse()?);
    let mut options = ScanOptions::default();
    if args.len() == 8 {
        options.formats = Some(Formats::try_from(args[7].parse::<u32>()?)?);
    }
    let mut scanner = Scanner::new(ScannerOptions {
        mode,
        ..ScannerOptions::default()
    });
    let result = scanner.inspect_with_options(image, options)?;
    assert_eq!(scanner.scan_with_options(image, options)?, result.barcodes);
    let best = result
        .best()
        .and_then(|best| result.barcodes.iter().position(|b| std::ptr::eq(b, best)));
    println!(
        "{}",
        json!({
            "mode": result.mode.as_str(),
            "unfinished": result.unfinished,
            "best": best,
            "barcodes": result.barcodes.iter().map(|b| json!({
                "text": b.text,
                "format": b.format.as_str(),
                "support": b.support,
                "polygon": b.polygon,
            })).collect::<Vec<_>>(),
            "undecoded": result.undecoded.iter().map(|r| json!({
                "format": r.format.map_or("Unknown", tapirscan::Format::as_str),
                "polygon": r.polygon,
            })).collect::<Vec<_>>(),
            "debug": if args.get(6).is_some_and(|v| v == "1") { result.diagnostics.map(|d| d.raw) } else { None },
        })
    );
    Ok(())
}

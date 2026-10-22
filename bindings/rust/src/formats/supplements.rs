//! EAN-2/EAN-5 supplements for the `read` and `require` policies.
//!
//! Supplements concern retail reads only. The scan itself runs exactly as with the
//! `ignore` policy, so every other format reads the same with or without them; this
//! stage then reads the supplement beside every EAN/UPC result from grayscale
//! profiles across the decoded symbol (`barcode_multiformat::addon_profile`) and,
//! under `require`, withholds retail results without one.

use super::{EanAddOnPolicy, POLICY};
use crate::{Image, ScanOptions};
use barcode_multiformat::numeric::f64_f32;
use scanner_types::{EngineScan, UndecodedRegion};
use serde_json::{Value, json};

fn is_retail(format: &str) -> bool {
    matches!(format, "EAN13" | "UPCA" | "EAN8" | "UPCE")
}

/// Attach supplements to the retail results of `scan`, a multiple-result scan under
/// the `ignore` policy, then apply `policy` and the caller's single-result option.
pub(super) fn apply(
    image: Image<'_>,
    options: ScanOptions,
    policy: EanAddOnPolicy,
    scan: &mut EngineScan,
) {
    let start = crate::timer::Timer::start();
    let pixel =
        |x: usize, y: usize| f64_f32(image.fixed_luminance(y * image.stride + x * image.channels));
    for barcode in scan
        .barcodes
        .iter_mut()
        .filter(|b| is_retail(b.format.as_str()))
    {
        let polygon = barcode.polygon.map(|point| point.map(f64_f32));
        barcode.ean_add_on = barcode_multiformat::addon_profile::read(
            pixel,
            image.width,
            image.height,
            &polygon,
            barcode.format.as_str(),
            &barcode.text,
            POLICY.supplement_profiles,
        );
    }
    let mut withheld = Vec::new();
    if policy == EanAddOnPolicy::Require {
        scan.barcodes.retain(|barcode| {
            let keep = !is_retail(barcode.format.as_str()) || barcode.ean_add_on.is_some();
            if !keep {
                withheld.push((barcode.format, barcode.text.clone(), barcode.polygon));
            }
            keep
        });
    }
    // One result was asked for: the best one; the others become undecoded regions.
    let mut demoted = Vec::new();
    if !options.multiple && scan.barcodes.len() > 1 {
        demoted = scan.barcodes.split_off(1);
    }
    if options.include_regions {
        scan.undecoded.extend(
            withheld
                .iter()
                .map(|&(format, _, polygon)| (format, polygon))
                .chain(demoted.iter().map(|b| (b.format, b.polygon)))
                .map(|(format, polygon)| UndecodedRegion {
                    format: Some(format),
                    polygon,
                }),
        );
    }
    if let Some(raw) = &mut scan.diagnostics {
        update_diagnostics(
            raw,
            scan.barcodes.as_slice(),
            options,
            start.elapsed().as_secs_f64() * 1000.0,
        );
    }
}

/// Mirror the supplements and the kept results in the raw diagnostics.
fn update_diagnostics(
    raw: &mut Value,
    barcodes: &[scanner_types::Barcode],
    options: ScanOptions,
    elapsed_ms: f64,
) {
    if let Some(reads) = raw["scan"]["barcodes"].as_array_mut() {
        let mut kept = Vec::new();
        let mut used = vec![false; barcodes.len()];
        for read in reads.drain(..) {
            let found = barcodes.iter().enumerate().position(|(i, b)| {
                !used[i]
                    && read["format"].as_str() == Some(b.format.as_str())
                    && read["text"].as_str() == Some(b.text.as_str())
            });
            if let Some(i) = found {
                used[i] = true;
                let mut read = read;
                if let Some(object) = read.as_object_mut() {
                    match &barcodes[i].ean_add_on {
                        Some(addon) => object.insert("eanAddOn".into(), json!(addon)),
                        None => object.remove("eanAddOn"),
                    };
                }
                kept.push(read);
            }
        }
        for (rank, read) in kept.iter_mut().enumerate() {
            if read.get("rank").is_some() {
                read["rank"] = json!(rank + 1);
            }
        }
        *reads = kept;
    }
    raw["multiple"] = json!(options.multiple);
    if let Some(total) = raw["elapsedMs"].as_f64() {
        raw["elapsedMs"] = json!(total + elapsed_ms);
    }
}

//! EAN-2/EAN-5 supplements for the `read` and `require` policies.
//!
//! Supplements concern retail reads only. The scan itself runs exactly as with the
//! `ignore` policy, so every other format reads the same with or without them; this
//! stage then reads the supplement beside every EAN/UPC result from grayscale
//! profiles across the decoded symbol (`barcode_multiformat::addon_profile`) and,
//! under `require`, withholds retail results without one. Where bars beside a result
//! fit a supplement digit but no value was confirmed, a retail-only run-length sweep
//! with supplement decoding runs once over the frame and may supply it.

use super::{EanAddOnPolicy, POLICY, gray_image, scan_reader};
use crate::{Error, Image, ScanOptions, geometry::overlap_quads};
use barcode_multiformat::formats::{EAN8, EAN13, UPCA, UPCE};
use barcode_multiformat::numeric::f64_f32;
use scanner_types::{EngineScan, UndecodedRegion};
use serde_json::{Value, json};

/// The engine bit of a retail format name, or `None` for other formats.
fn retail_bit(format: &str) -> Option<u32> {
    match format {
        "EAN13" => Some(EAN13),
        "UPCA" => Some(UPCA),
        "EAN8" => Some(EAN8),
        "UPCE" => Some(UPCE),
        _ => None,
    }
}

/// Attach supplements to the retail results of `scan`, a multiple-result scan under
/// the `ignore` policy, then apply `policy` and the caller's single-result option.
pub(super) fn apply(
    image: Image<'_>,
    options: ScanOptions,
    policy: EanAddOnPolicy,
    scan: &mut EngineScan,
    gray: &mut Vec<u8>,
) -> Result<(), Error> {
    let start = crate::timer::Timer::start();
    let pixel =
        |x: usize, y: usize| f64_f32(image.fixed_luminance(y * image.stride + x * image.channels));
    let mut evidence = 0;
    for barcode in &mut scan.barcodes {
        let Some(bit) = retail_bit(barcode.format.as_str()) else {
            continue;
        };
        let polygon = barcode.polygon.map(|point| point.map(f64_f32));
        let (addon, promising) = barcode_multiformat::addon_profile::read_with_evidence(
            pixel,
            image.width,
            image.height,
            &polygon,
            barcode.format.as_str(),
            &barcode.text,
            POLICY.supplement_profiles,
        );
        if addon.is_none() && promising {
            evidence |= bit;
        }
        barcode.ean_add_on = addon;
    }
    if evidence != 0 {
        sweep(image, scan, evidence, gray)?;
    }
    let mut withheld = Vec::new();
    if policy == EanAddOnPolicy::Require {
        scan.barcodes.retain(|barcode| {
            let keep =
                retail_bit(barcode.format.as_str()).is_none() || barcode.ean_add_on.is_some();
            if !keep {
                withheld.push((barcode.format, barcode.polygon));
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
                .into_iter()
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
    Ok(())
}

/// Supplements that a run-length sweep of the retail `formats` confirms on two or
/// more scan lines, for the same physical results still without one.
fn sweep(
    image: Image<'_>,
    scan: &mut EngineScan,
    formats: u32,
    gray: &mut Vec<u8>,
) -> Result<(), Error> {
    let pixels = if image.channels == 1 && image.stride == image.width {
        &image.data[..image.width * image.height]
    } else {
        gray_image(image, gray)?;
        gray.as_slice()
    };
    let found = scan_reader(
        pixels,
        image.width,
        image.height,
        formats | barcode_multiformat::linear::ADDON_READ,
        POLICY.supplement_sweep_effort,
    );
    for detection in found.barcodes.iter().filter(|d| d.addon.is_some()) {
        let polygon = detection.polygon.map(|point| point.map(f64::from));
        for barcode in &mut scan.barcodes {
            if barcode.ean_add_on.is_none()
                && barcode.format.as_str() == detection.format
                && barcode.text == detection.text
                && overlap_quads(&barcode.polygon, &polygon).0 >= 0.65
            {
                barcode.ean_add_on.clone_from(&detection.addon);
            }
        }
    }
    Ok(())
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

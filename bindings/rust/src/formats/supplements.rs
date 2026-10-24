//! EAN-2/EAN-5 supplements for the `read` and `require` policies.
//!
//! Supplements concern retail reads only. The scan itself runs exactly as with the
//! `ignore` policy, so every other format reads the same with or without them; this
//! stage then reads the supplement beside every EAN/UPC result from grayscale
//! profiles across the decoded symbol (`barcode_multiformat::addon_profile`) and,
//! under `require`, withholds retail results without one. Where bars beside a result
//! fit a supplement digit but no value was confirmed, a retail-only run-length sweep
//! with supplement decoding runs once over such results and the space for a
//! supplement on either side of them, and may supply it.

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
    let mut areas = Vec::new();
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
            areas.push(supplement_area(
                &barcode.polygon,
                barcode.format.as_str(),
                image.width,
                image.height,
            ));
        }
        barcode.ean_add_on = addon;
    }
    if evidence != 0 {
        sweep(image, &areas, scan, evidence, gray)?;
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

/// Modules beside a main symbol that a supplement can take: a gap of up to 16 and
/// five digits in 47, with a few modules of its quiet zone. The area searched extends
/// a result along its scan axis by this many of its modules beyond either end.
const SUPPLEMENT_MODULES: f64 = 70.;

/// Modules across a main symbol of retail `format`.
fn main_modules(format: &str) -> f64 {
    match format {
        "EAN8" => 67.,
        "UPCE" => 51.,
        _ => 95.,
    }
}

/// The pixel rectangle `[x0, y0, x1, y1]` holding a retail result and the space for
/// a supplement beyond either end of its scan axis.
fn supplement_area(
    polygon: &[[f64; 2]; 4],
    format: &str,
    width: usize,
    height: usize,
) -> [usize; 4] {
    let reach = SUPPLEMENT_MODULES / main_modules(format);
    // Retail outlines run from corner 0 to corner 1 along the scan axis, as the
    // profile reader takes them; the supplement may lie beyond either end.
    let axis = [
        (polygon[1][0] - polygon[0][0]) * reach,
        (polygon[1][1] - polygon[0][1]) * reach,
    ];
    let length = axis[0].hypot(axis[1]);
    let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for point in polygon {
        for sign in [-1., 1.] {
            for k in 0..2 {
                let value = point[k] + sign * axis[k];
                low[k] = low[k].min(value);
                high[k] = high[k].max(value);
            }
        }
    }
    // A margin across the bars keeps the quiet zone of tilted symbols.
    let pad = length / reach * 0.1 + 8.;
    let clamp = |value: f64, limit: usize| {
        barcode_research_core::numeric::f64_usize(
            value.clamp(0., barcode_research_core::numeric::usize_f64(limit)),
        )
    };
    [
        clamp((low[0] - pad).floor(), width),
        clamp((low[1] - pad).floor(), height),
        clamp((high[0] + pad).ceil(), width),
        clamp((high[1] + pad).ceil(), height),
    ]
}

/// Supplements that a run-length sweep of the retail `formats` confirms on two or
/// more scan lines, for the same physical results still without one. The sweep keeps
/// the frame's scan lines; pixels outside `areas` take the value of the first pixel,
/// whose constant background the sweep skips.
fn sweep(
    image: Image<'_>,
    areas: &[[usize; 4]],
    scan: &mut EngineScan,
    formats: u32,
    gray: &mut Vec<u8>,
) -> Result<(), Error> {
    let width = image.width;
    let mut part = Vec::new();
    gray_image(
        Image {
            width: 1,
            height: 1,
            ..image
        },
        &mut part,
    )?;
    gray.clear();
    gray.resize(width * image.height, part[0]);
    for &[x0, y0, x1, y1] in areas {
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let crop = Image {
            data: &image.data[y0 * image.stride + x0 * image.channels..],
            width: x1 - x0,
            height: y1 - y0,
            ..image
        };
        gray_image(crop, &mut part)?;
        for (row, source) in gray[y0 * width..y1 * width]
            .chunks_exact_mut(width)
            .zip(part.chunks_exact(crop.width))
        {
            row[x0..x1].copy_from_slice(source);
        }
    }
    let found = scan_reader(
        gray,
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

#[cfg(test)]
mod supplement_area_tests {
    use super::supplement_area;

    #[test]
    fn covers_the_symbol_and_a_supplement_beyond_either_end() {
        // An EAN-13 of 2 px modules (190 px), its outline from the start guard.
        let polygon = [[400., 300.], [590., 300.], [590., 400.], [400., 400.]];
        let [x0, y0, x1, y1] = supplement_area(&polygon, "EAN13", 1600, 1200);
        // 70 modules (140 px) beyond each end plus the margin of a tenth of the symbol.
        assert_eq!([x0, x1], [400 - 140 - 27, 590 + 140 + 27]);
        assert_eq!([y0, y1], [300 - 27, 400 + 27]);
        // A UPC-E of 51 modules reaches 70 of its own modules; the frame edge clips it.
        let polygon = [[10., 50.], [112., 50.], [112., 150.], [10., 150.]];
        let [x0, _, x1, _] = supplement_area(&polygon, "UPCE", 300, 200);
        assert_eq!([x0, x1], [0, 112 + 140 + 19]);
    }
}

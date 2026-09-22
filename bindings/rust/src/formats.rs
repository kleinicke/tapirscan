//! Opt-in format readers. EAN-13/UPC-A keep the selected pinned scanner path.
use crate::format_registry::{
    ALL_FORMATS_MASK, EAN_ADDON_READ_FLAG, EAN_ADDON_REQUIRE_FLAG, LINEAR_MASK,
};
use crate::{geometry::overlap, Error, Image, ScanOptions, Scanner, MODE};
use scanner_types::EngineScan;
use serde_json::{json, Value};

/// Whether a retail barcode needs its adjacent two- or five-digit supplement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EanAddOnPolicy {
    /// Decode the main value without searching for a supplement.
    #[default]
    Ignore,
    /// Attach a confirmed supplement when available; keep the main value otherwise.
    Read,
    /// Accept retail reads only when a supplement is confirmed.
    Require,
}

impl EanAddOnPolicy {
    fn engine_bits(self) -> u32 {
        match self {
            Self::Ignore => 0,
            Self::Read => EAN_ADDON_READ_FLAG,
            Self::Require => EAN_ADDON_REQUIRE_FLAG,
        }
    }
}

impl Scanner {
    /// Scan selected formats into the shared typed result and optional raw diagnostics.
    /// # Errors
    /// Rejects invalid masks, image layouts, malformed reader output, and oversized inputs.
    pub fn scan_formats_typed_with_addons(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        mask: u32,
        addons: EanAddOnPolicy,
        retain_diagnostics: bool,
    ) -> Result<EngineScan, Error> {
        validate(image, mask)?;

        if mask == 1 && addons == EanAddOnPolicy::Ignore {
            let result = self.scan_with_options(image, options)?;
            let mut value =
                crate::result::value(&result, MODE, 0.0).map_err(|_| Error::Parameters)?;
            if let Some(reads) = value["scan"]["barcodes"].as_array_mut() {
                for read in reads {
                    read["format"] = json!("EAN13");
                }
            }
            return typed_result(value, retain_diagnostics);
        }
        let start = crate::timer::Timer::start();
        // Rank only after all selected readers have finished.
        let full_options = ScanOptions {
            finish_candidates: options.finish_candidates,
            multiple: true,
            include_regions: options.include_regions,
        };
        let shared_retail =
            crate::MODE_ID == 1 && addons == EanAddOnPolicy::Ignore && mask & 12 != 0;
        let (extras, coverage) =
            scan_additional(image, if shared_retail { mask & !12 } else { mask }, addons)?;
        let mut retail = Vec::new();
        let mut value = if shared_retail || mask & 3 != 0 {
            let result =
                self.scan_with_coverage(image, full_options, &coverage, false, shared_retail)?;
            retail.clone_from(&result.retail);
            crate::result::value(&result, MODE, 0.0).map_err(|_| Error::Parameters)?
        } else {
            json!({"schemaVersion":2,"mode":MODE,"multiple":true,"elapsedMs":0.0,"localizationLimited":false,"scan":{"barcodes":[],"unfinished":false}})
        };
        let mut reads = value["scan"]["barcodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        reads.retain_mut(|b| {
            let text = b["text"].as_str().unwrap_or_default().to_owned();
            if mask & 2 != 0 && text.starts_with('0') {
                b["text"] = json!(&text[1..]);
                b["format"] = json!("UPCA");
                true
            } else {
                b["format"] = json!("EAN13");
                mask & 1 != 0
            }
        });
        reads.extend(retail.into_iter().filter(|b| match b["format"].as_str() {
            Some("EAN8") => mask & 4 != 0,
            Some("UPCE") => mask & 8 != 0,
            _ => false,
        }));
        let mut unread = if options.include_regions {
            unread_regions(&value, &reads)
        } else {
            Vec::new()
        };
        for extra in &extras {
            for b in &extra.barcodes {
                let mut b = serde_json::to_value(b).map_err(|_| Error::Parameters)?;
                b["axis"] = json!(0);
                b["candidate_indices"] = json!([]);
                reads.push(b);
            }
            if options.include_regions {
                for region in &extra.regions {
                    unread.push(serde_json::to_value(region).map_err(|_| Error::Parameters)?);
                }
            }
            value["scan"]["unfinished"] =
                json!(value["scan"]["unfinished"].as_bool().unwrap_or(false) || extra.unfinished);
        }
        if !extras.is_empty() {
            apply_supplement_policy(&mut reads, &mut unread, addons, options.include_regions);
            reads = distinct(reads);
            unread.retain(|region| !reads.iter().any(|b| overlap(region, b).1 >= 0.65));
            unread = distinct(unread);
        }
        reads = crate::linear_duplicates::merge(reads, image);
        reads.sort_by_key(|b| std::cmp::Reverse(b["support"].as_u64().unwrap_or(0)));
        for (i, b) in reads.iter_mut().enumerate() {
            b["rank"] = json!(i + 1);
        }
        if options.include_regions {
            value["scan"]["regions"] =
                json!(reads.iter().cloned().chain(unread).collect::<Vec<_>>());
        }
        if !options.multiple {
            reads.truncate(1);
        }
        value["scan"]["barcodes"] = json!(reads);
        value["scan"]["unfinished"] = json!(
            value["scan"]["unfinished"].as_bool().unwrap_or(false)
                || value["localizationLimited"].as_bool().unwrap_or(false)
        );
        value["multiple"] = json!(options.multiple);
        value["elapsedMs"] = json!(start.elapsed().as_secs_f64() * 1000.0);
        typed_result(value, retain_diagnostics)
    }
}

fn typed_result(mut raw: Value, retain_diagnostics: bool) -> Result<EngineScan, Error> {
    let mut regions = if let Some(regions) = raw["scan"]["regions"].as_array() {
        let reads = raw["scan"]["barcodes"]
            .as_array()
            .ok_or(Error::OutputShape)?;
        regions
            .iter()
            .filter(|region| !reads.contains(region))
            .cloned()
            .collect()
    } else {
        let reads = raw["scan"]["barcodes"]
            .as_array()
            .ok_or(Error::OutputShape)?;
        unread_regions(&raw, reads)
    };
    for region in &mut regions {
        if region["format"] == "Unknown" {
            region["format"] = Value::Null;
        }
    }
    let undecoded = regions
        .into_iter()
        .map(|value| serde_json::from_value(value).map_err(|_| Error::OutputShape))
        .collect::<Result<_, _>>()?;
    let unfinished = raw["scan"]["unfinished"]
        .as_bool()
        .ok_or(Error::OutputShape)?;
    let localization_limited = raw["localizationLimited"].as_bool().unwrap_or(false);
    let values = if retain_diagnostics {
        raw["scan"]["barcodes"]
            .as_array()
            .cloned()
            .ok_or(Error::OutputShape)?
    } else {
        raw["scan"]["barcodes"]
            .as_array_mut()
            .map(std::mem::take)
            .ok_or(Error::OutputShape)?
    };
    let barcodes = values
        .into_iter()
        .map(|value| serde_json::from_value(value).map_err(|_| Error::OutputShape))
        .collect::<Result<_, _>>()?;
    Ok(EngineScan {
        barcodes,
        undecoded,
        unfinished,
        localization_limited,
        diagnostics: retain_diagnostics.then_some(raw),
    })
}

fn apply_supplement_policy(
    reads: &mut Vec<Value>,
    unread: &mut Vec<Value>,
    policy: EanAddOnPolicy,
    include_regions: bool,
) {
    attach_supplements(reads);
    if policy == EanAddOnPolicy::Require {
        reads.retain(|read| {
            let accepted = !is_retail(read) || read["eanAddOn"].is_string();
            if !accepted && include_regions {
                unread.push(json!({"format":read["format"],"text":"",
                                  "polygon":read["polygon"],"support":0}));
            }
            accepted
        });
    }
}

fn is_retail(read: &Value) -> bool {
    matches!(
        read["format"].as_str(),
        Some("EAN13" | "UPCA" | "EAN8" | "UPCE")
    )
}

// Match confirmed supplements to the same physical base symbol, never text alone.
fn attach_supplements(reads: &mut [Value]) {
    let supplemental: Vec<Value> = reads
        .iter()
        .filter(|read| read["eanAddOn"].is_string())
        .cloned()
        .collect();
    for read in &supplemental {
        for base in &mut *reads {
            if base["format"] == read["format"]
                && base["text"] == read["text"]
                && !base["eanAddOn"].is_string()
                && overlap(base, read).0 >= 0.65
            {
                base["eanAddOn"] = read["eanAddOn"].clone();
            }
        }
    }
}

fn distinct(mut reads: Vec<Value>) -> Vec<Value> {
    reads.sort_by_key(|b| std::cmp::Reverse(b["support"].as_u64().unwrap_or(0)));
    let mut result: Vec<Value> = Vec::new();
    for read in reads {
        if !result.iter().any(|b| {
            ["text", "format", "eanAddOn", "structuredAppend"]
                .iter()
                .all(|key| b[key] == read[key])
                && b["readerInitialization"].as_bool().unwrap_or(false)
                    == read["readerInitialization"].as_bool().unwrap_or(false)
                && overlap(b, &read).0 >= 0.65
        }) {
            result.push(read);
        }
    }
    result
}

fn unread_regions(value: &Value, reads: &[Value]) -> Vec<Value> {
    let mut unread = Vec::new();
    let decoded: std::collections::HashSet<_> = reads
        .iter()
        .flat_map(|b| {
            b["candidate_indices"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
        })
        .collect();
    if let Some(proposals) = value["localization"]["proposals"].as_array() {
        for (i, p) in proposals.iter().enumerate() {
            if !decoded.contains(&(i as u64)) {
                unread
                    .push(json!({"format":"Unknown","text":"","polygon":p["polygon"],"support":0}));
            }
        }
    }

    if let Some(attempts) = value["recovery"]["attempts"].as_array() {
        for attempt in attempts {
            if let Some(accepted) = attempt["reads"].as_array() {
                unread.extend(unread_regions(
                    &json!({"localization":{"proposals":attempt["proposals"]}}),
                    accepted,
                ));
            }
        }
    }
    unread
}

fn gray_image(image: Image<'_>) -> Result<Vec<u8>, Error> {
    let mut gray = Vec::with_capacity(image.width * image.height);
    for y in 0..image.height {
        for x in 0..image.width {
            let i = y * image.stride + x * image.channels;
            let pixel = if image.channels == 1 {
                image.data[i]
            } else {
                let luma = (u32::from(image.data[i]) * 77
                    + u32::from(image.data[i + 1]) * 150
                    + u32::from(image.data[i + 2]) * 29
                    + 128)
                    >> 8;
                u8::try_from(luma).map_err(|_| Error::Parameters)?
            };
            gray.push(pixel);
        }
    }

    Ok(gray)
}

fn validate(image: Image<'_>, mask: u32) -> Result<(), Error> {
    if mask == 0
        || mask & !ALL_FORMATS_MASK != 0
        || image
            .width
            .checked_mul(image.height)
            .is_none_or(|n| n > 32 * 1024 * 1024)
    {
        return Err(Error::Parameters);
    }
    crate::ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    )?;
    if image.width < 3 || image.height < 3 {
        return Err(Error::Parameters);
    }

    Ok(())
}

#[cfg(not(feature = "low"))]
pub(crate) fn contains_point(point: [f64; 2], quad: &crate::Quad) -> bool {
    if !point.iter().all(|v| v.is_finite()) || !quad.iter().flatten().all(|v| v.is_finite()) {
        return false;
    }
    let (mut positive, mut negative, mut area) = (false, false, 0.0_f64);
    for i in 0..4 {
        let (a, b) = (quad[i], quad[(i + 1) % 4]);
        let cross = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        positive |= cross > 1e-6;
        negative |= cross < -1e-6;
        area += a[0] * b[1] - b[0] * a[1];
    }
    area.abs() > 1e-6 && !(positive && negative)
}

#[cfg(not(feature = "low"))]
pub(crate) fn uncovered_mask(
    proposals: &[crate::Proposal],
    coverage: &[crate::Quad],
    initial: u64,
) -> u64 {
    proposals
        .iter()
        .enumerate()
        .fold(initial, |mask, (i, proposal)| {
            if coverage.iter().any(|quad| {
                proposal
                    .polygon
                    .iter()
                    .all(|point| contains_point(*point, quad))
            }) {
                mask & !(1_u64 << i)
            } else {
                mask
            }
        })
}

/// Run the linear reader before primary discovery so strong reads can guide deep retries.
fn scan_additional(
    image: Image<'_>,
    mask: u32,
    addons: EanAddOnPolicy,
) -> Result<(Vec<barcode_multiformat::Scan>, Vec<crate::Quad>), Error> {
    let enabled = if addons == EanAddOnPolicy::Ignore {
        mask & !3
    } else {
        mask
    };
    if enabled == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let linear = enabled & LINEAR_MASK;
    let matrix = enabled & !LINEAR_MASK;
    let effort = [0, 1, 2, 2][crate::MODE_ID as usize];
    let qr_effort = [0, 1, 2, 3][crate::MODE_ID as usize];
    let gray = gray_image(image)?;
    let mut scans = Vec::new();
    let mut coverage = Vec::new();
    for (selected, level) in [
        (linear, effort),
        (matrix, if matrix & 512 != 0 { qr_effort } else { 1 }),
    ] {
        if selected == 0 {
            continue;
        }
        let scan = barcode_multiformat::scan(
            &gray,
            image.width,
            image.height,
            selected | addons.engine_bits(),
            level,
        );
        if selected == linear
            && crate::MODE_ID != 0
            && mask & 3 != 0
            && addons == EanAddOnPolicy::Ignore
        {
            coverage.extend(
                scan.barcodes
                    .iter()
                    .filter(|b| {
                        let checked = matches!(b.format.as_str(), "EAN8" | "UPCE" | "Code128")
                            && b.support >= 3
                            && b.error <= 0.08;
                        let unchecked = matches!(b.format.as_str(), "Code39" | "ITF")
                            && b.support >= 8
                            && b.error <= 0.035
                            && b.text.len() >= 8;
                        checked || unchecked
                    })
                    .map(|b| b.polygon.map(|p| p.map(f64::from))),
            );
        }
        scans.push(scan);
    }
    Ok((scans, coverage))
}

#[cfg(all(test, not(feature = "low")))]
mod coverage_tests {
    use super::{contains_point, uncovered_mask};
    use crate::Proposal;
    #[test]
    fn only_contained_proposals_lose_retries() {
        let quad = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        let adjacent = [[9., 0.], [19., 0.], [19., 10.], [9., 10.]];
        let mut proposals: Vec<_> = (0..63)
            .map(|_| Proposal {
                polygon: adjacent,
                score: 1.,
            })
            .collect();
        proposals[0].polygon = quad;
        proposals[62].polygon = quad;
        assert_eq!(
            uncovered_mask(&proposals, &[quad], u64::MAX),
            u64::MAX & !1 & !(1 << 62)
        );
        assert_eq!(uncovered_mask(&proposals, &[quad], 1 << 63), 1 << 63);
        assert!(contains_point([0., 5.], &quad));
        assert!(!contains_point([0., 0.], &[[0., 0.]; 4]));
        assert!(!contains_point([f64::NAN, 0.], &quad));
    }
}

#[cfg(test)]
mod result_tests {
    use super::typed_result;
    use serde_json::json;

    #[test]
    fn reported_regions_take_precedence_over_localization_fallback() {
        let read = json!({
            "text":"5901234123457", "format":"EAN13", "polygon":[[0.,0.],[2.,0.],[2.,1.],[0.,1.]], "support":4
        });
        let unread = json!({
            "text":"", "format":"Unknown", "polygon":[[3.,0.],[5.,0.],[5.,1.],[3.,1.]], "support":0
        });
        let raw = json!({
            "localizationLimited":false,
            "localization":{"proposals":[
                {"polygon":[[0.,0.],[2.,0.],[2.,1.],[0.,1.]]},
                {"polygon":[[3.,0.],[5.,0.],[5.,1.],[3.,1.]]},
                {"polygon":[[6.,0.],[8.,0.],[8.,1.],[6.,1.]]}
            ]},
            "scan":{"unfinished":false,"barcodes":[read.clone()],"regions":[read,unread]}
        });
        let result = typed_result(raw, false).unwrap();
        assert_eq!(result.barcodes.len(), 1);
        assert_eq!(result.undecoded.len(), 1);
        assert!((result.undecoded[0].polygon[0][0] - 3.).abs() < f64::EPSILON);
        assert!(result.undecoded[0].polygon[0][1].abs() < f64::EPSILON);
    }
}

//! Opt-in format readers. EAN-13/UPC-A keep the selected pinned scanner path.
use crate::{Error, Image, ScanOptions, Scanner, MODE};
use serde_json::{json, Value};

/// Explicit format mask; an empty or unknown mask is rejected.
pub const FORMATS: [(&str, u32); 16] = [
    ("EAN13", 1),
    ("UPCA", 2),
    ("EAN8", 4),
    ("UPCE", 8),
    ("Code128", 16),
    ("Code39", 32),
    ("ITF", 64),
    ("Codabar", 128),
    ("Code93", 256),
    ("QRCode", 512),
    ("DataMatrix", 1024),
    ("PDF417", 2048),
    ("Aztec", 4096),
    ("DataBar", 8192),
    ("DataBarExpanded", 16384),
    ("MaxiCode", 131_072),
];

impl Scanner {
    /// Scan selected formats and return the shared JSON result contract.
    /// Additional readers retain the research scanline effort (1) in every mode.
    /// # Errors
    /// Rejects invalid masks, image layouts, and inputs above 32 megapixels.
    pub fn scan_formats_json(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        mask: u32,
    ) -> Result<Value, Error> {
        validate(image, mask)?;
        if mask == 1 {
            let result = self.scan_with_options(image, options)?;
            let mut value: Value =
                serde_json::from_str(&result.to_json(MODE, 0.0)).map_err(|_| Error::Parameters)?;
            if let Some(reads) = value["scan"]["barcodes"].as_array_mut() {
                for read in reads {
                    read["format"] = json!("EAN13");
                }
            }
            return Ok(value);
        }
        let start = std::time::Instant::now();
        // Rank only after all selected readers have finished.
        let full_options = ScanOptions {
            multiple: true,
            include_regions: options.include_regions,
        };
        let mut value = if mask & 3 != 0 {
            let result = self.scan_with_options(image, full_options)?;
            serde_json::from_str(&result.to_json(MODE, 0.0)).map_err(|_| Error::Parameters)?
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
        let mut unread = if options.include_regions {
            unread_regions(&value, &reads)
        } else {
            Vec::new()
        };
        if mask & !3 != 0 {
            let gray = gray_image(image)?;
            let extra = barcode_multiformat::scan(&gray, image.width, image.height, mask & !3, 1);
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
        if mask & !3 != 0 {
            reads = distinct(reads);
            unread.retain(|region| !reads.iter().any(|b| overlap(region, b).1 >= 0.65));
            unread = distinct(unread);
        }
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
        Ok(value)
    }
}

fn quad(value: &Value) -> [[f64; 2]; 4] {
    std::array::from_fn(|i| std::array::from_fn(|j| value["polygon"][i][j].as_f64().unwrap_or(0.0)))
}
fn signed_area(points: &[[f64; 2]]) -> f64 {
    if points.is_empty() {
        return 0.0;
    }
    points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = points[(i + 1) % points.len()];
            p[0] * q[1] - q[0] * p[1]
        })
        .sum::<f64>()
        / 2.0
}
// Same convex clipping and 0.65 thresholds as the research JS reconciliation.
fn overlap(first: &Value, second: &Value) -> (f64, f64) {
    let first_quad = quad(first);
    let second_quad = quad(second);
    let first_area = signed_area(&first_quad).abs();
    let second_area = signed_area(&second_quad).abs();
    if first_area.min(second_area) < 1e-6 {
        return (0.0, 0.0);
    }
    let winding = signed_area(&second_quad).signum();
    let mut points = first_quad.to_vec();
    for edge_index in 0..4 {
        if points.is_empty() {
            break;
        }
        let edge_start = second_quad[edge_index];
        let edge_end = second_quad[(edge_index + 1) % 4];
        let side = |point: [f64; 2]| {
            winding
                * ((edge_end[0] - edge_start[0]) * (point[1] - edge_start[1])
                    - (edge_end[1] - edge_start[1]) * (point[0] - edge_start[0]))
        };
        let mut clipped = Vec::new();
        for point_index in 0..points.len() {
            let current = points[point_index];
            let next = points[(point_index + 1) % points.len()];
            let current_side = side(current);
            let next_side = side(next);
            if current_side >= 0.0 {
                clipped.push(current);
            }
            if (current_side >= 0.0) != (next_side >= 0.0) {
                let fraction = current_side / (current_side - next_side);
                clipped.push([
                    current[0] + fraction * (next[0] - current[0]),
                    current[1] + fraction * (next[1] - current[1]),
                ]);
            }
        }
        points = clipped;
    }
    let intersection = first_area.min(second_area).min(signed_area(&points).abs());
    (
        intersection / first_area.min(second_area),
        intersection / first_area,
    )
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
    let allowed = FORMATS.iter().fold(0, |acc, (_, bit)| acc | bit);
    if mask == 0
        || mask & !allowed != 0
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

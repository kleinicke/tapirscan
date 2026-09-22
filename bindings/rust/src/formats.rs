//! Opt-in format readers. EAN-13/UPC-A keep the selected pinned scanner path.
use crate::format_registry::{
    ALL_FORMATS_MASK, EAN_ADDON_READ_FLAG, EAN_ADDON_REQUIRE_FLAG, LINEAR_MASK,
};
use crate::read::{Read, Region};
use crate::{geometry::overlap_quads, Error, Image, ScanOptions, Scanner, MODE};
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
    ) -> Result<EngineScan, Error> {
        validate(image, mask)?;

        if mask == 1 && addons == EanAddOnPolicy::Ignore {
            return primary_result(&self.scan_with_options(image, options)?);
        }
        let start = crate::timer::Timer::start();
        // Rank only after all selected readers have finished.
        let full_options = ScanOptions {
            multiple: true,
            ..options
        };
        let shared_retail =
            crate::MODE_ID == 1 && addons == EanAddOnPolicy::Ignore && mask & 12 != 0;
        let (extras, coverage) =
            scan_additional(image, if shared_retail { mask & !12 } else { mask }, addons)?;
        let primary = if shared_retail || mask & 3 != 0 {
            Some(self.scan_with_coverage(image, full_options, &coverage, false, shared_retail)?)
        } else {
            None
        };
        let mut reads = primary.as_ref().map_or_else(Vec::new, primary_reads);
        reads.retain_mut(|read| {
            if mask & 2 != 0 && read.text.starts_with('0') {
                read.text.remove(0);
                read.format = "UPCA".into();
                true
            } else {
                mask & 1 != 0
            }
        });
        if let Some(primary) = &primary {
            reads.extend(
                primary
                    .retail
                    .iter()
                    .filter(|read| match read.format.as_str() {
                        "EAN8" => mask & 4 != 0,
                        "UPCE" => mask & 8 != 0,
                        _ => false,
                    })
                    .cloned(),
            );
        }
        let mut unread = primary
            .as_ref()
            .map_or_else(Vec::new, |primary| unread_regions(primary, &reads));
        let localization_limited = primary
            .as_ref()
            .is_some_and(|p| p.localization_work_limited);
        let mut unfinished = primary.as_ref().is_some_and(crate::Result::unfinished);
        let had_extras = !extras.is_empty();
        for extra in extras {
            reads.extend(extra.barcodes.into_iter().map(Read::additional));
            if options.include_regions {
                unread.extend(extra.regions.into_iter().map(|region| Region {
                    format: region.format,
                    text: region.text,
                    polygon: region.polygon.map(|point| point.map(f64::from)),
                    support: region.support as u64,
                    localization_score: Some(f64::from(region.localization_score)),
                }));
            }
            unfinished |= extra.unfinished;
        }
        if had_extras {
            apply_supplement_policy(&mut reads, &mut unread, addons, options.include_regions);
            reads = distinct(reads);
            unread.retain(|region| {
                !reads
                    .iter()
                    .any(|read| overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
            });
            unread = distinct_regions(unread);
        }
        reads = crate::linear_duplicates::merge(reads, image);
        reads.sort_by_key(|read| std::cmp::Reverse(read.support));
        for (i, read) in reads.iter_mut().enumerate() {
            read.rank = Some(i + 1);
        }
        unfinished |= localization_limited;
        let raw = format_diagnostics(
            primary.as_ref(),
            &reads,
            &unread,
            options,
            start.elapsed().as_secs_f64() * 1000.0,
        )?;
        if !options.multiple && reads.len() > 1 {
            let remaining = reads.split_off(1);
            if options.include_regions {
                unread.extend(remaining.into_iter().map(|read| Region {
                    format: read.format,
                    text: read.text,
                    polygon: read.polygon,
                    support: read.support,
                    localization_score: None,
                }));
            }
        }
        typed_result(reads, unread, unfinished, localization_limited, raw)
    }
}

fn primary_result(result: &crate::Result) -> Result<EngineScan, Error> {
    let reads = primary_reads(result);
    let unread = unread_regions(result, &reads);
    let raw = result
        .options
        .retain_diagnostics
        .then(|| crate::result::value(result, MODE, 0.0))
        .transpose()?;
    typed_result(
        reads,
        unread,
        result.unfinished(),
        result.localization_work_limited,
        raw,
    )
}

fn format_diagnostics(
    primary: Option<&crate::Result>,
    reads: &[Read],
    unread: &[Region],
    options: ScanOptions,
    elapsed_ms: f64,
) -> Result<Option<Value>, Error> {
    if !options.retain_diagnostics {
        return Ok(None);
    }
    let mut raw = if let Some(primary) = primary {
        crate::result::value(primary, MODE, 0.0)?
    } else {
        json!({"schemaVersion":2,"mode":MODE,"multiple":true,"elapsedMs":0.0,"localizationLimited":false,"scan":{"barcodes":[],"unfinished":false}})
    };
    if options.include_regions {
        let mut regions: Vec<_> = reads
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|_| Error::OutputShape)?;
        regions.extend(
            unread
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| Error::OutputShape)?,
        );
        raw["scan"]["regions"] = json!(regions);
    }
    raw["multiple"] = json!(options.multiple);
    raw["elapsedMs"] = json!(elapsed_ms);
    Ok(Some(raw))
}

fn primary_reads(result: &crate::Result) -> Vec<Read> {
    result
        .barcodes()
        .iter()
        .map(|barcode| {
            let d = &barcode.detection;
            Read::primary(
                d.digits,
                d.polygon,
                d.support,
                d.axis,
                barcode.candidate_indices.clone(),
            )
        })
        .collect()
}

fn typed_result(
    reads: Vec<Read>,
    unread: Vec<Region>,
    unfinished: bool,
    localization_limited: bool,
    mut diagnostics: Option<Value>,
) -> Result<EngineScan, Error> {
    if let Some(raw) = &mut diagnostics {
        raw["scan"]["barcodes"] = serde_json::to_value(&reads).map_err(|_| Error::OutputShape)?;
        raw["scan"]["unfinished"] = json!(unfinished);
    }
    Ok(EngineScan {
        barcodes: reads
            .into_iter()
            .map(Read::into_public)
            .collect::<Result<_, _>>()?,
        undecoded: unread
            .into_iter()
            .map(Region::into_public)
            .collect::<Result<_, _>>()?,
        unfinished,
        localization_limited,
        diagnostics,
    })
}

fn apply_supplement_policy(
    reads: &mut Vec<Read>,
    unread: &mut Vec<Region>,
    policy: EanAddOnPolicy,
    include_regions: bool,
) {
    attach_supplements(reads);
    if policy == EanAddOnPolicy::Require {
        reads.retain(|read| {
            let accepted = !matches!(read.format.as_str(), "EAN13" | "UPCA" | "EAN8" | "UPCE")
                || read.addon.is_some();
            if !accepted && include_regions {
                unread.push(Region {
                    format: read.format.clone(),
                    ..Region::unknown(read.polygon)
                });
            }
            accepted
        });
    }
}

// Match confirmed supplements to the same physical base symbol, never text alone.
fn attach_supplements(reads: &mut [Read]) {
    let supplemental: Vec<_> = reads
        .iter()
        .filter(|read| read.addon.is_some())
        .map(|read| {
            (
                read.format.clone(),
                read.text.clone(),
                read.polygon,
                read.addon.clone(),
            )
        })
        .collect();
    for (format, text, polygon, addon) in supplemental {
        for base in &mut *reads {
            if base.format == format
                && base.text == text
                && base.addon.is_none()
                && overlap_quads(&base.polygon, &polygon).0 >= 0.65
            {
                base.addon.clone_from(&addon);
            }
        }
    }
}

fn distinct(mut reads: Vec<Read>) -> Vec<Read> {
    reads.sort_by_key(|read| std::cmp::Reverse(read.support));
    let mut result: Vec<Read> = Vec::new();
    for read in reads {
        if !result.iter().any(|other| {
            other.text == read.text
                && other.format == read.format
                && other.addon == read.addon
                && other.structured_append == read.structured_append
                && other.reader_initialization.unwrap_or(false)
                    == read.reader_initialization.unwrap_or(false)
                && overlap_quads(&other.polygon, &read.polygon).0 >= 0.65
        }) {
            result.push(read);
        }
    }
    result
}

fn distinct_regions(mut regions: Vec<Region>) -> Vec<Region> {
    regions.sort_by_key(|region| std::cmp::Reverse(region.support));
    let mut result: Vec<Region> = Vec::new();
    for region in regions {
        if !result.iter().any(|other| {
            other.text == region.text
                && other.format == region.format
                && overlap_quads(&other.polygon, &region.polygon).0 >= 0.65
        }) {
            result.push(region);
        }
    }
    result
}

fn unread_regions(result: &crate::Result, reads: &[Read]) -> Vec<Region> {
    if !result.options.include_regions {
        return Vec::new();
    }
    let decoded: std::collections::HashSet<_> = reads
        .iter()
        .flat_map(|read| read.candidate_indices.iter().flatten().copied())
        .collect();
    let mut unread: Vec<_> = result
        .proposals
        .iter()
        .enumerate()
        .filter(|(i, _)| !decoded.contains(i))
        .map(|(_, proposal)| Region::unknown(proposal.polygon))
        .collect();
    if let Some(recovery) = &result.recovery {
        unread.extend(recovery.unread.iter().cloned());
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
    use super::*;
    #[test]
    fn explicit_unread_regions_survive_without_diagnostics() {
        let q = [[0., 0.], [2., 0.], [2., 1.], [0., 1.]];
        let read = Read::primary([0; 13], q, 4, 0, vec![0]);
        let unread = Region::unknown(q.map(|[x, y]| [x + 3., y]));
        let result = typed_result(vec![read], vec![unread], false, false, None).unwrap();
        assert_eq!(result.barcodes.len(), 1);
        assert_eq!(result.undecoded.len(), 1);
        assert!((result.undecoded[0].polygon[0][0] - 3.).abs() < f64::EPSILON);
        assert!(result.undecoded[0].format.is_none());
        assert!(result.diagnostics.is_none());
    }
}

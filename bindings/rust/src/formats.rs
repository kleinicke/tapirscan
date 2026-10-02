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
    #[cfg(not(feature = "low"))]
    #[expect(
        clippy::too_many_lines,
        reason = "Keep ordered bounded source recovery passes together."
    )]
    fn recover_linear_regions(
        &mut self,
        image: Image<'_>,
        options: ScanOptions,
        mask: u32,
        primary: Option<&crate::Result>,
        reads: &mut Vec<Read>,
        unread: &mut Vec<Region>,
    ) -> Result<Vec<Region>, Error> {
        let mut profiles = std::mem::take(&mut self.fast_profiles);
        let result = profiles.with_source_cache(
            crate::ImageView::new(
                image.data,
                image.width,
                image.height,
                image.channels,
                image.stride,
            )?,
            crate::MODE_ID == 1,
            |sampler| {
                let mut deferred_reads = Vec::new();
                let proposals = if let Some(primary) = primary {
                    primary.proposals.clone()
                } else {
                    let im = crate::ImageView::new(
                        image.data,
                        image.width,
                        image.height,
                        image.channels,
                        image.stride,
                    )?;
                    if crate::MODE_ID == 1 && mask == 16384 {
                        self.localizer.detect_sparse(im, 768.)?.proposals
                    } else {
                        self.localizer.detect(im)?.proposals
                    }
                };
                // A broad mask can turn a short UPC-E interpretation of EAN-13 into a
                // false extra read. The new rare-format recovery requires explicit
                // selection; the established Medium readers keep their broad behavior.
                let selected_rare = if matches!(mask, 8 | 16384) { mask } else { 0 };
                let oriented_mask = if crate::MODE_ID == 1 {
                    mask & 496 | selected_rare
                } else {
                    mask & 16
                };
                let (mut found, mut pending) = crate::fast_linear::recover_proposals(
                    image,
                    &proposals,
                    oriented_mask,
                    sampler,
                )?;
                // The quick localizer misses a few severely rotated expanded symbols.
                // Preserve the full proposal search only when this selected reader has
                // no value after the source pass, keeping the common case bounded.
                if crate::MODE_ID == 1 && mask == 16384 && reads.is_empty() && found.is_empty() {
                    let im = crate::ImageView::new(
                        image.data,
                        image.width,
                        image.height,
                        image.channels,
                        image.stride,
                    )?;
                    let full = self.localizer.detect(im)?.proposals;
                    let (extra, regions) =
                        crate::fast_linear::recover_proposals(image, &full, mask, sampler)?;
                    found.extend(extra);
                    pending.extend(regions);
                }
                reads.extend(found);
                // Independent short-code source hypothesis, bounded to unresolved proposals.
                if reads.is_empty() && mask & 12 != 0 {
                    let unresolved: Vec<_> = proposals.iter().take(4).copied().collect();
                    let (mut extra, regions) = crate::fast_linear::recover_proposals_contrast(
                        image,
                        &unresolved,
                        mask & 12,
                        sampler,
                        1.5,
                    )?;
                    if matches!(crate::MODE_ID, 2 | 3) {
                        // Additional unresolved equal-payload claims remain provisional;
                        // an equal value alone cannot establish physical ownership.
                        extra = crate::linear_duplicates::merge(extra, image);
                        let ambiguous: Vec<_> = extra
                            .iter()
                            .enumerate()
                            .filter(|(i, read)| {
                                extra[..*i]
                                    .iter()
                                    .any(|old| old.format == read.format && old.text == read.text)
                            })
                            .map(|(_, read)| (read.format.clone(), read.text.clone()))
                            .collect();
                        extra.retain(|read| {
                            !ambiguous
                                .iter()
                                .any(|(format, text)| *format == read.format && *text == read.text)
                        });
                    }
                    for r in extra {
                        // Preserve existing physical owners and require separate source rows.
                        if r.support < 4
                            || reads
                                .iter()
                                .any(|old| overlap_quads(&r.polygon, &old.polygon).0 > 0.)
                        {
                            continue;
                        }
                        reads.push(r);
                    }
                    if options.include_regions {
                        pending.extend(regions);
                    }
                }
                // Polarity probe only for frames without an established owner. Multiple
                // inverse claims remain deferred rather than risking glare-split duplicates.
                if reads.is_empty() && mask & 15 != 0 {
                    let selected: Vec<_> = proposals.iter().take(4).copied().collect();
                    let (extra, regions) = crate::fast_linear::recover_proposals_inverted(
                        image,
                        &selected,
                        mask & 15,
                        sampler,
                    )?;
                    let extra = crate::linear_duplicates::merge(extra, image);
                    if extra.len() == 1 && extra[0].support >= 4 {
                        reads.extend(extra);
                    }
                    if options.include_regions {
                        pending.extend(regions);
                    }
                }
                // Reuse rejected narrow groups only after ordinary Medium recovery fails.
                if crate::MODE_ID == 1 && reads.is_empty() && mask & 12 != 0 {
                    let hypotheses = if let Some(cached) =
                        primary.and_then(|p| p.short_fragments.as_ref())
                    {
                        cached.clone()
                    } else {
                        use barcode_research_core::numeric::usize_f64;
                        let im = crate::ImageView::new(
                            image.data,
                            image.width,
                            image.height,
                            image.channels,
                            image.stride,
                        )?;
                        let scale = (768. / usize_f64(image.width.max(image.height))).min(1.);
                        let sx = usize_f64(image.width)
                            / (usize_f64(image.width) * scale).round().max(3.);
                        let sy = usize_f64(image.height)
                            / (usize_f64(image.height) * scale).round().max(3.);
                        let mut fragments = Vec::new();
                        let _ = barcode_research_core::stripes::detect_with_observer(im, |g| {
                            let [u0, u1, v0, v1] = g.bounds;
                            let (w, h) = (u1 - u0, v1 - v0);
                            let narrow = g.reason == "narrow"
                                && w >= 24.
                                && h >= 5.
                                && w / h >= 2.
                                && g.edges >= 120;
                            if !narrow {
                                return;
                            }
                            let half = w * 2.;
                            let center = f64::midpoint(u0, u1);
                            let (s, c) = g.angle.sin_cos();
                            let polygon = [
                                [center - half, v0],
                                [center + half, v0],
                                [center + half, v1],
                                [center - half, v1],
                            ]
                            .map(|[u, v]| [(u * c - v * s) * sx, (u * s + v * c) * sy]);
                            fragments.push((g.edges, crate::Proposal { polygon, score: 0. }));
                        })?;
                        fragments.sort_by_key(|(edges, _)| std::cmp::Reverse(*edges));
                        let hypotheses: Vec<_> =
                            fragments.into_iter().take(4).map(|(_, p)| p).collect();
                        hypotheses
                    };
                    let (extra, regions) = crate::fast_linear::recover_proposals_contrast(
                        image,
                        &hypotheses,
                        mask & 12,
                        sampler,
                        1.5,
                    )?;
                    let extra = crate::linear_duplicates::merge(extra, image);
                    if extra.len() == 1 && extra[0].support >= 4 {
                        reads.extend(extra);
                    }
                    if options.include_regions {
                        pending.extend(regions);
                    }
                }
                // Reuse the existing source proposals and gray-profile sampler. A
                // decoded region only suppresses a retry when it contains every corner
                // of that proposal; one read never ends scanning of the whole image.
                let recovery_mask = mask & (16 | 32 | 64 | 256);
                if recovery_mask != 0 {
                    let limit = [0, 8, 8, 16][crate::MODE_ID as usize];
                    let unresolved: Vec<_> = proposals
                        .iter()
                        .filter(|proposal| {
                            !reads.iter().any(|read| {
                                proposal
                                    .polygon
                                    .iter()
                                    .all(|point| contains_point(*point, &read.polygon))
                            })
                        })
                        .take(limit)
                        .copied()
                        .collect();
                    let (extra, regions) = crate::fast_linear::recover_proposals_contrast(
                        image,
                        &unresolved,
                        recovery_mask,
                        sampler,
                        2.,
                    )?;
                    if !extra.is_empty() {
                        let (reconciled, deferred) = crate::recovery_admission::append_recovered(
                            std::mem::take(reads),
                            extra,
                            image,
                            sampler,
                            2.,
                        );
                        *reads = reconciled;
                        deferred_reads.extend(deferred);
                        *reads = crate::linear_duplicates::merge_selected(
                            std::mem::take(reads),
                            image,
                            recovery_mask & 256 != 0,
                        );
                    }
                    if options.include_regions {
                        pending.extend(regions);
                    }
                }
                #[cfg(any(feature = "high", feature = "very-high"))]
                if mask & 3 != 0 {
                    let bands = crate::detail::source_bands(image, reads);
                    let (extra, regions) =
                        crate::fast_linear::recover_proposals(image, &bands, mask & 3, sampler)?;
                    reads.extend(extra);
                    if options.include_regions {
                        unread.extend(regions);
                    }
                }
                #[cfg(feature = "medium")]
                if mask & 12 != 0 && image.width.max(image.height) > 768 {
                    crate::short_crop::recover(
                        &mut self.localizer,
                        image,
                        &proposals,
                        mask,
                        sampler,
                        reads,
                    )?;
                }
                if options.include_regions {
                    unread.extend(pending);
                }
                Ok(deferred_reads)
            },
        );
        self.fast_profiles = profiles;
        result
    }

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
        #[cfg(feature = "low")]
        if crate::fast_linear::enabled(options, mask, addons) {
            return crate::fast_linear::scan(self, image, options, mask);
        }

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
        let (extras, coverage) = scan_additional(
            image,
            if shared_retail { mask & !12 } else { mask },
            addons,
            &mut self.additional_gray,
        )?;
        let primary = if shared_retail || mask & 3 != 0 {
            Some(self.scan_with_coverage(image, full_options, &coverage, false, shared_retail)?)
        } else {
            None
        };
        let mut reads = primary.as_ref().map_or_else(Vec::new, primary_reads);
        retain_requested_primary(&mut reads, mask);
        extend_primary_retail(&mut reads, primary.as_ref(), mask);
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
        #[cfg(not(feature = "low"))]
        let mut deferred_reads = Vec::new();
        #[cfg(not(feature = "low"))]
        if (mask & 511 != 0 || (crate::MODE_ID == 1 && mask == 16384))
            && addons == EanAddOnPolicy::Ignore
        {
            deferred_reads = self.recover_linear_regions(
                image,
                options,
                mask,
                primary.as_ref(),
                &mut reads,
                &mut unread,
            )?;
            unfinished = true;
        }
        if had_extras {
            apply_supplement_policy(&mut reads, &mut unread, addons, options.include_regions);
            reads = distinct(reads);
            unread = retain_unresolved(unread, &reads);
        }
        // Deferred physical-instance claims must survive the generic overlap
        // cleanup: an existing read was not sufficient proof of their identity.
        #[cfg(not(feature = "low"))]
        if options.include_regions {
            unread.extend(deferred_reads);
        }
        reads = crate::linear_duplicates::merge(reads, image);
        rank_reads(&mut reads);
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

pub(crate) fn typed_result(
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

fn rank_reads(reads: &mut [Read]) {
    reads.sort_by_key(|read| std::cmp::Reverse(read.support));
    for (i, read) in reads.iter_mut().enumerate() {
        read.rank = Some(i + 1);
    }
}

fn retain_unresolved(mut unread: Vec<Region>, reads: &[Read]) -> Vec<Region> {
    unread.retain(|region| {
        !reads
            .iter()
            .any(|read| overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
    });
    distinct_regions(unread)
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

// Source rows are validated before conversion. Specializing the channel count
// lets the compiler vectorize the exact integer luminance calculation.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Weighted bytes plus 128 are at most 65408; shifting eight bits is at most 255."
)]
fn gray_rows<const CHANNELS: usize>(image: Image<'_>, gray: &mut [u8]) {
    for (source, target) in image
        .data
        .chunks(image.stride)
        .zip(gray.chunks_mut(image.width))
    {
        for (pixel, value) in source[..image.width * CHANNELS]
            .chunks_exact(CHANNELS)
            .zip(target)
        {
            *value = ((u32::from(pixel[0]) * 77
                + u32::from(pixel[1]) * 150
                + u32::from(pixel[2]) * 29
                + 128)
                >> 8) as u8;
        }
    }
}

fn gray_image(image: Image<'_>, gray: &mut Vec<u8>) -> Result<(), Error> {
    gray.resize(image.width * image.height, 0);
    match image.channels {
        1 => {
            for (source, target) in image
                .data
                .chunks(image.stride)
                .zip(gray.chunks_mut(image.width))
            {
                target.copy_from_slice(&source[..image.width]);
            }
        }
        3 => gray_rows::<3>(image, gray),
        4 => gray_rows::<4>(image, gray),
        _ => return Err(Error::Parameters),
    }
    Ok(())
}

fn scaled_matrix_retry(
    pixels: &[u8],
    width: usize,
    height: usize,
    target_long: usize,
    mask: u32,
    effort: usize,
) -> barcode_multiformat::Scan {
    let long = width.max(height);
    let out_width = ((width * target_long + long / 2) / long).max(3);
    let out_height = ((height * target_long + long / 2) / long).max(3);
    let x_source: Vec<_> = (0..out_width)
        .map(|x| ((2 * x + 1) * width / (2 * out_width)).min(width - 1))
        .collect();
    let mut smaller = vec![0; out_width * out_height];
    for y in 0..out_height {
        let source_y = ((2 * y + 1) * height / (2 * out_height)).min(height - 1);
        let source = &pixels[source_y * width..(source_y + 1) * width];
        let target = &mut smaller[y * out_width..(y + 1) * out_width];
        for (pixel, &x) in target.iter_mut().zip(&x_source) {
            *pixel = source[x];
        }
    }
    let mut scan = scan_reader(&smaller, out_width, out_height, mask, effort);
    #[cfg(any(feature = "high", feature = "very-high"))]
    if mask == 512 && effort == 2 {
        let known: Vec<_> = scan.barcodes.iter().collect();
        let recovered =
            barcode_research_core::qr_grid::recover(&smaller, out_width, out_height, &scan, &known);
        scan.barcodes.extend(recovered.barcodes);
        if scan.barcodes.is_empty() {
            let foreground = barcode_research_core::qr_frontend::scan_foreground(
                &smaller, out_width, out_height,
            );
            scan.barcodes.extend(foreground.barcodes);
            scan.regions.extend(foreground.regions);
        }
    }
    let scale_x = barcode_multiformat::numeric::usize_f32(width)
        / barcode_multiformat::numeric::usize_f32(out_width);
    let scale_y = barcode_multiformat::numeric::usize_f32(height)
        / barcode_multiformat::numeric::usize_f32(out_height);
    for polygon in scan
        .barcodes
        .iter_mut()
        .map(|value| &mut value.polygon)
        .chain(scan.regions.iter_mut().map(|value| &mut value.polygon))
    {
        for point in polygon {
            point[0] *= scale_x;
            point[1] *= scale_y;
        }
    }
    scan.unfinished = true;
    scan
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

/// Broad selections also retain the existing full-image readers, including their
/// complementary linear detections. Pure `Common1D` never pays for this pass.
#[cfg(feature = "low")]
pub(crate) fn fast_additional(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    mask: u32,
) -> Result<(Vec<Read>, Vec<Region>), Error> {
    let (scans, _) = scan_additional(
        image,
        mask,
        EanAddOnPolicy::Ignore,
        &mut scanner.additional_gray,
    )?;
    let mut reads = Vec::new();
    let mut unread = Vec::new();
    for scan in scans {
        reads.extend(scan.barcodes.into_iter().map(Read::additional));
        if options.include_regions {
            unread.extend(scan.regions.into_iter().map(|region| Region {
                format: region.format,
                text: region.text,
                polygon: region.polygon.map(|point| point.map(f64::from)),
                support: region.support as u64,
                localization_score: Some(f64::from(region.localization_score)),
            }));
        }
    }
    let reads = distinct(reads);
    unread.retain(|region| {
        !reads
            .iter()
            .any(|read| overlap_quads(&region.polygon, &read.polygon).1 >= 0.65)
    });
    Ok((reads, distinct_regions(unread)))
}

/// Run the linear reader before primary discovery so strong reads can guide deep retries.
#[expect(
    clippy::too_many_lines,
    reason = "Keep shared grayscale, initial readers and bounded recovery ordered together."
)]
fn scan_additional(
    image: Image<'_>,
    mask: u32,
    addons: EanAddOnPolicy,
    gray: &mut Vec<u8>,
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
    // ITF needs the original angle coverage even with localized profile recovery.
    let preserved = if crate::MODE_ID == 1 && addons == EanAddOnPolicy::Ignore {
        linear & 64
    } else {
        0
    };
    let effort = if addons == EanAddOnPolicy::Ignore {
        [0, 0, 2, 2]
    } else {
        [0, 1, 2, 2]
    }[crate::MODE_ID as usize];
    let qr_effort = [0, 1, 2, 3][crate::MODE_ID as usize];
    let pixels = if image.channels == 1 && image.stride == image.width {
        &image.data[..image.width * image.height]
    } else {
        gray_image(image, gray)?;
        gray.as_slice()
    };
    #[cfg(any(feature = "low", feature = "medium"))]
    let sparse = (crate::LOW_FAST_PATH
        && (matrix != 0 || (linear & !127 != 0 && addons == EanAddOnPolicy::Ignore)))
        .then(|| crate::fast_sparse::Prepared::new(pixels, image.width, image.height));
    let mut scans = Vec::new();
    let mut coverage = Vec::new();
    for (selected, level) in [
        (linear & !preserved, effort),
        (preserved, 1),
        (matrix, if matrix & 512 != 0 { qr_effort } else { 1 }),
    ] {
        if selected == 0 {
            continue;
        }
        #[cfg(any(feature = "low", feature = "medium"))]
        let scan = if let Some(sparse) = sparse.as_ref().filter(|_| {
            selected == matrix
                || (crate::LOW_FAST_PATH
                    && selected & !127 != 0
                    && addons == EanAddOnPolicy::Ignore)
        }) {
            sparse.scan(selected, level)
        } else {
            scan_reader(
                pixels,
                image.width,
                image.height,
                selected | addons.engine_bits(),
                level,
            )
        };
        #[cfg(not(any(feature = "low", feature = "medium")))]
        let scan = scan_reader(
            pixels,
            image.width,
            image.height,
            selected | addons.engine_bits(),
            level,
        );
        if selected & LINEAR_MASK != 0
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
        #[cfg(feature = "low")]
        let retries = if crate::LOW_FAST_PATH
            && selected == 512
            && mask == selected
            && addons == EanAddOnPolicy::Ignore
            && scan.barcodes.is_empty()
            && image.width.max(image.height) > 1920
        {
            vec![scaled_matrix_retry(
                pixels,
                image.width,
                image.height,
                960,
                selected,
                level,
            )]
        } else {
            Vec::new()
        };
        #[cfg(feature = "medium")]
        let retries = medium_retries(
            pixels,
            [image.width, image.height],
            mask,
            selected,
            level,
            addons,
            &scan,
        );
        #[cfg(any(feature = "high", feature = "very-high"))]
        let retries = if selected == 512
            && mask == selected
            && addons == EanAddOnPolicy::Ignore
            && scan.barcodes.is_empty()
            && image.width.max(image.height) > 1920
        {
            vec![scaled_matrix_retry(
                pixels,
                image.width,
                image.height,
                960,
                selected,
                1,
            )]
        } else {
            Vec::new()
        };
        #[cfg(not(feature = "low"))]
        let mut scan = scan;
        #[cfg(not(feature = "low"))]
        let grid_retries = if selected == matrix && addons == EanAddOnPolicy::Ignore {
            matrix_retries(pixels, [image.width, image.height], selected, &mut scan)
        } else {
            Vec::new()
        };
        let projected = if selected == 512
            && !scan.regions.is_empty()
            && (crate::MODE_ID != 0 || crate::LOW_FAST_PATH)
        {
            let known: Vec<_> = scan
                .barcodes
                .iter()
                .chain(retries.iter().flat_map(|r| &r.barcodes))
                .collect();
            #[cfg(not(feature = "low"))]
            let known: Vec<_> = known
                .into_iter()
                .chain(grid_retries.iter().flat_map(|r| &r.barcodes))
                .collect();
            Some(barcode_research_core::qr_grid::recover(
                pixels,
                image.width,
                image.height,
                &scan,
                &known,
            ))
        } else {
            None
        };
        scans.push(scan);
        scans.extend(retries);
        scans.extend(projected);
        #[cfg(not(feature = "low"))]
        scans.extend(grid_retries);
    }
    // Extra foreground thresholds reach unresolved QR frames without a surviving
    // region. Bound discovery to Full-HD-sized inputs; restoration has a smaller
    // pixel budget because it introduces another image and threshold search.
    #[cfg(not(feature = "low"))]
    if mask == 512
        && addons == EanAddOnPolicy::Ignore
        && image.width * image.height <= 2_097_152
        && scans.iter().all(|scan| scan.barcodes.is_empty())
        && (crate::MODE_ID >= 2 || scans.iter().all(|scan| scan.regions.is_empty()))
        && pixels.iter().any(|&value| value != 0 && value != 255)
    {
        let foreground =
            barcode_research_core::qr_frontend::scan_foreground(pixels, image.width, image.height);
        let restoration_limit = if crate::MODE_ID >= 2 {
            2_097_152
        } else {
            262_144
        };
        let restore =
            foreground.barcodes.is_empty() && image.width * image.height <= restoration_limit;
        scans.push(foreground);
        if restore {
            let sharpened = crate::signal_recovery::sharpen(pixels, image.width, image.height, 2);
            if sharpened != pixels {
                let sharpened_scan = scan_reader(
                    &sharpened,
                    image.width,
                    image.height,
                    512,
                    if crate::MODE_ID >= 2 { 2 } else { 1 },
                );
                let retry_foreground = sharpened_scan.barcodes.is_empty();
                scans.push(sharpened_scan);
                if retry_foreground {
                    scans.push(barcode_research_core::qr_frontend::scan_foreground(
                        &sharpened,
                        image.width,
                        image.height,
                    ));
                }
            }
        }
    }
    #[cfg(any(feature = "high", feature = "very-high"))]
    if mask == 512
        && addons == EanAddOnPolicy::Ignore
        && scans.iter().all(|scan| scan.barcodes.is_empty())
    {
        let targets: &[usize] = if crate::MODE_ID == 3 {
            &[1200, 1600]
        } else {
            &[1200]
        };
        for &target in targets {
            if image.width.max(image.height) <= target {
                continue;
            }
            let retry = scaled_matrix_retry(pixels, image.width, image.height, target, 512, 2);
            let decoded = !retry.barcodes.is_empty();
            scans.push(retry);
            if decoded {
                break;
            }
        }
    }
    #[cfg(any(feature = "high", feature = "very-high"))]
    if mask == 512
        && addons == EanAddOnPolicy::Ignore
        && image.width * image.height <= 2_097_152
        && scans.iter().all(|scan| scan.barcodes.is_empty())
    {
        for method in 0..2 {
            let normalized = if method == 0 {
                crate::signal_recovery::equalize(pixels)
            } else {
                pixels.iter().map(|&v| 255 - v).collect::<Vec<_>>()
            };
            if normalized == pixels {
                continue;
            }
            let retry = scan_reader(&normalized, image.width, image.height, 512, 2);
            let decoded = !retry.barcodes.is_empty();
            scans.push(retry);
            if decoded {
                break;
            }
        }
    }
    Ok((scans, coverage))
}

#[cfg(feature = "medium")]
fn medium_retries(
    pixels: &[u8],
    [width, height]: [usize; 2],
    mask: u32,
    selected: u32,
    level: usize,
    addons: EanAddOnPolicy,
    scan: &barcode_multiformat::Scan,
) -> Vec<barcode_multiformat::Scan> {
    if addons != EanAddOnPolicy::Ignore || !scan.barcodes.is_empty() || mask != selected {
        return Vec::new();
    }
    let mut retries = Vec::new();
    if mask == 8192 {
        let sparse = crate::fast_sparse::Prepared::new(pixels, width, height);
        retries.push(sparse.scan(mask, 0));
    }
    if matches!(mask, 512 | 131_072) && width.max(height) > 1920 {
        let dimensions: &[usize] = if mask == 512 { &[960] } else { &[1600, 1200] };
        for &target in dimensions {
            retries.push(scaled_matrix_retry(
                pixels, width, height, target, mask, level,
            ));
        }
    }
    // Revisit an unresolved QR candidate with the existing extended reader.
    // Requiring a pending region avoids the expensive pass on blank misses.
    if mask == 512 && width.max(height) <= 1920 && scan.unfinished && !scan.regions.is_empty() {
        retries.push(scan_reader(pixels, width, height, mask, 2));
    }
    retries
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
mod result_tests;

#[cfg(test)]
mod grayscale_reuse_tests;

fn retain_requested_primary(reads: &mut Vec<Read>, mask: u32) {
    reads.retain_mut(|read| {
        if mask & 2 != 0 && read.text.starts_with('0') {
            read.text.remove(0);
            read.format = "UPCA".into();
            true
        } else {
            mask & 1 != 0
        }
    });
}

fn extend_primary_retail(reads: &mut Vec<Read>, primary: Option<&crate::Result>, mask: u32) {
    if let Some(primary) = primary {
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
}

/// One source-sample allowance for the whole matrix group, independent of how
/// many formats are enabled. Initial readers have already tried every format;
/// finder hints prioritize bounded recovery without discarding pending regions.
#[cfg(not(feature = "low"))]
fn matrix_retries(
    pixels: &[u8],
    [width, height]: [usize; 2],
    mask: u32,
    scan: &mut barcode_multiformat::Scan,
) -> Vec<barcode_multiformat::Scan> {
    let mode = crate::MODE_ID as usize;
    let mut budget = [0, 250_000, 2_000_000, 4_000_000][mode];
    let mut crop_budget = [0, 65_536, 131_072, 262_144][mode];
    let mut retries: Vec<barcode_multiformat::Scan> = Vec::new();
    if mask & 4096 != 0 {
        crate::matrix_grid::resolve_runes(pixels, width, height, scan, &mut budget);
    }
    // Pending recovery remains unfinished even when its shared allowance or
    // crop size cap prevents a retry, including candidates beyond the first eight.
    scan.unfinished |= scan.regions.iter().any(|region| {
        let selected = match region.format.as_str() {
            "QRCode" => 512,
            "DataMatrix" => 1024,
            "Aztec" => 4096,
            _ => 0,
        };
        mask & selected != 0
            && !scan.barcodes.iter().any(|read| {
                read.format == region.format
                    && crate::signal_recovery::covered(region.polygon, read.polygon)
            })
    });
    for region in scan.regions.iter().take(8) {
        let hinted = match region.format.as_str() {
            "QRCode" => 512,
            "DataMatrix" => 1024,
            "Aztec" => 4096,
            _ => 0,
        };
        if mask & hinted == 0 {
            continue;
        }
        // Only complete containment can skip a localized retry. Small decoded
        // matrix polygons cannot suppress a larger unresolved candidate.
        if scan
            .barcodes
            .iter()
            .chain(retries.iter().flat_map(|s| &s.barcodes))
            .any(|r| {
                r.format == region.format
                    && crate::signal_recovery::covered(region.polygon, r.polygon)
            })
        {
            continue;
        }
        let pending = barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: vec![region.clone()],
            unfinished: true,
            lines: 0,
        };
        if hinted != 512 && budget > 0 {
            retries.push(crate::matrix_grid::recover(
                pixels,
                width,
                height,
                hinted,
                &pending,
                &mut budget,
            ));
        }
        if crop_budget > 0
            && !retries.iter().flat_map(|s| &s.barcodes).any(|r| {
                r.format == region.format
                    && crate::signal_recovery::covered(region.polygon, r.polygon)
            })
        {
            retries.extend(crate::signal_recovery::region_retries(
                pixels,
                width,
                height,
                hinted,
                [0, 1, 2, 3][mode],
                &pending,
                &mut crop_budget,
            ));
        }
    }
    retries
}

#[cfg(all(test, not(feature = "low")))]
mod matrix_budget_tests {
    use super::matrix_retries;
    #[test]
    fn no_pending_candidates_do_not_invent_work() {
        let mut scan = barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: Vec::new(),
            unfinished: false,
            lines: 0,
        };
        assert!(matrix_retries(&[255; 64], [8, 8], 1536, &mut scan).is_empty());
        assert!(!scan.unfinished);
    }
}

#[cfg(all(test, not(feature = "low")))]
mod pending_recovery_tests {
    #[test]
    fn oversized_pending_qr_remains_unfinished_without_a_retry() {
        let mut scan = barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: vec![barcode_multiformat::regions::Region {
                format: "QRCode".into(),
                text: String::new(),
                polygon: [[0., 0.], [999., 0.], [999., 999.], [0., 999.]],
                localization_score: 1.,
                support: 1,
            }],
            unfinished: false,
            lines: 0,
        };
        let pixels = vec![255; 1_000_000];
        assert!(super::matrix_retries(&pixels, [1000, 1000], 512, &mut scan).is_empty());
        assert!(scan.unfinished);
        assert_eq!(scan.regions.len(), 1);
    }
}

/// Dispatch effort-preserving QR-only scans through the maintained finder frontend.
pub(crate) fn scan_reader(
    pixels: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
) -> barcode_multiformat::Scan {
    if mask & 4096 != 0 && mask & !(512 | 1024 | 2048 | 4096 | 131_072) == 0 {
        crate::matrix_frontend::scan(pixels, width, height, mask, effort)
    } else if mask == 512 {
        barcode_research_core::qr_frontend::scan_with_effort(pixels, width, height, effort)
    } else {
        barcode_multiformat::scan(pixels, width, height, mask, effort)
    }
}

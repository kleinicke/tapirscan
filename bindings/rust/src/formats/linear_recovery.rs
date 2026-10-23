//! Ordered, bounded recovery of linear symbols inside localized source regions.
use super::{
    NON_RETAIL_LINEAR, PRIMARY_RETAIL, SHORT_RETAIL, SWEPT, contains_point, gray_image,
    is_label_sheet_sized, is_large_frame, policy::POLICY, scan_reader,
};
use crate::format_registry::{ENGINE_OPT_IN_MASK, LINEAR_MASK};
use crate::read::{Read, Region};
use crate::{Error, Image, ScanOptions, Scanner, geometry::overlap_quads};
use barcode_multiformat::formats::{
    CODE39, CODE93, CODE128, COMMON_LINEAR_MASK, DATA_BAR, DATA_BAR_EXPANDED, ITF, RETAIL_MASK,
    UPCE,
};

/// EAN-13/UPC-A reads that mark a large frame as a batch frame (shelf, carton).
const RETAIL_BATCH_READS: usize = 8;

/// Window size, in source pixels, for linear localization of large frames. A
/// 1536-pixel window puts the 768-pixel localizer raster at half scale.
const WINDOW_SIDE: usize = 1536;
/// Most windowed proposals decoded per frame, strongest first.
const WINDOW_PROPOSALS: usize = 32;

/// Stripe proposals from overlapping windows that no full-frame proposal covers.
fn windowed_proposals(
    localizer: &mut barcode_research_core::stripes::Detector,
    image: Image<'_>,
    known: &[barcode_research_core::oriented::Proposal],
) -> Result<Vec<barcode_research_core::oriented::Proposal>, Error> {
    let starts = |n: usize| -> Vec<usize> {
        if n <= WINDOW_SIDE {
            return vec![0];
        }
        let mut starts: Vec<usize> = (0..n - WINDOW_SIDE).step_by(WINDOW_SIDE - 256).collect();
        starts.push(n - WINDOW_SIDE);
        starts
    };
    let (width, height) = (image.width.min(WINDOW_SIDE), image.height.min(WINDOW_SIDE));
    let mut candidates = Vec::new();
    for y in starts(image.height) {
        for x in starts(image.width) {
            let offset = y * image.stride + x * image.channels;
            let window = crate::ImageView::new(
                &image.data[offset..],
                width,
                height,
                image.channels,
                image.stride,
            )?;
            for mut proposal in localizer.detect(window)?.proposals {
                for point in &mut proposal.polygon {
                    point[0] += barcode_research_core::numeric::usize_f64(x);
                    point[1] += barcode_research_core::numeric::usize_f64(y);
                }
                candidates.push(proposal);
            }
        }
    }
    Ok(strongest_new_proposals(known, candidates))
}

/// Stripe proposals from one half-scale raster that no full-frame proposal covers.
fn half_scale_proposals(
    localizer: &mut barcode_research_core::stripes::Detector,
    image: Image<'_>,
    known: &[barcode_research_core::oriented::Proposal],
) -> Result<Vec<barcode_research_core::oriented::Proposal>, Error> {
    let view = crate::ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    )?;
    let side = barcode_research_core::numeric::usize_f64(image.width.max(image.height));
    let candidates = localizer.detect_at(view, side * 0.5)?.proposals;
    Ok(strongest_new_proposals(known, candidates))
}

/// The strongest `WINDOW_PROPOSALS` candidates, in order, that neither `known` nor
/// an earlier kept candidate already covers.
fn strongest_new_proposals(
    known: &[barcode_research_core::oriented::Proposal],
    candidates: Vec<barcode_research_core::oriented::Proposal>,
) -> Vec<barcode_research_core::oriented::Proposal> {
    let mut found: Vec<barcode_research_core::oriented::Proposal> = Vec::new();
    for proposal in candidates {
        // Only a proposal of similar extent was already tried: a large
        // full-frame region does not cover the small symbols inside it.
        let new = !known.iter().chain(found.iter()).any(|old| {
            let (a, b) = overlap_quads(&old.polygon, &proposal.polygon);
            a.min(b) > 0.5
        });
        if new {
            found.push(proposal);
        }
    }
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(WINDOW_PROPOSALS);
    found
}

/// Unread retail boxes whose central band is decoded for the other linear formats.
const CORE_PROPOSALS: usize = 4;
/// Linear reads of the first boxes that mark a sheet of symbols, whose further boxes are then
/// decoded too.
const SHEET_READS: usize = 8;
/// Boxes that, with half of [`SHEET_READS`] read from them, also mark a sheet: rotated
/// symbols spend the shared recovery budgets before twelve of them are read.
const SHEET_BOXES: usize = 16;

/// The middle 40% of a box across its bars, keeping its full length.
fn central_band(q: &crate::Quad) -> crate::Quad {
    use crate::geometry::{distance, lerp};
    // Corners 0-1 and 3-2 run along the bars' scan axis unless 1-2 is the longer edge.
    if distance(q[0], q[1]) >= distance(q[1], q[2]) {
        [
            lerp(q[0], q[3], 0.3),
            lerp(q[1], q[2], 0.3),
            lerp(q[1], q[2], 0.7),
            lerp(q[0], q[3], 0.7),
        ]
    } else {
        [
            lerp(q[0], q[1], 0.3),
            lerp(q[0], q[1], 0.7),
            lerp(q[3], q[2], 0.7),
            lerp(q[3], q[2], 0.3),
        ]
    }
}

/// Unread boxes with unconfirmed rows that the whole-image linear reader retries on a
/// linear-only frame (see `sweep_box`).
const UNREAD_BOX_SWEEPS: usize = 2;

/// Rows that a UPC-E read of the broadened localized recovery needs: a row through an
/// EAN-13 can decode as a short UPC-E.
const UPCE_HYPOTHESIS_SUPPORT: u64 = 3;

/// Run the whole-image linear reader on the padded bounding rectangle of one box, with
/// reads in source coordinates.
fn sweep_box(
    image: Image<'_>,
    polygon: &crate::Quad,
    mask: u32,
    gray: &mut Vec<u8>,
) -> Result<Vec<Read>, Error> {
    let (mut low, mut high) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for point in polygon {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    let pad = (high[0] - low[0]).max(high[1] - low[1]) * 0.1 + 8.;
    let clamp = |value: f64, limit: usize| {
        barcode_research_core::numeric::f64_usize(
            value.clamp(0., barcode_research_core::numeric::usize_f64(limit)),
        )
    };
    let (x0, x1) = (
        clamp((low[0] - pad).floor(), image.width),
        clamp((high[0] + pad).ceil(), image.width),
    );
    let (y0, y1) = (
        clamp((low[1] - pad).floor(), image.height),
        clamp((high[1] + pad).ceil(), image.height),
    );
    if x1 <= x0 + 2 || y1 <= y0 + 2 {
        return Ok(Vec::new());
    }
    let crop = Image {
        data: &image.data[y0 * image.stride + x0 * image.channels..],
        width: x1 - x0,
        height: y1 - y0,
        channels: image.channels,
        stride: image.stride,
    };
    gray_image(crop, gray)?;
    let scan = scan_reader(gray, crop.width, crop.height, mask, POLICY.sweep_effort);
    let offset = [
        barcode_multiformat::numeric::usize_f32(x0),
        barcode_multiformat::numeric::usize_f32(y0),
    ];
    Ok(scan
        .barcodes
        .into_iter()
        .map(|mut barcode| {
            for point in &mut barcode.polygon {
                point[0] += offset[0];
                point[1] += offset[1];
            }
            Read::additional(barcode)
        })
        .collect())
}

/// Whether a read is of a linear format; 2D reads do not stop linear recovery.
fn is_linear_read(read: &Read) -> bool {
    !matches!(
        read.format.as_str(),
        "QRCode" | "DataMatrix" | "PDF417" | "Aztec" | "MaxiCode"
    )
}

impl Scanner {
    #[expect(
        clippy::too_many_lines,
        reason = "Keep ordered bounded source recovery passes together."
    )]
    pub(super) fn recover_linear_regions(
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
            POLICY.source_profile_cache,
            |sampler| {
                let mut deferred_reads = Vec::new();
                let localized;
                let proposals = if let Some(primary) = primary {
                    primary.proposals.as_slice()
                } else {
                    // Segment-voting boxes follow rotated and warped bars.
                    localized = crate::pipeline::segment_voting::localize(image).proposals;
                    localized.as_slice()
                };
                // A broad mask can turn a short UPC-E interpretation of EAN-13 into a
                // false extra read. The new rare-format recovery requires explicit
                // selection; the established Medium readers keep their broad behavior.
                let selected_rare = if matches!(mask, UPCE | DATA_BAR_EXPANDED) {
                    mask
                } else {
                    0
                };
                // Every non-retail linear format reuses the localized boxes; Low does the same in
                // its fast path. DataBar and DataBar Expanded gained most (Low reads them best).
                let oriented_mask = mask
                    & (NON_RETAIL_LINEAR | DATA_BAR | DATA_BAR_EXPANDED | ENGINE_OPT_IN_MASK)
                    | selected_rare;
                // Low's fast path reads UPC-E in the localized boxes with any selection; the
                // primary scan misses rotated ones. Where the boxes are decoded for other
                // linear formats anyway and the primary scan read no linear symbol, they are
                // also decoded for UPC-E, and such reads need `UPCE_HYPOTHESIS_SUPPORT` rows.
                let upce_hypothesis = mask & UPCE != 0
                    && oriented_mask & !UPCE != 0
                    && !reads.iter().any(is_linear_read);
                let oriented_mask = if upce_hypothesis {
                    oriented_mask | UPCE
                } else {
                    oriented_mask
                };
                let mut hinted = Vec::new();
                let (mut found, mut pending) = crate::fast_linear::recover_proposals_hinted(
                    image,
                    proposals,
                    oriented_mask,
                    sampler,
                    &mut hinted,
                )?;
                // A sheet of symbols read through many of its first boxes: decode the further
                // segment-voting boxes that no read covers.
                let sheet_like = found.len() >= SHEET_READS
                    || (found.len() >= SHEET_READS / 2 && proposals.len() >= SHEET_BOXES);
                if oriented_mask != 0 && sheet_like {
                    // The first boxes share the recovery budgets, which a sheet uses up:
                    // its unread first boxes and further boxes get Low's per box.
                    let sheet: Vec<_> = proposals
                        .iter()
                        .copied()
                        .chain(crate::pipeline::segment_voting::localize(image).more)
                        .filter(|p| {
                            !reads
                                .iter()
                                .chain(found.iter())
                                .any(|read| overlap_quads(&read.polygon, &p.polygon).0 > 0.)
                        })
                        .collect();
                    if !sheet.is_empty() {
                        let (extra, _) = crate::fast_linear::recover_sheet(
                            image,
                            &sheet,
                            oriented_mask,
                            sampler,
                        )?;
                        found.extend(extra);
                    }
                }
                // On such a sheet, also run Low's whole-image sweep for the formats that
                // Medium otherwise reads only in boxes: Low reads its rows there.
                if oriented_mask & LINEAR_MASK & !SWEPT != 0 && sheet_like {
                    let w = barcode_research_core::numeric::usize_f64(image.width);
                    let h = barcode_research_core::numeric::usize_f64(image.height);
                    found.extend(sweep_box(
                        image,
                        &[[0., 0.], [w, 0.], [w, h], [0., h]],
                        oriented_mask & LINEAR_MASK & !SWEPT,
                        &mut self.additional_gray,
                    )?);
                }
                // Retail boxes span the full bar height; rows near their edges can keep
                // checksum-free formats below their row support. Decode the central band of
                // unread boxes, which stripe localization would have proposed on its own.
                if oriented_mask & NON_RETAIL_LINEAR != 0 {
                    let cores: Vec<_> = proposals
                        .iter()
                        .filter(|p| {
                            !reads
                                .iter()
                                .chain(found.iter())
                                .any(|read| overlap_quads(&read.polygon, &p.polygon).0 > 0.)
                        })
                        .take(CORE_PROPOSALS)
                        .map(|p| crate::Proposal {
                            polygon: central_band(&p.polygon),
                            score: p.score,
                        })
                        .collect();
                    if !cores.is_empty() {
                        let (extra, _) = crate::fast_linear::recover_proposals(
                            image,
                            &cores,
                            oriented_mask & NON_RETAIL_LINEAR,
                            sampler,
                        )?;
                        found.extend(extra);
                    }
                }
                // Segment voting (and the retail boxes, on its 768-pixel raster) can miss thin
                // and partial symbols, such as rows of small labels on a large sheet. Frames
                // without a linear read fall back to the stripe boxes (the sparse search; from
                // High up also the full search's other boxes, so High reads what Medium reads
                // there); reads of 2D formats do not stop it. With retail
                // selected, only frames above twice that raster take it. Large label sheets
                // carry many short Code 128 fields: one read there also takes the stripe boxes.
                let linear_read = reads.iter().chain(found.iter()).any(is_linear_read);
                let large = is_large_frame(image.width, image.height);
                let label_sheet = is_label_sheet_sized(image.width, image.height)
                    && reads
                        .iter()
                        .chain(found.iter())
                        .any(|read| read.format == "Code128");
                if (primary.is_none() || large)
                    && oriented_mask != 0
                    && (!linear_read || label_sheet)
                {
                    let im = crate::ImageView::new(
                        image.data,
                        image.width,
                        image.height,
                        image.channels,
                        image.stride,
                    )?;
                    let stripes = self.localizer.detect_sparse(im, 768.)?.proposals;
                    let full = if POLICY.full_stripe_fallback {
                        self.localizer.detect(im)?.proposals
                    } else {
                        Vec::new()
                    };
                    // Each list keeps its own proposal limit and decoding budgets.
                    for boxes in [stripes, full] {
                        if boxes.is_empty() {
                            continue;
                        }
                        let (extra, regions) = crate::fast_linear::recover_proposals_hinted(
                            image,
                            &boxes,
                            oriented_mask,
                            sampler,
                            &mut hinted,
                        )?;
                        found.extend(extra);
                        pending.extend(regions);
                    }
                }
                // Low confirms thin or rotated symbols whose rows decoded too rarely here: its
                // fast path spends four times the samples on dense confirmation rows. The first
                // such boxes that no read covers are retried with its budgets.
                let unread_hints = |reads: &[Read], found: &[Read]| -> Vec<crate::Quad> {
                    hinted
                        .iter()
                        .filter(|polygon| {
                            !reads
                                .iter()
                                .chain(found)
                                .any(|read| overlap_quads(&read.polygon, polygon).0 > 0.)
                        })
                        .take(UNREAD_BOX_SWEEPS)
                        .copied()
                        .collect()
                };
                let retries: Vec<_> = unread_hints(reads, &found)
                    .into_iter()
                    .map(|polygon| crate::Proposal { polygon, score: 1. })
                    .collect();
                if !retries.is_empty() {
                    let (extra, _) = crate::fast_linear::recover_proposals_patient(
                        image,
                        &retries,
                        oriented_mask,
                        sampler,
                    )?;
                    found.extend(extra);
                }
                // With Codabar or Code 93 selected, Low sweeps the whole image for every selected
                // linear format; that reader decodes symbols whose rows the profiles decode too
                // rarely to accept. Linear-only frames still unread here run that reader on the
                // first boxes with such rows, for the formats not swept before. Sweeping the
                // whole image or the first unread boxes cost Medium 3-14% on them in Chrome.
                if primary.is_none()
                    && oriented_mask & LINEAR_MASK & !COMMON_LINEAR_MASK & !SWEPT != 0
                    && !reads.iter().chain(found.iter()).any(is_linear_read)
                {
                    let preserved = if POLICY.preserved_sweep {
                        ITF | CODE128
                    } else {
                        0
                    };
                    for polygon in hinted.iter().take(UNREAD_BOX_SWEEPS) {
                        found.extend(sweep_box(
                            image,
                            polygon,
                            oriented_mask & LINEAR_MASK & !SWEPT & !preserved,
                            &mut self.additional_gray,
                        )?);
                    }
                }
                // With retail selected, that reader runs on the first unread boxes with such
                // rows that no read covers, also on frames with other reads, for the non-retail
                // linear formats. Medium leaves out Code 128 and Code 39 on large frames, where
                // its whole-image sweep reads them; High and Very High keep them, so they read
                // the fields such crops give Medium.
                if primary.is_some()
                    && oriented_mask & LINEAR_MASK & !COMMON_LINEAR_MASK & !SWEPT != 0
                {
                    let preserved =
                        if POLICY.preserved_sweep && large && !POLICY.preserved_sweep_any_size {
                            CODE128 | CODE39
                        } else {
                            0
                        };
                    for polygon in unread_hints(reads, &found) {
                        // A crop of a partly covered ITF can yield a short fragment of it;
                        // ITF payloads shorter than six digits are not kept from these crops.
                        found.extend(
                            sweep_box(
                                image,
                                &polygon,
                                oriented_mask & NON_RETAIL_LINEAR & !preserved,
                                &mut self.additional_gray,
                            )?
                            .into_iter()
                            .filter(|read| read.format != "ITF" || read.text.len() >= 6),
                        );
                    }
                }
                // The localizer works on a 768-pixel raster: in large frames small
                // symbols fall below its minimum group size. Localize again at half
                // scale (High and Very High), then on overlapping windows (Very High),
                // and decode only the new proposals. Very High reads what High reads plus
                // what the windows add. Medium keeps its cost on large retail photos.
                if POLICY.half_scale_proposals && oriented_mask != 0 && large {
                    let mut windowed = half_scale_proposals(&mut self.localizer, image, proposals)?;
                    if POLICY.window_proposals {
                        let known: Vec<_> = proposals.iter().chain(&windowed).copied().collect();
                        windowed.extend(windowed_proposals(&mut self.localizer, image, &known)?);
                    }
                    if !windowed.is_empty() {
                        let (extra, regions) = crate::fast_linear::recover_proposals(
                            image,
                            &windowed,
                            oriented_mask,
                            sampler,
                        )?;
                        found.extend(extra);
                        pending.extend(regions);
                    }
                }
                if upce_hypothesis {
                    found.retain(|read| {
                        read.format != "UPCE" || read.support >= UPCE_HYPOTHESIS_SUPPORT
                    });
                }
                reads.extend(found);
                // Batch frames (shelves, cartons) of many EAN-13 or UPC-A symbols lose the
                // smaller ones on the 768-pixel raster: once a large frame read eight, Medium
                // also decodes the boxes of one half-scale raster (High upscales crops).
                if POLICY.retail_batch_half_scale
                    && mask & PRIMARY_RETAIL != 0
                    && large
                    && reads
                        .iter()
                        .filter(|read| matches!(read.format.as_str(), "EAN13" | "UPCA"))
                        .count()
                        >= RETAIL_BATCH_READS
                {
                    let boxes = half_scale_proposals(&mut self.localizer, image, proposals)?;
                    let (extra, _) = crate::fast_linear::recover_proposals(
                        image,
                        &boxes,
                        mask & RETAIL_MASK,
                        sampler,
                    )?;
                    for read in extra {
                        if !reads
                            .iter()
                            .any(|old| overlap_quads(&old.polygon, &read.polygon).0 > 0.)
                        {
                            reads.push(read);
                        }
                    }
                }
                // Independent short-code source hypothesis, bounded to unresolved proposals.
                if reads.is_empty() && mask & SHORT_RETAIL != 0 {
                    let unresolved: Vec<_> = proposals.iter().take(4).copied().collect();
                    let (mut extra, regions) = crate::fast_linear::recover_proposals_as(
                        image,
                        &unresolved,
                        mask & SHORT_RETAIL,
                        sampler,
                        crate::fast_linear::SourceProfile::Gray(1.5),
                    )?;
                    if POLICY.short_retail_withholds_repeats {
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
                if reads.is_empty() && mask & RETAIL_MASK != 0 {
                    let selected: Vec<_> = proposals.iter().take(4).copied().collect();
                    let (extra, regions) = crate::fast_linear::recover_proposals_as(
                        image,
                        &selected,
                        mask & RETAIL_MASK,
                        sampler,
                        crate::fast_linear::SourceProfile::Inverted,
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
                let recovery_mask = mask & (CODE128 | CODE39 | ITF | CODE93);
                if recovery_mask != 0 {
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
                        .take(POLICY.checksum_free_recovery_proposals)
                        .copied()
                        .collect();
                    let (extra, regions) = crate::fast_linear::recover_proposals_as(
                        image,
                        &unresolved,
                        recovery_mask,
                        sampler,
                        crate::fast_linear::SourceProfile::Gray(2.),
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
                            recovery_mask & CODE93 != 0,
                        );
                    }
                    if options.include_regions {
                        pending.extend(regions);
                    }
                }
                #[cfg(any(feature = "high", feature = "very-high"))]
                if mask & PRIMARY_RETAIL != 0 {
                    let bands = crate::detail::source_bands(image, reads);
                    let (extra, regions) = crate::fast_linear::recover_proposals(
                        image,
                        &bands,
                        mask & PRIMARY_RETAIL,
                        sampler,
                    )?;
                    reads.extend(extra);
                    if options.include_regions {
                        unread.extend(regions);
                    }
                }
                #[cfg(feature = "medium")]
                if mask & SHORT_RETAIL != 0 && image.width.max(image.height) > 768 {
                    crate::short_crop::recover(
                        &mut self.localizer,
                        image,
                        proposals,
                        mask,
                        sampler,
                        reads,
                    )?;
                }
                if mask & UPCE != 0 && mask != UPCE && reads.iter().any(is_linear_read) {
                    // Retry proposals that do not intersect a current read;
                    // keep proposal and unread lists intact.
                    let uncovered: Vec<_> = proposals
                        .iter()
                        .filter(|proposal| {
                            !reads
                                .iter()
                                .any(|read| overlap_quads(&read.polygon, &proposal.polygon).0 > 0.)
                        })
                        .take(2)
                        .copied()
                        .collect();
                    if !uncovered.is_empty() {
                        for read in
                            crate::fast_linear::recover_uncovered_upce(image, &uncovered, sampler)?
                        {
                            if !reads
                                .iter()
                                .any(|old| overlap_quads(&read.polygon, &old.polygon).0 > 0.)
                            {
                                reads.push(read);
                            }
                        }
                    }
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
}

#[cfg(test)]
mod central_band_tests {
    use super::central_band;

    fn assert_close(actual: crate::Quad, expected: crate::Quad) {
        for (a, e) in actual.iter().flatten().zip(expected.iter().flatten()) {
            assert!((a - e).abs() < 1e-9, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn keeps_the_scan_axis_and_the_middle_of_the_bars() {
        let along = [[0., 0.], [100., 0.], [100., 20.], [0., 20.]];
        assert_close(
            central_band(&along),
            [[0., 6.], [100., 6.], [100., 14.], [0., 14.]],
        );
        let across = [[0., 0.], [20., 0.], [20., 100.], [0., 100.]];
        assert_close(
            central_band(&across),
            [[6., 0.], [14., 0.], [14., 100.], [6., 100.]],
        );
    }
}

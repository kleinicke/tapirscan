use super::effort::SELECTED;
use super::{Error, Image, ImageView, Proposal, Quad, Result, ScanOptions, Scanner};
use barcode_research_core::{frame::Barcode, multi_scan::Policy, shear, stripes};

#[cfg(not(feature = "low"))]
mod restoration;
#[cfg(any(feature = "medium", feature = "low"))]
pub(crate) mod segment_voting;
mod source_evidence;
#[cfg(not(feature = "low"))]
use restoration::recover_restored_regions;
#[cfg(any(feature = "high", feature = "very-high"))]
use restoration::recover_threshold_regions;
#[cfg(feature = "medium")]
use restoration::{recover_late_wide_crop, recover_scaled_crop};
#[cfg(feature = "medium")]
use source_evidence::independently_confirmed_ean_reads;
#[cfg(any(feature = "medium", feature = "low"))]
pub(crate) use source_evidence::recovered_ean_source_agreement;
pub(crate) use source_evidence::source_contradiction;

struct Localization {
    proposals: Vec<Proposal>,
    /// Medium: alternative boxes per segment-voting cluster, tried only while it stays unread.
    #[cfg(feature = "medium")]
    alternatives: Vec<(Quad, Vec<Quad>)>,
    short_fragments: Option<Vec<Proposal>>,
    omitted: usize,
    work_limited: bool,
    search_window: Quad,
}

pub(super) fn scan(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    consolidate: bool,
    shared_retail: bool,
) -> std::result::Result<Result, Error> {
    #[cfg(feature = "low")]
    let _ = coverage;
    checked_image(image)?;
    if shared_retail && (image.channels != 4 || image.stride != image.width * 4) {
        let mut packed = std::mem::take(&mut scanner.retail_rgba);
        retail_pixels(image, &mut packed);
        let result = scan_prepared(
            scanner,
            Image {
                data: &packed,
                width: image.width,
                height: image.height,
                channels: 4,
                stride: image.width * 4,
            },
            options,
            coverage,
            consolidate,
            shared_retail,
        );
        scanner.retail_rgba = packed;
        return result;
    }
    scan_prepared(
        scanner,
        image,
        options,
        coverage,
        consolidate,
        shared_retail,
    )
}

fn scan_prepared(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    consolidate: bool,
    shared_retail: bool,
) -> std::result::Result<Result, Error> {
    scan_prepared_impl(
        scanner,
        image,
        options,
        coverage,
        consolidate,
        shared_retail,
        true,
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep the ordered scan stages, including Medium segment-voting alternatives, together."
)]
fn scan_prepared_impl(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    consolidate: bool,
    shared_retail: bool,
    allow_restoration: bool,
) -> std::result::Result<Result, Error> {
    let im = checked_image(image)?;
    let localization = localize(&mut scanner.localizer, im, image, shared_retail)?;
    let mut candidates: Vec<_> = localization.proposals.iter().map(|p| p.polygon).collect();
    candidates.push(localization.search_window);
    let policy = scan_policy(image, &localization.proposals, coverage, options);

    #[cfg(feature = "medium")]
    scanner
        .regions
        .retail_configure(if shared_retail { 15 } else { 1 })?;
    #[cfg(not(feature = "medium"))]
    let _ = shared_retail;
    let mut scan = scanner.regions.scan(im, &candidates, policy)?;
    #[cfg(feature = "medium")]
    let mut retail = finish_retail(&mut scan, &mut scanner.regions, im);
    #[cfg(feature = "medium")]
    scan_alternatives(
        scanner,
        image,
        im,
        options,
        coverage,
        &localization,
        &mut scan,
        &mut retail,
    )?;
    #[cfg(not(feature = "medium"))]
    let mut retail = Vec::new();
    #[cfg(not(feature = "low"))]
    let recovery = recover(
        scanner,
        image,
        &mut scan,
        coverage,
        options,
        shared_retail,
        &retail,
    )?;
    #[cfg(feature = "low")]
    let recovery: Option<super::read::Recovery> = None;
    if let Some(recovery) = &recovery {
        retail.extend(recovery.retail.iter().cloned());
    }

    #[cfg(feature = "medium")]
    recover_primary_proposals(
        image,
        &localization.proposals,
        &mut scan,
        &mut scanner.fast_profiles,
    )?;
    #[cfg(feature = "medium")]
    if scan.frame.barcodes.is_empty() && retail.is_empty() {
        recover_deferred_proposals(
            image,
            &localization.proposals,
            &mut scan,
            &mut scanner.fast_profiles,
        )?;
    }
    #[cfg(any(feature = "high", feature = "very-high"))]
    recover_primary_proposals_high(
        image,
        &localization.proposals,
        &mut scan,
        &mut scanner.fast_profiles,
    )?;
    #[cfg(any(feature = "high", feature = "very-high"))]
    if allow_restoration {
        recover_threshold_regions(scanner, image, &localization.proposals, &mut scan)?;
    }
    // Preserve the shared Retail/Common budget in Medium. Source-region
    // restoration remains available for EAN-only scans or explicit completion.
    #[cfg(not(feature = "low"))]
    if allow_restoration
        && (!cfg!(feature = "medium")
            || !shared_retail
            || options.finish_candidates
            || scan.frame.barcodes.is_empty())
    {
        recover_restored_regions(
            scanner,
            image,
            options,
            coverage,
            &localization.proposals,
            &mut scan,
            cfg!(feature = "medium") && shared_retail && !options.finish_candidates,
        )?;
    }
    #[cfg(feature = "medium")]
    if allow_restoration && scan.frame.barcodes.is_empty() && retail.is_empty() {
        recover_unread_crops(
            scanner,
            image,
            options,
            coverage,
            &localization.proposals,
            &mut scan,
        )?;
    }
    #[cfg(feature = "low")]
    let _ = allow_restoration;
    #[cfg(feature = "medium")]
    drop_short_ghosts(&scan, &mut retail);
    #[cfg(feature = "medium")]
    merge_cross_list_duplicates(&scan, &mut retail);
    finish_primary(&mut scan, image, consolidate, options.multiple);
    Ok(Result {
        proposals: localization.proposals,
        short_fragments: localization.short_fragments,
        localization_omitted: localization.omitted,
        localization_work_limited: localization.work_limited,
        search_window: localization.search_window,
        scan,
        options,
        recovery,
        retail,
    })
}

/// Late Medium crop retries for frames without any read: a wide-restored crop,
/// then area-reduced crops of large unresolved symbols.
#[cfg(feature = "medium")]
fn recover_unread_crops(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
) -> std::result::Result<(), Error> {
    recover_late_wide_crop(scanner, image, options, coverage, proposals, scan)?;
    if scan.frame.barcodes.is_empty() {
        recover_scaled_crop(scanner, image, options, coverage, proposals, scan)?;
    }
    Ok(())
}

#[cfg(not(feature = "low"))]
fn protected_primary(image: Image<'_>, reads: &[Barcode]) -> Vec<Barcode> {
    let mut protected = reads
        .iter()
        .map(|b| Barcode {
            detection: b.detection.clone(),
            candidate_indices: b.candidate_indices.clone(),
        })
        .collect::<Vec<_>>();
    super::linear_duplicates::merge_primary(&mut protected, image);
    protected
}

#[cfg(not(feature = "low"))]
fn admit_primary_reads(
    image: Image<'_>,
    scan: &mut super::ScanResult,
    mut protected: Vec<Barcode>,
    reads: impl IntoIterator<Item = super::read::Read>,
) -> std::result::Result<(), Error> {
    for r in reads {
        if r.support < 3
            || protected
                .iter()
                .chain(&scan.frame.barcodes)
                .any(|b| super::geometry::overlap_quads(&r.polygon, &b.detection.polygon).0 >= 0.65)
        {
            continue;
        }
        let bytes = r.text.as_bytes();
        if bytes.len() == 13 && bytes.iter().all(u8::is_ascii_digit) {
            let digits = std::array::from_fn(|i| bytes[i] - b'0');
            // A new same-value read intersecting existing ownership is not an
            // independently established instance. Preserve the existing read;
            // separated physical copies remain eligible for recovery.
            if protected.iter().chain(&scan.frame.barcodes).any(|b| {
                b.detection.digits == digits
                    && super::geometry::overlap_quads(&r.polygon, &b.detection.polygon).0 > 0.
            }) {
                continue;
            }
            let barcode = Barcode {
                detection: barcode_research_core::candidate_scanner::Detection {
                    digits,
                    polygon: r.polygon,
                    support: usize::try_from(r.support).map_err(|_| Error::Parameters)?,
                    axis: r.axis.unwrap_or(0),
                },
                candidate_indices: Vec::new(),
            };
            // These are optional hypotheses, not existing frame ownership. A
            // second same-value band needs a separated measured extent before
            // it can be admitted as another physical symbol. Keep original
            // frame reads and their geometry untouched.
            let owners = protected_primary(image, std::slice::from_ref(&barcode));
            if owners.iter().any(|new| {
                protected.iter().any(|old| {
                    old.detection.digits == new.detection.digits
                        && super::geometry::overlap_quads(
                            &new.detection.polygon,
                            &old.detection.polygon,
                        )
                        .0 > 0.
                })
            }) {
                continue;
            }
            protected.extend(owners);
            scan.frame.barcodes.push(barcode);
        }
    }
    Ok(())
}

/// Unresolved primary proposals tried with the wide high-pass profile hypothesis.
#[cfg(feature = "medium")]
const HIGHPASS_PROPOSALS: usize = 8;

/// Decode up to eight unresolved EAN-13/UPC-A proposals from wide high-pass
/// profiles (blurred, ghosted or unevenly lit photographs). Restored reads need
/// payload agreement with unmodified source rows, or five supporting rows, and
/// must not be contradicted by the source.
#[cfg(feature = "medium")]
fn recover_highpass_proposals(
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &super::ScanResult,
    protected: &[Barcode],
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<Vec<super::read::Read>, Error> {
    let selected: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame
                .candidates
                .get(*i)
                .is_some_and(|c| c.detections.is_empty())
                && !protected.iter().any(|b| {
                    super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
                })
        })
        .map(|(_, p)| *p)
        .take(HIGHPASS_PROPOSALS)
        .collect();
    let (reads, _) =
        super::fast_linear::recover_proposals_highpass(image, &selected, 1, sampler, 1.5)?;
    let mut accepted = Vec::new();
    for r in reads {
        let bytes = r.text.as_bytes();
        if bytes.len() != 13 || !bytes.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let digits = std::array::from_fn(|i| bytes[i] - b'0');
        if (r.support >= 5
            || source_evidence::recovered_ean_source_agreement(image, r.polygon, &digits))
            && !source_contradiction(image, r.polygon, digits, sampler)?
        {
            accepted.push(r);
        }
    }
    Ok(accepted)
}

/// Retry bounded unresolved source proposals with independent row confirmation.
/// Protect reconciled original ownership before adding a new physical read.
#[cfg(feature = "medium")]
fn recover_primary_proposals(
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<(), Error> {
    let protected = protected_primary(image, &scan.frame.barcodes);
    let all_proposals = proposals;
    let mut unresolved: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame.candidates.get(*i).is_some_and(|c| {
                (c.work.guard_pass > 0
                    || (c.work.invalid_visual_seen == 0
                        && c.work.invalid_veto_reads == 0
                        && (p.score >= 0.95 || c.work.forward_blur_windows >= 4)))
                    && c.detections.is_empty()
            }) && !protected
                .iter()
                .any(|b| super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3)
        })
        .collect();
    unresolved.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let dense_eligible: Vec<_> = unresolved.iter().take(2).map(|(_, p)| **p).collect();
    let proposals: Vec<_> = unresolved.into_iter().take(2).map(|(_, p)| *p).collect();
    let (extra, _) = super::fast_linear::recover_proposals(image, &proposals, 1, sampler)?;
    let (restored, _) =
        super::fast_linear::recover_proposals_contrast(image, &proposals, 1, sampler, 1.0)?;
    let (colored, _) = super::fast_linear::recover_proposals_color(
        image,
        &proposals[..proposals.len().min(2)],
        1,
        sampler,
        2,
    )?;
    let highpass = recover_highpass_proposals(image, all_proposals, scan, &protected, sampler)?;
    admit_primary_reads(
        image,
        scan,
        protected,
        extra
            .into_iter()
            .chain(restored)
            .chain(colored)
            .chain(highpass),
    )?;
    if !scan.frame.barcodes.is_empty() {
        return Ok(());
    }
    let dense_proposals = dense_eligible;
    let (extra, _) =
        super::fast_linear::recover_proposals_wide(image, &dense_proposals, 1, sampler)?;
    let mut accepted = independently_confirmed_ean_reads(image, extra, sampler)?;
    if accepted.is_empty() {
        let extra =
            super::fast_linear::recover_proposals_band(image, &dense_proposals, 1, sampler, 1.5)?.0;
        accepted = independently_confirmed_ean_reads(image, extra, sampler)?;
    }
    let protected = protected_primary(image, &scan.frame.barcodes);
    admit_primary_reads(image, scan, protected, accepted)
}

/// Retry bounded unresolved source proposals with independent row confirmation.
/// Protect reconciled original ownership before adding a new physical read.
#[cfg(any(feature = "high", feature = "very-high"))]
fn recover_primary_proposals_high(
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<(), Error> {
    let protected = protected_primary(image, &scan.frame.barcodes);
    let mut unresolved: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame.candidates.get(*i).is_some_and(|c| {
                (c.work.guard_pass > 0
                    || (c.work.invalid_visual_seen == 0
                        && c.work.invalid_veto_reads == 0
                        && (p.score >= 0.95 || c.work.forward_blur_windows >= 4)))
                    && c.detections.is_empty()
            }) && !protected
                .iter()
                .any(|b| super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3)
        })
        .collect();
    unresolved.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let proposals: Vec<_> = unresolved.into_iter().take(6).map(|(_, p)| *p).collect();
    let (extra, _) = super::fast_linear::recover_proposals(image, &proposals, 1, sampler)?;
    let (colored, _) = super::fast_linear::recover_proposals_color(
        image,
        &proposals[..proposals.len().min(2)],
        1,
        sampler,
        2,
    )?;
    admit_primary_reads(image, scan, protected, extra.into_iter().chain(colored))
}

fn localize(
    detector: &mut stripes::Detector,
    im: ImageView<'_>,
    image: Image<'_>,
    capture_short_fragments: bool,
) -> std::result::Result<Localization, Error> {
    #[cfg(not(feature = "medium"))]
    let mut fragments = Vec::new();
    // Medium localizes by bar-segment voting: one box per symbol, alternatives kept aside.
    #[cfg(feature = "medium")]
    let voted = segment_voting::localize(image);
    #[cfg(feature = "medium")]
    let found = stripes::Result {
        proposals: voted.proposals,
        omitted: 0,
        limited: false,
        trace: [0; 13],
    };
    #[cfg(not(feature = "medium"))]
    let found = if capture_short_fragments {
        use barcode_research_core::numeric::usize_f64;
        let scale = (768. / usize_f64(image.width.max(image.height))).min(1.);
        let sx = usize_f64(image.width) / (usize_f64(image.width) * scale).round().max(3.);
        let sy = usize_f64(image.height) / (usize_f64(image.height) * scale).round().max(3.);
        detector.detect_with_observer(im, |g| {
            let [u0, u1, v0, v1] = g.bounds;
            let (w, h) = (u1 - u0, v1 - v0);
            if g.reason != "narrow" || w < 24. || h < 5. || w / h < 2. || g.edges < 120 {
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
            fragments.push((g.edges, Proposal { polygon, score: 0. }));
            // Stable ties preserve the original collect/sort/take ordering.
            fragments.sort_by_key(|(edges, _)| std::cmp::Reverse(*edges));
            fragments.truncate(4);
        })?
    } else {
        detector.detect(im)?
    };
    #[cfg(feature = "medium")]
    let short_fragments = capture_short_fragments.then(Vec::new);
    #[cfg(not(feature = "medium"))]
    let short_fragments =
        capture_short_fragments.then(|| fragments.into_iter().map(|(_, p)| p).collect());
    let count = found.proposals.len();
    let examined = count.min(SELECTED.fit_limit);
    let mut proposals = found.proposals;
    for i in 0..examined {
        if let Some(p) = shear::refine(im, proposals[i].polygon) {
            proposals.push(p);
        }
    }

    // Small strongly sheared labels can have no accepted orthogonal stripe box.
    // Reuse the bounded pixel-only envelope fit before abandoning localization.
    #[cfg(not(feature = "low"))]
    if count == 0 && image.width * image.height <= 262_144 && detector.has_stripe_evidence() {
        let w = f64::from(u32::try_from(image.width - 1).map_err(|_| Error::Parameters)?);
        let h = f64::from(u32::try_from(image.height - 1).map_err(|_| Error::Parameters)?);
        for q in [
            [[0., 0.], [w, 0.], [w, h], [0., h]],
            [[0., h], [0., 0.], [w, 0.], [w, h]],
        ] {
            if let Some(p) = shear::refine_sparse(im, q) {
                proposals.push(p);
            }
        }
    }
    let (secondary_omitted, secondary_limited) =
        add_secondary_proposals(detector, im, image, &mut proposals);
    if proposals.len() > SELECTED.proposal_limit {
        return Err(Error::Parameters);
    }
    let right = f64::from(u32::try_from(image.width - 1).map_err(|_| Error::Parameters)?);
    let bottom = f64::from(u32::try_from(image.height - 1).map_err(|_| Error::Parameters)?);
    Ok(Localization {
        proposals,
        #[cfg(feature = "medium")]
        alternatives: voted.alternatives,
        short_fragments,
        omitted: found.omitted + count - examined + secondary_omitted,
        work_limited: found.limited
            || count > examined
            || secondary_limited
            || secondary_omitted > 0,
        search_window: [[0., 0.], [right, 0.], [right, bottom], [0., bottom]],
    })
}

fn add_secondary_proposals(
    detector: &mut stripes::Detector,
    im: ImageView<'_>,
    image: Image<'_>,
    proposals: &mut Vec<Proposal>,
) -> (usize, bool) {
    if !cfg!(feature = "very-high") || image.width.max(image.height) <= 640 {
        return (0, false);
    }
    // At most eight alternate base proposals plus four shear refinements. Keep
    // the entire original prefix; no code value or GT selects this extra grid.
    let Ok(second) = detector.detect_secondary(im) else {
        return (0, true);
    };
    let secondary_count = second.proposals.len();
    let selected = secondary_count.min(8);
    let mut omitted = second.omitted + secondary_count - selected;
    let extra: Vec<_> = second.proposals.into_iter().take(selected).collect();
    for p in &extra {
        proposals.push(Proposal {
            polygon: p.polygon,
            score: p.score,
        });
    }
    for p in extra.iter().take(4) {
        if let Some(p) = shear::refine(im, p.polygon) {
            proposals.push(p);
        }
    }
    omitted += selected.saturating_sub(4);
    (omitted, second.limited)
}

fn scan_policy(
    image: Image<'_>,
    proposals: &[Proposal],
    coverage: &[Quad],
    options: ScanOptions,
) -> Policy {
    #[cfg(feature = "low")]
    let _ = (image, proposals, coverage);
    #[cfg(not(feature = "low"))]
    let (retry_mask, low_resolution_mask) = super::detail::retry_masks(image, proposals);
    Policy {
        complete: options.finish_candidates,
        #[cfg(not(feature = "low"))]
        candidate_retry_mask: super::formats::uncovered_mask(proposals, coverage, retry_mask),
        #[cfg(not(feature = "low"))]
        low_resolution_mask: super::formats::uncovered_mask(
            proposals,
            coverage,
            low_resolution_mask,
        ),
        transition_cleanup: true,
        source_identity: true,
        interior_normalization: true,
        guard_bias: true,
        ..Policy::default()
    }
}

#[cfg(feature = "medium")]
fn finish_retail(
    scan: &mut super::ScanResult,
    regions: &mut super::RegionScanner,
    im: ImageView<'_>,
) -> Vec<super::read::Read> {
    let Some(retail) = regions.retail_finish_typed(im, &scan.frame, false) else {
        return Vec::new();
    };
    scan.frame.unfinished |= retail.unfinished;
    retail
        .detections
        .into_iter()
        .map(|d| super::read::Read::retail(d.digits, d.polygon, d.support))
        .collect()
}

#[cfg(not(feature = "low"))]
fn recover(
    scanner: &mut Scanner,
    image: Image<'_>,
    scan: &mut super::ScanResult,
    coverage: &[Quad],
    options: ScanOptions,
    shared_retail: bool,
    retail: &[super::read::Read],
) -> std::result::Result<Option<super::read::Recovery>, Error> {
    let result = super::detail::recover(
        image,
        &mut scan.frame.barcodes,
        &mut scanner.recovery,
        coverage,
        retail,
        super::detail::RecoveryOptions {
            directions: SELECTED.recovery_directions,
            complete: options.finish_candidates,
            shared_retail,
            diagnostics: options.retain_diagnostics,
        },
    )?;
    scan.frame.unfinished = true;
    Ok(Some(result))
}

fn retail_pixels(image: Image<'_>, pixels: &mut Vec<u8>) {
    // Preserve packed RGBA interpolation while retaining storage across frames.
    pixels.resize(image.width * image.height * 4, 255);
    let rows = image
        .data
        .chunks(image.stride)
        .zip(pixels.chunks_exact_mut(image.width * 4));
    for (source, target) in rows {
        let target = target.chunks_exact_mut(4);
        if image.channels == 1 {
            for (&gray, out) in source[..image.width].iter().zip(target) {
                out.copy_from_slice(&[gray, gray, gray, 255]);
            }
        } else {
            let source = source[..image.width * image.channels].chunks_exact(image.channels);
            for (pixel, out) in source.zip(target) {
                out.copy_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
            }
        }
    }
}

fn checked_image(image: Image<'_>) -> std::result::Result<ImageView<'_>, Error> {
    if image.width < 3 || image.height < 3 {
        return Err(Error::Parameters);
    }
    ImageView::new(
        image.data,
        image.width,
        image.height,
        image.channels,
        image.stride,
    )
}

pub(super) fn select_one(reads: &mut Vec<Barcode>) {
    // A strict improvement preserves the first result on equal support.
    let mut best = 0;
    for i in 1..reads.len() {
        if reads[i].detection.support > reads[best].detection.support {
            best = i;
        }
    }
    if !reads.is_empty() {
        let selected = reads.remove(best);
        reads.clear();
        reads.push(selected);
    }
}

#[cfg(feature = "medium")]
fn recover_deferred_proposals(
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<(), Error> {
    let mut eligible: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame.candidates.get(*i).is_some_and(|c| {
                (c.work.guard_pass > 0
                    || (c.work.invalid_visual_seen == 0
                        && c.work.invalid_veto_reads == 0
                        && (p.score >= 0.95 || c.work.forward_blur_windows >= 4)))
                    && c.detections.is_empty()
            })
        })
        .collect();
    eligible.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let already: Vec<_> = eligible.iter().take(2).map(|(i, _)| *i).collect();
    let mut deferred: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            !already.contains(i)
                && p.score >= 0.75
                && scan
                    .frame
                    .candidates
                    .get(*i)
                    .is_some_and(|c| c.detections.is_empty())
        })
        .collect();
    deferred.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
    let selected: Vec<_> = deferred.into_iter().take(2).map(|(_, p)| *p).collect();
    let extra = super::fast_linear::recover_proposals_band(image, &selected, 1, sampler, 0.75)?.0;
    let extra = super::linear_duplicates::merge(extra, image);
    if extra.len() != 1 {
        return Ok(());
    }
    let mut accepted = Vec::new();
    for r in extra {
        let bytes = r.text.as_bytes();
        if r.support < 4 || bytes.len() != 13 || !bytes.iter().all(u8::is_ascii_digit) {
            continue;
        }
        let digits = std::array::from_fn(|i| bytes[i] - b'0');
        if recovered_ean_source_agreement(image, r.polygon, &digits)
            && !source_contradiction(image, r.polygon, digits, sampler)?
        {
            accepted.push(r);
        }
    }
    let protected = protected_primary(image, &scan.frame.barcodes);
    admit_primary_reads(image, scan, protected, accepted)
}

fn finish_primary(
    scan: &mut super::ScanResult,
    image: Image<'_>,
    consolidate: bool,
    multiple: bool,
) {
    if consolidate {
        super::linear_duplicates::merge_primary(&mut scan.frame.barcodes, image);
    }
    if !multiple {
        select_one(&mut scan.frame.barcodes);
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, not(feature = "low")))]
mod recovery_tests;

#[cfg(all(test, feature = "medium"))]
mod recovered_source_tests;

#[cfg(all(test, any(feature = "medium", feature = "low")))]
mod independent_source_rows_tests;

/// Try alternative boxes of segment-voting clusters whose own box produced no read but whose
/// candidate showed retail evidence (guard patterns or blurred-symbol windows). Reads from
/// alternative geometry must not be contradicted by the source image.
#[cfg(feature = "medium")]
#[allow(clippy::too_many_arguments)] // one ordered pipeline transaction over shared scan state
fn scan_alternatives(
    scanner: &mut Scanner,
    image: Image<'_>,
    im: ImageView<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    localization: &Localization,
    scan: &mut super::ScanResult,
    retail: &mut Vec<super::read::Read>,
) -> std::result::Result<(), Error> {
    use barcode_research_core::numeric::usize_f64;
    let (mx, my) = (usize_f64(image.width - 1), usize_f64(image.height - 1));
    let mut quads = Vec::new();
    for (primary, alternatives) in &localization.alternatives {
        let evidence = localization
            .proposals
            .iter()
            .position(|p| p.polygon == *primary)
            .and_then(|i| scan.frame.candidates.get(i))
            .is_some_and(|c| c.work.guard_pass > 0 || c.work.forward_blur_windows >= 2);
        if !evidence {
            continue;
        }
        let decoded = scan
            .frame
            .barcodes
            .iter()
            .any(|b| super::geometry::overlap_quads(&b.detection.polygon, primary).0 > 0.3)
            || retail
                .iter()
                .any(|r| super::geometry::overlap_quads(&r.polygon, primary).0 > 0.3);
        if decoded {
            continue;
        }
        if let Some(r) = shear::refine(im, *primary) {
            quads.push(r.polygon);
        }
        quads.extend(
            alternatives
                .iter()
                .map(|q| q.map(|p| [p[0].clamp(0., mx), p[1].clamp(0., my)])),
        );
    }
    quads.retain(|q| {
        let a: f64 = (0..4)
            .map(|k| q[k][0] * q[(k + 1) % 4][1] - q[(k + 1) % 4][0] * q[k][1])
            .sum();
        a.abs() > 16.
    });
    // Region scans track per-candidate retries in 64-bit masks.
    quads.truncate(24);
    if quads.is_empty() {
        return Ok(());
    }
    let props: Vec<Proposal> = quads
        .iter()
        .map(|q| Proposal {
            polygon: *q,
            score: 0.9,
        })
        .collect();
    let policy = scan_policy(image, &props, coverage, options);
    let mut extra = scanner.regions.scan(im, &quads, policy)?;
    let extra_retail = finish_retail(&mut extra, &mut scanner.regions, im);
    for mut b in extra.frame.barcodes {
        if source_contradiction(
            image,
            b.detection.polygon,
            b.detection.digits,
            &mut scanner.fast_profiles,
        )? {
            continue;
        }
        if !scan.frame.barcodes.iter().any(|o| {
            o.detection.digits == b.detection.digits
                && super::geometry::overlap_quads(&o.detection.polygon, &b.detection.polygon).0 > 0.
        }) {
            b.candidate_indices.clear();
            scan.frame.barcodes.push(b);
        }
    }
    for r in extra_retail {
        if !retail.iter().any(|o| {
            o.text == r.text && super::geometry::overlap_quads(&o.polygon, &r.polygon).0 > 0.
        }) {
            retail.push(r);
        }
    }
    Ok(())
}

/// Drop UPC-E/EAN-8 reads centred inside an EAN-13/UPC-A read: part-symbol ghosts.
#[cfg(feature = "medium")]
fn drop_short_ghosts(scan: &super::ScanResult, retail: &mut Vec<super::read::Read>) {
    let long: Vec<Quad> = scan
        .frame
        .barcodes
        .iter()
        .map(|b| b.detection.polygon)
        .chain(
            retail
                .iter()
                .filter(|r| matches!(r.format.as_str(), "EAN13" | "UPCA"))
                .map(|r| r.polygon),
        )
        .collect();
    retail.retain(|r| {
        if !matches!(r.format.as_str(), "UPCE" | "EAN8") {
            return true;
        }
        let c = [
            r.polygon.iter().map(|p| p[0]).sum::<f64>() / 4.,
            r.polygon.iter().map(|p| p[1]).sum::<f64>() / 4.,
        ];
        !long.iter().any(|q| super::formats::contains_point(c, q))
    });
}

/// One read per symbol across the primary and retail lists: drop a retail read when a primary
/// read or a better-supported retail read with the same payload (UPC-A as EAN-13) lies within
/// half the longer read's long side and the two overlap or one is a fragment (less than half the
/// other's short side). Separate equal symbols stay apart.
#[cfg(feature = "medium")]
fn merge_cross_list_duplicates(scan: &super::ScanResult, retail: &mut Vec<super::read::Read>) {
    let centre = |q: &Quad| {
        [
            q.iter().map(|p| p[0]).sum::<f64>() / 4.,
            q.iter().map(|p| p[1]).sum::<f64>() / 4.,
        ]
    };
    let side =
        |q: &Quad, k: usize| (q[k][0] - q[(k + 1) % 4][0]).hypot(q[k][1] - q[(k + 1) % 4][1]);
    let long = |q: &Quad| (0..4).map(|k| side(q, k)).fold(0., f64::max);
    let short = |q: &Quad| (0..4).map(|k| side(q, k)).fold(f64::INFINITY, f64::min);
    let same_symbol = |a: &Quad, b: &Quad| {
        super::geometry::overlap_quads(a, b).0 > 0.
            || short(a) < 0.5 * short(b)
            || short(b) < 0.5 * short(a)
            || super::geometry::adjacent_strips(a, b)
    };
    let normalized = |t: &str| {
        if t.len() == 12 {
            format!("0{t}")
        } else {
            t.to_string()
        }
    };
    let primary: Vec<(String, [f64; 2], f64, Quad)> = scan
        .frame
        .barcodes
        .iter()
        .map(|b| {
            let text = b
                .detection
                .digits
                .iter()
                .map(|d| char::from(b'0' + d))
                .collect();
            (
                text,
                centre(&b.detection.polygon),
                long(&b.detection.polygon),
                b.detection.polygon,
            )
        })
        .collect();
    let mut order: Vec<usize> = (0..retail.len()).collect();
    order.sort_by(|&a, &b| retail[b].support.cmp(&retail[a].support));
    let mut keep = vec![true; retail.len()];
    let mut kept: Vec<usize> = Vec::new();
    for &i in &order {
        let text = normalized(&retail[i].text);
        let (c, l) = (centre(&retail[i].polygon), long(&retail[i].polygon));
        let near = |c2: [f64; 2], l2: f64| (c[0] - c2[0]).hypot(c[1] - c2[1]) < 0.5 * l.max(l2);
        let q = retail[i].polygon;
        let duplicate = primary
            .iter()
            .any(|(t, pc, pl, pq)| *t == text && near(*pc, *pl) && same_symbol(pq, &q))
            || kept.iter().any(|&k| {
                normalized(&retail[k].text) == text
                    && near(centre(&retail[k].polygon), long(&retail[k].polygon))
                    && same_symbol(&retail[k].polygon, &q)
            });
        if duplicate {
            keep[i] = false;
        } else {
            kept.push(i);
        }
    }
    let mut index = 0;
    retail.retain(|_| {
        index += 1;
        keep[index - 1]
    });
}

use super::effort::SELECTED;
#[cfg(not(feature = "low"))]
use super::fast_linear::SourceProfile;
use super::{Error, Image, ImageView, Proposal, Quad, Result, ScanOptions, Scanner};
use barcode_research_core::{frame::Barcode, multi_scan::Policy, shear, stripes};

#[cfg(not(feature = "low"))]
mod deblur;
#[cfg(not(feature = "low"))]
mod restoration;
pub(crate) mod segment_voting;
mod source_evidence;
#[cfg(not(feature = "low"))]
use restoration::recover_restored_regions;
#[cfg(not(feature = "low"))]
use restoration::{recover_late_wide_crop, recover_scaled_crop};
#[cfg(not(feature = "low"))]
use source_evidence::independently_confirmed_ean_reads;
pub(crate) use source_evidence::recovered_ean_source_agreement;
pub(crate) use source_evidence::source_contradiction;

struct Localization {
    proposals: Vec<Proposal>,
    /// Alternative boxes per segment-voting cluster, tried only while it stays unread.
    #[cfg(not(feature = "low"))]
    alternatives: Vec<(Quad, Vec<Quad>)>,
    omitted: usize,
    work_limited: bool,
    search_window: Quad,
    /// Whether some boxes, fewer than were kept, were dropped as textures. Scanned, they
    /// could have shown the retail evidence that gates later recoveries, which then also
    /// searched the kept boxes and the whole frame; dropping them must not remove that work,
    /// so they count as such evidence. On frames where most boxes are textures, the
    /// recoveries would mostly search textures.
    #[cfg(not(feature = "low"))]
    stray_textures: bool,
    /// Segment-voting boxes beyond the proposal limit; on a sheet of symbols, those that
    /// are no texture are scanned.
    #[cfg(not(feature = "low"))]
    sheet: Vec<Quad>,
}

/// Linear reads of the first boxes that mark a sheet of symbols, whose further boxes are
/// then scanned too.
#[cfg(not(feature = "low"))]
const SHEET_READS: usize = 8;
/// Further boxes scanned together.
#[cfg(not(feature = "low"))]
const SHEET_BATCH: usize = 24;

/// Per-scan decisions carried through recursive crop recovery, never stored on Scanner.
#[derive(Clone, Copy)]
pub(crate) struct ScanContext {
    pub options: ScanOptions,
    pub consolidate: bool,
    pub shared_retail: bool,
    /// Other linear formats share this scan: Medium retail crops decode only their box.
    #[cfg(not(feature = "low"))]
    pub shared_linear: bool,
}

impl ScanContext {
    /// Crop rescans retain format context but decode only primary retail formats.
    #[cfg(not(feature = "low"))]
    fn for_crop(self, consolidate: bool) -> Self {
        Self {
            consolidate,
            shared_retail: false,
            ..self
        }
    }
}

pub(super) fn scan(
    scanner: &mut Scanner,
    image: Image<'_>,
    context: ScanContext,
    coverage: &[Quad],
) -> std::result::Result<Result, Error> {
    #[cfg(feature = "low")]
    let _ = coverage;
    checked_image(image)?;
    if context.shared_retail && image.channels == 1 {
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
            context,
            coverage,
        );
        scanner.retail_rgba = packed;
        return result;
    }
    scan_prepared(scanner, image, context, coverage)
}

fn scan_prepared(
    scanner: &mut Scanner,
    image: Image<'_>,
    context: ScanContext,
    coverage: &[Quad],
) -> std::result::Result<Result, Error> {
    scan_prepared_impl(scanner, image, context, coverage, TextureSource::of(image))
}

/// Original pixels behind a scanned image: point `p` of the scanned image lies at
/// `origin + p / scale` in `image`. Recovery rescans of reduced or restored crops test
/// textures on original pixels, where reduction cannot alias them.
#[derive(Clone, Copy)]
pub(super) struct TextureSource<'a> {
    image: Image<'a>,
    origin: [f64; 2],
    scale: f64,
    /// The scanned image is the whole frame, which alone runs the restoration retries.
    frame: bool,
}

impl<'a> TextureSource<'a> {
    pub(super) fn of(image: Image<'a>) -> Self {
        Self {
            image,
            origin: [0., 0.],
            scale: 1.,
            frame: true,
        }
    }

    /// A recovery rescan of a crop or reduced copy of `image`; Low has no such rescans.
    #[cfg(not(feature = "low"))]
    pub(super) fn crop(image: Image<'a>, origin: [f64; 2], scale: f64) -> Self {
        Self {
            image,
            origin,
            scale,
            frame: false,
        }
    }

    pub(super) fn texture(&self, q: &Quad) -> bool {
        let q = q.map(|p| {
            [
                self.origin[0] + p[0] / self.scale,
                self.origin[1] + p[1] / self.scale,
            ]
        });
        super::texture::texture_proposal(self.image, &q)
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "Keep the ordered scan stages, including Medium segment-voting alternatives, together."
)]
fn scan_prepared_impl(
    scanner: &mut Scanner,
    image: Image<'_>,
    context: ScanContext,
    coverage: &[Quad],
    source: TextureSource<'_>,
) -> std::result::Result<Result, Error> {
    let ScanContext {
        options,
        consolidate,
        shared_retail,
        ..
    } = context;
    let allow_restoration = source.frame;
    let im = checked_image(image)?;
    let localization = localize(&mut scanner.localizer, im, image, source)?;
    let mut candidates: Vec<_> = localization.proposals.iter().map(|p| p.polygon).collect();
    // Medium scans the full-frame window of a whole image only when its boxes read nothing but
    // show evidence (below). Crop rescans are small and always scan theirs.
    let window_first = cfg!(not(feature = "medium")) || !allow_restoration;
    if window_first {
        candidates.push(localization.search_window);
    }
    #[cfg(feature = "medium")]
    let mut window_scanned = window_first;
    #[cfg(not(feature = "medium"))]
    let window_scanned = window_first;
    let policy = scan_policy(image, &localization.proposals, coverage, options);

    #[cfg(not(feature = "low"))]
    scanner
        .regions
        .retail_configure(if shared_retail { 15 } else { 1 })?;
    #[cfg(feature = "low")]
    let _ = shared_retail;
    let mut scan = scanner.regions.scan(im, &candidates, policy)?;
    #[cfg(not(feature = "low"))]
    let mut retail = finish_retail(&mut scan, &mut scanner.regions, im);
    // A sheet of symbols read through many of its first boxes: scan its further boxes.
    #[cfg(not(feature = "low"))]
    if allow_restoration && scan.frame.barcodes.len() + retail.len() >= SHEET_READS {
        let sheet: Vec<Quad> = localization
            .sheet
            .iter()
            .copied()
            .filter(|q| !source.texture(q))
            .collect();
        for batch in sheet.chunks(SHEET_BATCH) {
            retail.extend(scan_extra_boxes(
                scanner,
                im,
                batch,
                policy,
                shared_retail,
                &mut scan,
            )?);
        }
    }
    // A frame whose boxes read nothing but show retail evidence (guard patterns or blurred
    // symbol windows) also scans the full-frame window, after the segment-voting
    // alternatives, detail recovery and primary-box profiles, which cost less and read many
    // such frames first. Other linear symbols sharing the scan show stray guards, so they need
    // repeated guard passes.
    #[cfg(feature = "medium")]
    let window_evidence = allow_restoration
        && scan.frame.barcodes.is_empty()
        && retail.is_empty()
        && (localization.stray_textures
            || scan.frame.candidates.iter().any(|c| {
                if context.shared_linear {
                    c.work.guard_pass >= 4
                } else {
                    c.work.guard_pass > 0 || c.work.forward_blur_windows >= 2
                }
            }));
    #[cfg(not(feature = "low"))]
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
    #[cfg(feature = "low")]
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
        source,
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
    if window_evidence && scan.frame.barcodes.is_empty() && retail.is_empty() {
        window_scanned = true;
        retail = scan_unread_window(
            scanner,
            im,
            localization.search_window,
            policy,
            shared_retail,
            &mut scan,
        )?;
    }
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
    // Medium's contrast, high-pass, wide and band profile recoveries also apply here, but only
    // to frames High's own retries left unread: frames with a read pay nothing. Other linear
    // symbols sharing the scan show stray guards, so they need repeated guard passes.
    #[cfg(any(feature = "high", feature = "very-high"))]
    let retail_unread = scan.frame.barcodes.is_empty()
        && (!context.shared_linear
            || localization.stray_textures
            || scan.frame.candidates.iter().any(|c| c.work.guard_pass >= 4));
    #[cfg(any(feature = "high", feature = "very-high"))]
    if retail_unread {
        recover_primary_proposals(
            image,
            &localization.proposals,
            &mut scan,
            &mut scanner.fast_profiles,
        )?;
    }
    #[cfg(any(feature = "high", feature = "very-high"))]
    if retail_unread && scan.frame.barcodes.is_empty() {
        recover_deferred_proposals(
            image,
            &localization.proposals,
            &mut scan,
            &mut scanner.fast_profiles,
        )?;
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
            context,
            coverage,
            &localization.proposals,
            &mut scan,
            cfg!(feature = "medium") && shared_retail && !options.finish_candidates,
        )?;
    }
    // Each crop rescans the whole High pipeline. Retry only frames with repeated guard passes
    // (Medium's threshold for its shared full-frame rescan), and not when other linear formats
    // share the scan: there most frames are retail-unread.
    #[cfg(any(feature = "high", feature = "very-high"))]
    if allow_restoration
        && scan.frame.barcodes.is_empty()
        && !context.shared_linear
        && scan.frame.candidates.iter().any(|c| c.work.guard_pass >= 4)
    {
        recover_unread_crops(
            scanner,
            image,
            context,
            coverage,
            &localization.proposals,
            &mut scan,
        )?;
    }
    #[cfg(feature = "medium")]
    if allow_restoration && scan.frame.barcodes.is_empty() && retail.is_empty() {
        recover_unread_crops(
            scanner,
            image,
            context,
            coverage,
            &localization.proposals,
            &mut scan,
        )?;
    }
    // Blurred but well localized symbols: deblurred upright crops of the strongest boxes.
    #[cfg(feature = "medium")]
    let unread = scan.frame.barcodes.is_empty() && retail.is_empty();
    #[cfg(any(feature = "high", feature = "very-high"))]
    let unread = scan.frame.barcodes.is_empty();
    #[cfg(not(feature = "low"))]
    if allow_restoration && unread {
        deblur::recover_deblurred_crops(
            scanner,
            image,
            options,
            &localization.proposals,
            &mut scan,
        )?;
        if scan.frame.barcodes.is_empty() {
            recover_downscaled(scanner, image, context, coverage, &mut scan)?;
        }
    }
    #[cfg(feature = "low")]
    let _ = allow_restoration;
    #[cfg(not(feature = "low"))]
    drop_short_ghosts(&scan, &mut retail);
    #[cfg(not(feature = "low"))]
    merge_cross_list_duplicates(&scan, &mut retail);
    finish_primary(&mut scan, image, consolidate, options.multiple);
    Ok(Result {
        proposals: localization.proposals,
        localization_omitted: localization.omitted,
        localization_work_limited: localization.work_limited,
        search_window: window_scanned.then_some(localization.search_window),
        scan,
        options,
        recovery,
        retail,
    })
}

/// Late crop retries for frames without any read: a wide-restored crop,
/// then area-reduced crops of large unresolved symbols.
#[cfg(not(feature = "low"))]
fn recover_unread_crops(
    scanner: &mut Scanner,
    image: Image<'_>,
    context: ScanContext,
    coverage: &[Quad],
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
) -> std::result::Result<(), Error> {
    recover_late_wide_crop(scanner, image, context.options, coverage, proposals, scan)?;
    if scan.frame.barcodes.is_empty() {
        recover_scaled_crop(scanner, image, context, coverage, proposals, scan)?;
    }
    Ok(())
}

/// Large frames whose boxes read nothing but show retail evidence: decoding can fail at one
/// sampling scale only, so rescan the whole frame reduced to the localizer raster and keep
/// source-confirmed EAN-13/UPC-A reads.
#[cfg(not(feature = "low"))]
fn recover_downscaled(
    scanner: &mut Scanner,
    image: Image<'_>,
    context: ScanContext,
    coverage: &[Quad],
    scan: &mut super::ScanResult,
) -> std::result::Result<(), Error> {
    use barcode_research_core::numeric::usize_f64;
    let long = image.width.max(image.height);
    if long < 1600
        || !scan
            .frame
            .candidates
            .iter()
            .any(|c| c.work.guard_pass > 0 || c.work.forward_blur_windows >= 2)
    {
        return Ok(());
    }
    let factor = 768. / usize_f64(long);
    let (pixels, w, h) = restoration::reduce_gray(image, factor);
    let reduced = Image {
        data: &pixels,
        width: w,
        height: h,
        channels: 1,
        stride: w,
    };
    let scaled: Vec<Quad> = coverage
        .iter()
        .map(|q| q.map(|v| [v[0] * factor, v[1] * factor]))
        .collect();
    let retry = scan_prepared_impl(
        scanner,
        reduced,
        context.for_crop(false),
        &scaled,
        TextureSource::crop(image, [0., 0.], factor),
    )?;
    scan.frame.unfinished |= retry.scan.frame.unfinished;
    for mut read in retry.scan.frame.barcodes {
        if read.detection.support < 3 {
            continue;
        }
        read.detection.polygon = read
            .detection
            .polygon
            .map(|p| [p[0] / factor, p[1] / factor]);
        if scan
            .frame
            .barcodes
            .iter()
            .any(|old| old.detection.digits == read.detection.digits)
            || !recovered_ean_source_agreement(
                image,
                read.detection.polygon,
                &read.detection.digits,
            )
            || source_contradiction(
                image,
                read.detection.polygon,
                read.detection.digits,
                &mut scanner.fast_profiles,
            )?
        {
            continue;
        }
        read.candidate_indices.clear();
        scan.frame.barcodes.push(read);
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

/// Unresolved primary proposals with barcode evidence tried with the ghosting hypothesis.
#[cfg(not(feature = "low"))]
const DEGHOST_PROPOSALS: usize = 4;

/// Unresolved primary proposals tried with the wide high-pass profile hypothesis.
#[cfg(not(feature = "low"))]
const HIGHPASS_PROPOSALS: usize = 8;

/// Decode up to eight unresolved EAN-13/UPC-A proposals from wide high-pass
/// profiles (blurred, ghosted or unevenly lit photographs). Restored reads need
/// payload agreement with unmodified source rows, or five supporting rows, and
/// must not be contradicted by the source.
#[cfg(not(feature = "low"))]
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
            scan.frame.candidates.get(*i).is_some_and(|c| {
                // High and Very High retry only boxes with retail evidence, keeping frames
                // without a symbol cheap; Medium retries every unread box.
                c.detections.is_empty()
                    && (cfg!(feature = "medium")
                        || c.work.guard_pass > 0
                        || c.work.forward_blur_windows >= 2)
            }) && !protected
                .iter()
                .any(|b| super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3)
        })
        .map(|(_, p)| *p)
        .take(HIGHPASS_PROPOSALS)
        .collect();
    let (mut reads, _) = super::fast_linear::recover_proposals_as(
        image,
        &selected,
        1,
        sampler,
        SourceProfile::Highpass(1.5),
    )?;
    // Camera shake doubles every bar; undo a ghost on either side for boxes with evidence.
    if reads.is_empty() {
        let shaken: Vec<_> = selected
            .iter()
            .filter(|p| {
                proposals
                    .iter()
                    .position(|q| q.polygon == p.polygon)
                    .and_then(|i| scan.frame.candidates.get(i))
                    .is_some_and(|c| c.work.guard_pass > 0 || c.work.forward_blur_windows >= 2)
            })
            .take(DEGHOST_PROPOSALS)
            .copied()
            .collect();
        for backward in [false, true] {
            if reads.is_empty() && !shaken.is_empty() {
                reads = super::fast_linear::recover_proposals_as(
                    image,
                    &shaken,
                    1,
                    sampler,
                    SourceProfile::Deghost(backward),
                )?
                .0;
            }
        }
    }
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

/// Evidence shared by primary profile retries and the deferred-proposal exclusion.
#[cfg(not(feature = "low"))]
fn recovery_evidence(
    proposal: &Proposal,
    candidate: Option<&barcode_research_core::candidate_scanner::Candidate>,
) -> bool {
    candidate.is_some_and(|c| {
        (c.work.guard_pass > 0
            || (c.work.invalid_visual_seen == 0
                && c.work.invalid_veto_reads == 0
                && (proposal.score >= 0.95 || c.work.forward_blur_windows >= 4)))
            && c.detections.is_empty()
    })
}

/// Retry bounded unresolved source proposals with independent row confirmation.
/// Protect reconciled original ownership before adding a new physical read.
#[cfg(not(feature = "low"))]
fn recover_primary_proposals(
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<(), Error> {
    #[cfg(feature = "medium")]
    {
        // Only profile retries share this transaction; recursive crop scans start later.
        sampler.with_source_cache(checked_image(image)?, true, |sampler| {
            recover_primary_profiles(image, proposals, scan, sampler)
        })
    }
    #[cfg(not(feature = "medium"))]
    {
        recover_primary_profiles(image, proposals, scan, sampler)
    }
}

#[cfg(not(feature = "low"))]
fn recover_primary_profiles(
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
            recovery_evidence(p, scan.frame.candidates.get(*i))
                && !protected.iter().any(|b| {
                    super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
                })
        })
        .collect();
    unresolved.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let proposals: Vec<_> = unresolved.into_iter().take(2).map(|(_, p)| *p).collect();
    let (extra, _) = super::fast_linear::recover_proposals(image, &proposals, 1, sampler)?;
    let (restored, _) = super::fast_linear::recover_proposals_as(
        image,
        &proposals,
        1,
        sampler,
        SourceProfile::Gray(1.0),
    )?;
    let (colored, _) = super::fast_linear::recover_proposals_as(
        image,
        &proposals[..proposals.len().min(2)],
        1,
        sampler,
        SourceProfile::Color(2),
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
    let (extra, _) = super::fast_linear::recover_proposals_as(
        image,
        &proposals,
        1,
        sampler,
        SourceProfile::WideEan,
    )?;
    let mut accepted = independently_confirmed_ean_reads(image, extra, sampler)?;
    if accepted.is_empty() {
        let extra = super::fast_linear::recover_proposals_as(
            image,
            &proposals,
            1,
            sampler,
            SourceProfile::BandEan(1.5),
        )?
        .0;
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
            recovery_evidence(p, scan.frame.candidates.get(*i))
                && !protected.iter().any(|b| {
                    super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
                })
        })
        .collect();
    unresolved.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let proposals: Vec<_> = unresolved.into_iter().take(6).map(|(_, p)| *p).collect();
    let (extra, _) = super::fast_linear::recover_proposals(image, &proposals, 1, sampler)?;
    let (colored, _) = super::fast_linear::recover_proposals_as(
        image,
        &proposals[..proposals.len().min(2)],
        1,
        sampler,
        SourceProfile::Color(2),
    )?;
    admit_primary_reads(image, scan, protected, extra.into_iter().chain(colored))
}

fn localize(
    detector: &mut stripes::Detector,
    im: ImageView<'_>,
    image: Image<'_>,
    source: TextureSource<'_>,
) -> std::result::Result<Localization, Error> {
    // Medium localizes by bar-segment voting: one box per symbol, alternatives kept aside.
    #[cfg(feature = "medium")]
    let mut voted = segment_voting::localize(image);
    #[cfg(feature = "medium")]
    let sheet: Vec<Quad> = std::mem::take(&mut voted.more)
        .into_iter()
        .map(|p| p.polygon)
        .collect();
    #[cfg(feature = "medium")]
    let found = stripes::Result {
        proposals: voted.proposals,
        omitted: 0,
        limited: false,
        trace: [0; 13],
    };
    #[cfg(not(feature = "medium"))]
    // High and Very High box symbols by segment voting, so they skip the stripe pass except
    // on tiny frames.
    let found = if cfg!(any(feature = "high", feature = "very-high")) && !tiny_stripes(image) {
        stripes::Result {
            proposals: Vec::new(),
            omitted: 0,
            limited: false,
            trace: [0; 13],
        }
    } else {
        detector.detect(im)?
    };
    #[cfg(any(feature = "high", feature = "very-high"))]
    let (found, alternatives, sheet) = high_boxes(found, image);
    // Boxes on stripes, rings, random bar patterns and similar textures can hold no symbol;
    // retrying them costs most of the scan of such frames.
    let mut found = found;
    #[cfg(not(feature = "low"))]
    let localized = found.proposals.len();
    found.proposals.retain(|p| !source.texture(&p.polygon));
    let count = found.proposals.len();
    #[allow(
        clippy::unnecessary_min_or_max,
        reason = "fit_limit is a per-mode constant; Low's 0 makes the bound trivial only in that mode."
    )]
    let examined = count.min(SELECTED.fit_limit);
    let mut proposals = found.proposals;
    for i in 0..examined {
        if let Some(p) = shear::refine(im, proposals[i].polygon) {
            proposals.push(p);
        }
    }
    // Refinements of kept boxes cover the same bars and need no texture test.
    let refined = proposals.len();
    let (secondary_omitted, secondary_limited) =
        add_secondary_proposals(detector, im, image, &mut proposals);
    // Secondary boxes get the same texture test.
    let mut index = 0;
    #[cfg(not(feature = "low"))]
    let added = proposals.len();
    proposals.retain(|p| {
        index += 1;
        index <= refined || !source.texture(&p.polygon)
    });
    #[cfg(not(feature = "low"))]
    let stray_textures = {
        let dropped = localized - count + added - proposals.len();
        dropped > 0 && dropped < proposals.len()
    };
    if proposals.len() > SELECTED.proposal_limit {
        return Err(Error::Parameters);
    }
    let right = f64::from(u32::try_from(image.width - 1).map_err(|_| Error::Parameters)?);
    let bottom = f64::from(u32::try_from(image.height - 1).map_err(|_| Error::Parameters)?);
    Ok(Localization {
        proposals,
        #[cfg(feature = "medium")]
        alternatives: voted.alternatives,
        #[cfg(any(feature = "high", feature = "very-high"))]
        alternatives,
        omitted: found.omitted + count - examined + secondary_omitted,
        work_limited: found.limited
            || count > examined
            || secondary_limited
            || secondary_omitted > 0,
        search_window: [[0., 0.], [right, 0.], [right, bottom], [0., bottom]],
        #[cfg(not(feature = "low"))]
        stray_textures,
        #[cfg(not(feature = "low"))]
        sheet,
    })
}

/// High and Very High also box symbols by segment voting; on tiny frames the stripe pass
/// still supplies stripe evidence and boxes.
#[cfg(any(feature = "high", feature = "very-high"))]
fn high_boxes(
    mut found: stripes::Result,
    image: Image<'_>,
) -> (stripes::Result, Vec<(Quad, Vec<Quad>)>, Vec<Quad>) {
    let mut voted = segment_voting::localize(image);
    let sheet = std::mem::take(&mut voted.more)
        .into_iter()
        .map(|p| p.polygon)
        .collect();
    if tiny_stripes(image) {
        // Voting has few segments on tiny frames; keep distinct stripe boxes after its boxes.
        let mut proposals = voted.proposals;
        for p in std::mem::take(&mut found.proposals) {
            if !proposals
                .iter()
                .any(|q| super::geometry::overlap_quads(&p.polygon, &q.polygon).0 >= 0.9)
            {
                proposals.push(p);
            }
        }
        proposals.truncate(SELECTED.proposal_limit - SELECTED.fit_limit - 12);
        (
            stripes::Result { proposals, ..found },
            voted.alternatives,
            sheet,
        )
    } else {
        (
            stripes::Result {
                proposals: voted.proposals,
                omitted: 0,
                limited: false,
                trace: found.trace,
            },
            voted.alternatives,
            sheet,
        )
    }
}

/// Frames below half the voting raster also run the stripe localizer in High and Very High.
#[cfg(not(feature = "medium"))]
fn tiny_stripes(image: Image<'_>) -> bool {
    image.width.max(image.height) < 384
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

/// Scans the full-frame window of a frame whose boxes read nothing and appends it to `scan`
/// as the candidate after the boxes, as if it had been scanned with them. Rescanning the
/// unread boxes would repeat their first pass, so only the window is scanned, with the
/// window's own retry selection from the frame policy. Retail evidence is grouped per
/// candidate and the boxes' groups assembled nothing, so it restarts with the window alone.
#[cfg(feature = "medium")]
fn scan_unread_window(
    scanner: &mut Scanner,
    im: ImageView<'_>,
    window: Quad,
    policy: Policy,
    shared_retail: bool,
    scan: &mut super::ScanResult,
) -> std::result::Result<Vec<super::read::Read>, Error> {
    let index = scan.frame.candidates.len();
    let bit = |mask: u64| u64::from(index < 64 && mask >> index & 1 == 1);
    let policy = Policy {
        candidate_retry_mask: bit(policy.candidate_retry_mask),
        low_resolution_mask: bit(policy.low_resolution_mask),
        ..policy
    };
    scanner
        .regions
        .retail_configure(if shared_retail { 15 } else { 1 })?;
    let mut window = scanner.regions.scan(im, &[window], policy)?;
    let retail = finish_retail(&mut window, &mut scanner.regions, im);
    for mut candidate in window.frame.candidates {
        candidate.index += index;
        scan.frame.candidates.push(candidate);
    }
    for mut barcode in window.frame.barcodes {
        for i in &mut barcode.candidate_indices {
            *i += index;
        }
        scan.frame.barcodes.push(barcode);
    }
    for mut error in window.errors {
        error.candidate_index += index;
        scan.errors.push(error);
    }
    scan.frame.unfinished |= window.frame.unfinished;
    Ok(retail)
}

/// Scan `boxes` after the first candidates, without per-box retries, and append the
/// results to `scan`.
#[cfg(not(feature = "low"))]
fn scan_extra_boxes(
    scanner: &mut Scanner,
    im: ImageView<'_>,
    boxes: &[Quad],
    policy: Policy,
    shared_retail: bool,
    scan: &mut super::ScanResult,
) -> std::result::Result<Vec<super::read::Read>, Error> {
    let index = scan.frame.candidates.len();
    let policy = Policy {
        candidate_retry_mask: 0,
        low_resolution_mask: 0,
        ..policy
    };
    scanner
        .regions
        .retail_configure(if shared_retail { 15 } else { 1 })?;
    let mut extra = scanner.regions.scan(im, boxes, policy)?;
    let retail = finish_retail(&mut extra, &mut scanner.regions, im);
    for mut candidate in extra.frame.candidates {
        candidate.index += index;
        scan.frame.candidates.push(candidate);
    }
    for mut barcode in extra.frame.barcodes {
        for i in &mut barcode.candidate_indices {
            *i += index;
        }
        scan.frame.barcodes.push(barcode);
    }
    for mut error in extra.errors {
        error.candidate_index += index;
        scan.errors.push(error);
    }
    scan.frame.unfinished |= extra.frame.unfinished;
    Ok(retail)
}

#[cfg(not(feature = "low"))]
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
#[expect(
    clippy::too_many_arguments,
    reason = "Forwards the scan state and the original-pixel texture source to detail recovery."
)]
fn recover(
    scanner: &mut Scanner,
    image: Image<'_>,
    scan: &mut super::ScanResult,
    coverage: &[Quad],
    options: ScanOptions,
    shared_retail: bool,
    retail: &[super::read::Read],
    source: TextureSource<'_>,
) -> std::result::Result<Option<super::read::Recovery>, Error> {
    // The frame's own seed crops already ran before Medium rescans a crop or reduced copy;
    // repeating them there read nothing new. From High up they can still add row support.
    if cfg!(feature = "medium") && !source.frame {
        return Ok(None);
    }
    let result = super::detail::recover(
        image,
        &mut scan.frame.barcodes,
        &mut scanner.recovery,
        &mut scanner.recovery_scratch,
        coverage,
        retail,
        super::detail::RecoveryOptions {
            texture: &|q| source.texture(q),
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
    // Grayscale keeps RGB interpolation and color-profile eligibility.
    // Color inputs already provide identical channel-aware samples without packing.
    pixels.resize(image.width * image.height * 4, 255);
    let rows = image
        .data
        .chunks(image.stride)
        .zip(pixels.chunks_exact_mut(image.width * 4));
    for (source, target) in rows {
        let target = target.as_chunks_mut::<4>().0;
        for (&gray, out) in source[..image.width].iter().zip(target) {
            *out = [gray, gray, gray, 255];
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

#[cfg(not(feature = "low"))]
fn recover_deferred_proposals(
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<(), Error> {
    let mut eligible: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| recovery_evidence(p, scan.frame.candidates.get(*i)))
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
    let extra = super::fast_linear::recover_proposals_as(
        image,
        &selected,
        1,
        sampler,
        SourceProfile::BandEan(0.75),
    )?
    .0;
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
#[cfg(not(feature = "low"))]
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
#[cfg(not(feature = "low"))]
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
#[cfg(not(feature = "low"))]
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

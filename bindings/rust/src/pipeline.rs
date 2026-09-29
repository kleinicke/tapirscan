use super::effort::SELECTED;
use super::{Error, Image, ImageView, Proposal, Quad, Result, ScanOptions, Scanner};
use barcode_research_core::{frame::Barcode, multi_scan::Policy, shear, stripes};

struct Localization {
    proposals: Vec<Proposal>,
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
    #[cfg(feature = "low")]
    let _ = allow_restoration;
    if consolidate {
        super::linear_duplicates::merge_primary(&mut scan.frame.barcodes, image);
    }
    if !options.multiple {
        select_one(&mut scan.frame.barcodes);
    }
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
                detection: barcode_research_core::experiment::Detection {
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
    admit_primary_reads(
        image,
        scan,
        protected,
        extra.into_iter().chain(restored).chain(colored),
    )?;
    if !scan.frame.barcodes.is_empty() {
        return Ok(());
    }
    let dense_proposals = dense_eligible;
    let (extra, _) =
        super::fast_linear::recover_proposals_wide(image, &dense_proposals, 1, sampler)?;
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
    let mut fragments = Vec::new();
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
    for y in 0..image.height {
        for x in 0..image.width {
            let source = y * image.stride + x * image.channels;
            let target = (y * image.width + x) * 4;
            pixels[target] = image.data[source];
            pixels[target + 1] = image.data[source + usize::from(image.channels != 1)];
            pixels[target + 2] = image.data[source + 2 * usize::from(image.channels != 1)];
            pixels[target + 3] = 255;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retail_storage_reuse_preserves_stride_channels_and_alpha() {
        let mut storage = Vec::new();
        for (width, height) in [(12, 8), (3, 4), (12, 8)] {
            for channels in [1, 3, 4] {
                let stride = width * channels + 7;
                let mut data = vec![37; stride * height];
                let mut expected = Vec::new();
                for y in 0..height {
                    for x in 0..width {
                        let source = y * stride + x * channels;
                        let value = u8::try_from(x + y).unwrap();
                        data[source] = value;
                        if channels != 1 {
                            data[source + 1] = value + 1;
                            data[source + 2] = value + 2;
                        }
                        expected.extend_from_slice(&[
                            value,
                            value + u8::from(channels != 1),
                            value + 2 * u8::from(channels != 1),
                            255,
                        ]);
                    }
                }
                data.truncate((height - 1) * stride + width * channels);
                retail_pixels(
                    Image {
                        data: &data,
                        width,
                        height,
                        channels,
                        stride,
                    },
                    &mut storage,
                );
                assert_eq!(storage, expected);
            }
        }
    }
}

#[cfg(not(feature = "low"))]
fn restore_source_image(image: Image<'_>) -> Vec<u8> {
    let (w, h) = (image.width, image.height);
    let gray: Vec<u32> = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| {
                let at = y * image.stride + x * image.channels;
                if image.channels == 1 {
                    u32::from(image.data[at])
                } else {
                    (77 * u32::from(image.data[at])
                        + 150 * u32::from(image.data[at + 1])
                        + 29 * u32::from(image.data[at + 2])
                        + 128)
                        / 256
                }
            })
        })
        .collect();
    let weights = [1_u32, 4, 6, 4, 1];
    let mut horizontal = vec![0_u32; w * h];
    for y in 0..h {
        for x in 0..w {
            horizontal[y * w + x] = weights
                .iter()
                .enumerate()
                .map(|(i, a)| a * gray[y * w + x.saturating_add(i).saturating_sub(2).min(w - 1)])
                .sum();
        }
    }
    let mut out = vec![0_u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let smooth: u32 = weights
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    a * horizontal[y.saturating_add(i).saturating_sub(2).min(h - 1) * w + x]
                })
                .sum();
            let value = (5 * i32::try_from(gray[y * w + x]).expect("gray")
                - 3 * i32::try_from((smooth + 128) / 256).expect("smooth")
                + 1)
                / 2;
            out[y * w + x] = u8::try_from(value.clamp(0, 255)).expect("clamped gray");
        }
    }
    out
}

#[cfg(not(feature = "low"))]
fn source_contradiction(
    image: Image<'_>,
    q: Quad,
    expected: [u8; 13],
    sampler: &mut barcode_research_core::fast_profile::Sampler,
) -> std::result::Result<bool, Error> {
    let im = checked_image(image)?;
    let point = |u: f64, v: f64| {
        let a = [
            q[0][0] + (q[1][0] - q[0][0]) * u,
            q[0][1] + (q[1][1] - q[0][1]) * u,
        ];
        let b = [
            q[3][0] + (q[2][0] - q[3][0]) * u,
            q[3][1] + (q[2][1] - q[3][1]) * u,
        ];
        [a[0] + (b[0] - a[0]) * v, a[1] + (b[1] - a[1]) * v]
    };
    let mut conflicts: Vec<([u8; 13], usize)> = Vec::new();
    for v in [0.2, 0.5, 0.8] {
        sampler.sample_limited(im, point(-0.15, v), point(1.15, v), 3., 1536);
        let mut row = Vec::new();
        for method in 0..3 {
            if method == 1 {
                sampler.adaptive_threshold();
            } else {
                sampler.threshold(method == 0);
            }
            for reverse in [false, true] {
                if reverse {
                    sampler.runs.reverse();
                }
                let black = if reverse {
                    sampler.first_black ^ sampler.runs.len().is_multiple_of(2)
                } else {
                    sampler.first_black
                };
                let total = sampler.runs.iter().sum::<f32>();
                for start in (usize::from(!black)..sampler.runs.len().saturating_sub(59)).step_by(2)
                {
                    if start == 0 {
                        continue;
                    }
                    let widths = &sampler.runs[start..start + 59];
                    let left = sampler.runs[..start].iter().sum::<f32>() / total;
                    let symbol_width = widths.iter().sum::<f32>();
                    let right = left + symbol_width / total;
                    if (left - 0.15 / 1.3).abs() > 0.12 || (right - 1.15 / 1.3).abs() > 0.12 {
                        continue;
                    }
                    let module = symbol_width / 95.;
                    if sampler.runs[start - 1] < module * 4.
                        || sampler.runs[start + 59] < module * 4.
                    {
                        continue;
                    }
                    if let Some(e) = barcode_research_core::run_ean::decode_visual_evidence(widths)
                    {
                        if e.digits != expected && !row.contains(&e.digits) {
                            row.push(e.digits);
                        }
                    }
                }
                if reverse {
                    sampler.runs.reverse();
                }
            }
        }
        for digits in row {
            if let Some((_, count)) = conflicts.iter_mut().find(|(d, _)| *d == digits) {
                *count += 1;
                if *count >= 2 {
                    return Ok(true);
                }
            } else {
                conflicts.push((digits, 1));
            }
        }
    }
    Ok(false)
}

#[cfg(not(feature = "low"))]
fn recover_restored_regions(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
    retail_limited: bool,
) -> std::result::Result<(), Error> {
    use barcode_research_core::numeric::usize_f64;
    if !scan
        .frame
        .candidates
        .iter()
        .any(|c| c.work.guard_pass >= 4 && c.detections.is_empty())
    {
        return Ok(());
    }
    let protected = protected_primary(image, &scan.frame.barcodes);
    let mut unresolved: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame
                .candidates
                .get(*i)
                .is_some_and(|c| c.work.guard_pass >= 4 && c.detections.is_empty())
                && !protected.iter().any(|b| {
                    super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
                })
        })
        .collect();
    unresolved.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let mut remaining = if retail_limited {
        262_144
    } else if cfg!(feature = "medium") {
        131_072
    } else {
        262_144
    };
    for (_, p) in unresolved
        .into_iter()
        .take(if retail_limited { 1 } else { 2 })
    {
        let (x, y, w, h) = restoration_bounds(image, p.polygon);
        if w < 3 || h < 3 || w * h > remaining {
            continue;
        }
        remaining -= w * h;
        let crop = Image {
            data: &image.data[y * image.stride + x * image.channels..],
            width: w,
            height: h,
            channels: image.channels,
            stride: image.stride,
        };
        let pixels = restore_source_image(crop);
        let enhanced = Image {
            data: &pixels,
            width: w,
            height: h,
            channels: 1,
            stride: w,
        };
        let crop_coverage: Vec<Quad> = coverage
            .iter()
            .map(|q| q.map(|v| [v[0] - usize_f64(x), v[1] - usize_f64(y)]))
            .collect();
        let retry = scan_prepared_impl(
            scanner,
            enhanced,
            options,
            &crop_coverage,
            true,
            false,
            false,
        )?;
        for mut read in retry.scan.frame.barcodes {
            if read.detection.support < 5 {
                continue;
            }
            for point in &mut read.detection.polygon {
                point[0] += usize_f64(x);
                point[1] += usize_f64(y);
            }
            if super::geometry::overlap_quads(&read.detection.polygon, &p.polygon).0 < 0.3 {
                continue;
            }
            if protected.iter().chain(&scan.frame.barcodes).any(|b| {
                super::geometry::overlap_quads(&read.detection.polygon, &b.detection.polygon).0
                    >= 0.3
            }) {
                continue;
            }
            if source_contradiction(
                image,
                read.detection.polygon,
                read.detection.digits,
                &mut scanner.fast_profiles,
            )? {
                continue;
            }
            read.candidate_indices.clear();
            scan.frame.barcodes.push(read);
        }
    }
    Ok(())
}

#[cfg(not(feature = "low"))]
fn restoration_bounds(image: Image<'_>, polygon: Quad) -> (usize, usize, usize, usize) {
    use barcode_research_core::numeric::{f64_usize, usize_f64};
    let xmin = polygon.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min);
    let xmax = polygon
        .iter()
        .map(|v| v[0])
        .fold(f64::NEG_INFINITY, f64::max);
    let ymin = polygon.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min);
    let ymax = polygon
        .iter()
        .map(|v| v[1])
        .fold(f64::NEG_INFINITY, f64::max);
    let horizontal_padding = (xmax - xmin) * 0.2 + 4.;
    let vertical_padding = (ymax - ymin) * 0.2 + 4.;
    let x = f64_usize(
        (xmin - horizontal_padding)
            .floor()
            .clamp(0., usize_f64(image.width)),
    );
    let y = f64_usize(
        (ymin - vertical_padding)
            .floor()
            .clamp(0., usize_f64(image.height)),
    );
    let right = f64_usize(
        (xmax + horizontal_padding)
            .ceil()
            .clamp(0., usize_f64(image.width)),
    );
    let bottom = f64_usize(
        (ymax + vertical_padding)
            .ceil()
            .clamp(0., usize_f64(image.height)),
    );
    let w = right.saturating_sub(x);
    let h = bottom.saturating_sub(y);
    (x, y, w, h)
}

#[cfg(any(feature = "high", feature = "very-high"))]
fn recover_threshold_regions(
    scanner: &mut Scanner,
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut super::ScanResult,
) -> std::result::Result<(), Error> {
    let protected = protected_primary(image, &scan.frame.barcodes);
    let mut unresolved: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame
                .candidates
                .get(*i)
                .is_some_and(|c| c.work.guard_pass > 0 && c.detections.is_empty())
                && !protected.iter().any(|b| {
                    super::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
                })
        })
        .collect();
    unresolved.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    let regions: Vec<_> = unresolved
        .into_iter()
        .take(2)
        .map(|(_, p)| p.polygon)
        .collect();
    if regions.is_empty() {
        return Ok(());
    }
    let policy = Policy {
        complete: false,
        max_retry_paths_per_candidate: if cfg!(feature = "very-high") { 64 } else { 16 },
        max_retry_paths_per_frame: if cfg!(feature = "very-high") { 128 } else { 32 },
        max_association_checks: 32_768,
        max_association_pixels: 262_144,
        transition_cleanup: true,
        source_identity: true,
        interior_normalization: true,
        guard_bias: true,
        ..Policy::default()
    };
    let retry = scanner
        .regions
        .scan_threshold_recovery(checked_image(image)?, &regions, policy)?;
    scan.frame.unfinished |= retry.frame.unfinished;
    for mut read in retry.frame.barcodes {
        if read.detection.support < 4
            || protected.iter().chain(&scan.frame.barcodes).any(|b| {
                super::geometry::overlap_quads(&read.detection.polygon, &b.detection.polygon).0
                    >= 0.3
            })
        {
            continue;
        }
        if source_contradiction(
            image,
            read.detection.polygon,
            read.detection.digits,
            &mut scanner.fast_profiles,
        )? {
            continue;
        }
        read.candidate_indices.clear();
        scan.frame.barcodes.push(read);
    }
    Ok(())
}

#[cfg(all(test, not(feature = "low")))]
mod recovery_tests {
    use super::*;

    #[test]
    fn region_restoration_preserves_padded_rgb_rgba_and_gray_crops() {
        for (width, height) in [(1, 1), (3, 5), (17, 9)] {
            let gray: Vec<u8> = (0..width * height)
                .map(|i| u8::try_from((i * 73 + 19) % 256).unwrap())
                .collect();
            let expected = restore_source_image(Image {
                data: &gray,
                width,
                height,
                channels: 1,
                stride: width,
            });
            for channels in [1, 3, 4] {
                let stride = (width + 4) * channels + 7;
                let offset = stride + 2 * channels;
                let mut padded = vec![173; offset + (height - 1) * stride + width * channels];
                for y in 0..height {
                    for x in 0..width {
                        for c in 0..channels.min(3) {
                            padded[offset + y * stride + x * channels + c] = gray[y * width + x];
                        }
                        if channels == 4 {
                            padded[offset + y * stride + x * channels + 3] = 11;
                        }
                    }
                }
                assert_eq!(
                    restore_source_image(Image {
                        data: &padded[offset..],
                        width,
                        height,
                        channels,
                        stride,
                    }),
                    expected
                );
            }
        }
        for value in [0, 1, 127, 254, 255] {
            let data = vec![value; 21];
            assert_eq!(
                restore_source_image(Image {
                    data: &data,
                    width: 7,
                    height: 3,
                    channels: 1,
                    stride: 7,
                }),
                data
            );
        }
    }

    #[test]
    fn optional_bands_keep_two_separated_copies_but_not_two_bands_of_one_copy() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let bits = barcode_research_core::ean::encode(&digits);
        let width = 400;
        let height = 100;
        let mut pixels = vec![255; width * height];
        for y in (5..35).chain(55..85) {
            for (module, bit) in bits.iter().enumerate() {
                if *bit > 0.5 {
                    pixels[y * width + 50 + module * 3..y * width + 53 + module * 3].fill(0);
                }
            }
        }
        let image = Image {
            data: &pixels,
            width,
            height,
            channels: 1,
            stride: width,
        };
        let mut scan = super::super::ScanResult {
            frame: barcode_research_core::frame::Frame {
                candidates: Vec::new(),
                barcodes: Vec::new(),
                reconciliation: barcode_research_core::frame::ReconciliationWork::default(),
                unfinished: false,
            },
            errors: Vec::new(),
            candidate_timings_available: false,
        };
        let reads = [(13., 6), (23., 5), (63., 4)].map(|(top, support)| {
            super::super::read::Read::primary(
                digits,
                [[50., top], [335., top], [335., top + 4.], [50., top + 4.]],
                support,
                0,
                Vec::new(),
            )
        });
        admit_primary_reads(image, &mut scan, Vec::new(), reads).unwrap();
        assert_eq!(scan.frame.barcodes.len(), 2);
        assert_eq!(scan.frame.barcodes[0].detection.support, 6);
        assert_eq!(scan.frame.barcodes[1].detection.support, 4);
    }

    #[test]
    fn source_veto_requires_a_conflicting_symbol_at_the_same_location() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let other = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
        let bits = barcode_research_core::ean::encode(&digits);
        let width = 400;
        let height = 40;
        let mut pixels = vec![255; width * height];
        for y in 0..height {
            for (module, bit) in bits.iter().enumerate() {
                if *bit > 0.5 {
                    pixels[y * width + 50 + module * 3..y * width + 53 + module * 3].fill(0);
                }
            }
        }
        let image = Image {
            data: &pixels,
            width,
            height,
            channels: 1,
            stride: width,
        };
        let q = [[50., 0.], [335., 0.], [335., 39.], [50., 39.]];
        let mut sampler = barcode_research_core::fast_profile::Sampler::default();
        assert!(!source_contradiction(image, q, digits, &mut sampler).unwrap());
        assert!(source_contradiction(image, q, other, &mut sampler).unwrap());
        pixels.fill(255);
        let blank = Image {
            data: &pixels,
            width,
            height,
            channels: 1,
            stride: width,
        };
        assert!(!source_contradiction(blank, q, other, &mut sampler).unwrap());
    }
}

#[cfg(feature = "medium")]
fn raw_source_normalize(values: &mut [f64; 256]) -> f64 {
    use barcode_research_core::numeric::usize_f64;
    const N: usize = 256;
    let mean = values.iter().sum::<f64>() / 256.;
    let slope = values
        .iter()
        .enumerate()
        .map(|(i, v)| (usize_f64(i) - 127.5) * (v - mean))
        .sum::<f64>()
        / (0..N).map(|i| (usize_f64(i) - 127.5).powi(2)).sum::<f64>();
    for (i, v) in values.iter_mut().enumerate() {
        *v -= mean + slope * (usize_f64(i) - 127.5);
    }
    let norm = values.iter().map(|v| v * v).sum::<f64>().sqrt();
    if norm > 0. {
        for v in values {
            *v /= norm;
        }
    }
    norm
}
#[cfg(feature = "medium")]
fn raw_source_profiles(image: Image<'_>, q: Quad) -> Vec<([f64; 256], f64)> {
    use barcode_research_core::numeric::{f64_usize, usize_f64};
    let gray = |x: usize, y: usize| {
        let at = y * image.stride + x * image.channels;
        if image.channels == 1 {
            f64::from(image.data[at])
        } else {
            0.299 * f64::from(image.data[at])
                + 0.587 * f64::from(image.data[at + 1])
                + 0.114 * f64::from(image.data[at + 2])
        }
    };
    let pixel = |x: f64, y: f64| {
        if x < 0. || y < 0. || x > usize_f64(image.width - 1) || y > usize_f64(image.height - 1) {
            return 255.;
        }
        let ix = f64_usize(x.floor());
        let iy = f64_usize(y.floor());
        let fx = x - x.floor();
        let fy = y - y.floor();
        let right = (ix + 1).min(image.width - 1);
        let bottom = (iy + 1).min(image.height - 1);
        (gray(ix, iy) * (1. - fx) + gray(right, iy) * fx) * (1. - fy)
            + (gray(ix, bottom) * (1. - fx) + gray(right, bottom) * fx) * fy
    };
    [0.2, 0.5, 0.8]
        .into_iter()
        .map(|v| {
            let a = [
                q[0][0] * (1. - v) + q[3][0] * v,
                q[0][1] * (1. - v) + q[3][1] * v,
            ];
            let b = [
                q[1][0] * (1. - v) + q[2][0] * v,
                q[1][1] * (1. - v) + q[2][1] * v,
            ];
            let mut row = std::array::from_fn(|i| {
                let u = -0.02 + 1.04 * usize_f64(i) / 255.;
                pixel(a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u)
            });
            let norm = raw_source_normalize(&mut row);
            (row, norm)
        })
        .collect()
}

// Optional recovery acceptance only: test the claimed payload against unmodified
// source luminance. Never repair digits or change existing decoder acceptance.
#[cfg(feature = "medium")]
fn recovered_ean_source_agreement(image: Image<'_>, q: Quad, digits: &[u8; 13]) -> bool {
    use barcode_research_core::numeric::{f64_isize, f64_usize, isize_f64, usize_f64};
    let profiles = raw_source_profiles(image, q);
    let bits = barcode_research_core::ean::encode(digits);
    let mut best = [0_f64; 3];
    for reverse in [false, true] {
        let raster: Vec<f64> = (0..824)
            .map(|i| {
                let module = f64_isize((usize_f64(i) / 8. - 4.).floor());
                if (0..95).contains(&module) {
                    f64::from(
                        bits[if reverse {
                            94 - usize::try_from(module).expect("module checked in 0..95")
                        } else {
                            usize::try_from(module).expect("module checked in 0..95")
                        }],
                    )
                } else {
                    0.
                }
            })
            .collect();
        for sigma in [0., 0.3, 0.6, 0.9] {
            let smoothed = if sigma == 0. {
                raster.clone()
            } else {
                let radius = f64_isize((sigma * 8. * 3_f64).ceil());
                let weights: Vec<_> = (-radius..=radius)
                    .map(|k| (-0.5 * (isize_f64(k) / (sigma * 8.)).powi(2)).exp())
                    .collect();
                let sum = weights.iter().sum::<f64>();
                (0..raster.len())
                    .map(|i| {
                        (-radius..=radius)
                            .zip(&weights)
                            .map(|(k, w)| {
                                let index =
                                    isize::try_from(i).expect("fixed 824-sample raster") + k;
                                if index < 0
                                    || index
                                        >= isize::try_from(raster.len())
                                            .expect("fixed 824-sample raster")
                                {
                                    0.
                                } else {
                                    raster
                                        [usize::try_from(index).expect("index checked nonnegative")]
                                        * w
                                }
                            })
                            .sum::<f64>()
                            / sum
                    })
                    .collect()
            };
            for pitch in [0.98, 1., 1.02] {
                for offset in [-1., -0.5, 0., 0.5, 1.] {
                    let mut template = std::array::from_fn(|i| {
                        let u = -0.02 + 1.04 * usize_f64(i) / 255.;
                        let index = ((95. * u - offset) / pitch + 4.) * 8.;
                        if index < 0. || index >= usize_f64(smoothed.len() - 1) {
                            return 0.;
                        }
                        let k = f64_usize(index.floor());
                        let f = index - index.floor();
                        smoothed[k] * (1. - f) + smoothed[k + 1] * f
                    });
                    raw_source_normalize(&mut template);
                    for (j, (row, norm)) in profiles.iter().enumerate() {
                        if *norm < 160. {
                            continue;
                        }
                        let corr = template.iter().zip(row).map(|(a, b)| a * b).sum::<f64>();
                        best[j] = best[j].min(corr);
                    }
                }
            }
        }
    }
    // At least two physical source rows explain 65% of raw profile variance.
    // A failed optional recovery is deferred; existing reads are untouched.
    let height = f64::midpoint(
        (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]),
        (q[2][0] - q[1][0]).hypot(q[2][1] - q[1][1]),
    );
    (0..3).any(|i| {
        ((i + 1)..3).any(|j| {
            best[i] < 0.
                && best[i] * best[i] >= 0.65
                && best[j] < 0.
                && best[j] * best[j] >= 0.65
                && usize_f64(j - i) * 0.3 * height >= 2.
        })
    })
}

#[cfg(all(test, feature = "medium"))]
mod recovered_source_tests {
    use super::*;

    const DIGITS: [u8; 13] = [4, 0, 0, 6, 3, 8, 1, 3, 3, 3, 9, 3, 1];
    const OTHER: [u8; 13] = [9, 7, 8, 0, 2, 0, 1, 3, 7, 9, 6, 2, 4];
    const WIDTH: usize = 412;
    const HEIGHT: usize = 100;
    const SYMBOL: Quad = [[16., 10.], [396., 10.], [396., 90.], [16., 90.]];

    fn fixture(black: u8, white: u8) -> Vec<u8> {
        let bits = barcode_research_core::ean::encode(&DIGITS);
        let mut pixels = vec![white; WIDTH * HEIGHT];
        for y in 8..92 {
            for (module, bit) in bits.iter().enumerate() {
                if *bit > 0.5 {
                    for x in 16 + module * 4..20 + module * 4 {
                        pixels[y * WIDTH + x] = black;
                    }
                }
            }
        }
        pixels
    }

    fn image(pixels: &[u8]) -> Image<'_> {
        Image {
            data: pixels,
            width: WIDTH,
            height: HEIGHT,
            channels: 1,
            stride: WIDTH,
        }
    }

    #[test]
    fn raw_evidence_rejects_unrelated_payload_and_accepts_either_direction() {
        let pixels = fixture(20, 235);
        assert!(recovered_ean_source_agreement(
            image(&pixels),
            SYMBOL,
            &DIGITS
        ));
        assert!(!recovered_ean_source_agreement(
            image(&pixels),
            SYMBOL,
            &OTHER
        ));
        let reversed = [SYMBOL[2], SYMBOL[3], SYMBOL[0], SYMBOL[1]];
        assert!(recovered_ean_source_agreement(
            image(&pixels),
            reversed,
            &DIGITS
        ));
    }

    #[test]
    fn raw_evidence_needs_contrast_and_separate_source_rows() {
        let faint = fixture(120, 130);
        assert!(!recovered_ean_source_agreement(
            image(&faint),
            SYMBOL,
            &DIGITS
        ));
        let flat = vec![128; WIDTH * HEIGHT];
        assert!(!recovered_ean_source_agreement(
            image(&flat),
            SYMBOL,
            &DIGITS
        ));
        let pixels = fixture(20, 235);
        let thin = [[16., 49.], [396., 49.], [396., 51.], [16., 51.]];
        assert!(!recovered_ean_source_agreement(
            image(&pixels),
            thin,
            &DIGITS
        ));
    }
}

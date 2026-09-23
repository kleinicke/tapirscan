use super::effort::SELECTED;
use super::{Error, Image, ImageView, Proposal, Quad, Result, ScanOptions, Scanner};
use barcode_research_core::{frame::Barcode, multi_scan::Policy, shear, stripes};

struct Localization {
    proposals: Vec<Proposal>,
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
    let im = checked_image(image)?;
    let localization = localize(&mut scanner.localizer, im, image)?;
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
    let recovery = recover(scanner, image, &mut scan, coverage, options, shared_retail)?;
    #[cfg(feature = "low")]
    let recovery: Option<super::read::Recovery> = None;
    if let Some(recovery) = &recovery {
        retail.extend(recovery.retail.iter().cloned());
    }

    if consolidate {
        super::linear_duplicates::merge_primary(&mut scan.frame.barcodes, image);
    }
    if !options.multiple {
        select_one(&mut scan.frame.barcodes);
    }
    Ok(Result {
        proposals: localization.proposals,
        localization_omitted: localization.omitted,
        localization_work_limited: localization.work_limited,
        search_window: localization.search_window,
        scan,
        options,
        recovery,
        retail,
    })
}

fn localize(
    detector: &mut stripes::Detector,
    im: ImageView<'_>,
    image: Image<'_>,
) -> std::result::Result<Localization, Error> {
    let found = detector.detect(im)?;
    let count = found.proposals.len();
    let examined = count.min(SELECTED.fit_limit);
    let mut proposals = found.proposals;
    for i in 0..examined {
        if let Some(p) = shear::refine(im, proposals[i].polygon) {
            proposals.push(p);
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
    Policy {
        complete: options.finish_candidates,
        #[cfg(not(feature = "low"))]
        candidate_retry_mask: super::formats::uncovered_mask(
            proposals,
            coverage,
            super::detail::retry_mask(image, proposals),
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
) -> std::result::Result<Option<super::read::Recovery>, Error> {
    let result = super::detail::recover(
        image,
        &mut scan.frame.barcodes,
        &mut scanner.recovery,
        coverage,
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

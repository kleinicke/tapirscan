//! Restored-contrast and threshold retries on bounded source crops.
use super::{
    checked_image, protected_primary, scan_prepared_impl, source_contradiction, Error, Image,
    Proposal, Quad, ScanOptions, Scanner,
};
#[cfg(feature = "medium")]
use super::{recovered_ean_source_agreement, scan_policy};
#[cfg(any(feature = "high", feature = "very-high"))]
use barcode_research_core::multi_scan::Policy;

#[cfg(not(feature = "low"))]
pub(super) fn restore_source_image(image: Image<'_>) -> Vec<u8> {
    restore_with_kernel(image, &[1, 4, 6, 4, 1])
}

/// Sharpen a recovery crop as `(5 * gray - 3 * blur + 1) / 2`, using a separable,
/// edge-clamped integer blur normalized by the squared kernel sum. Integer
/// arithmetic keeps every output byte exact.
#[cfg(not(feature = "low"))]
pub(super) fn restore_with_kernel(image: Image<'_>, weights: &[u32]) -> Vec<u8> {
    let (w, h) = (image.width, image.height);
    let center = weights.len() / 2;
    let normalizer = weights.iter().sum::<u32>().pow(2);
    let mut gray = Vec::with_capacity(w * h);
    for row in image.data.chunks(image.stride).take(h) {
        if image.channels == 1 {
            gray.extend(row[..w].iter().map(|&value| u32::from(value)));
        } else {
            gray.extend(
                row[..w * image.channels]
                    .chunks_exact(image.channels)
                    .map(|pixel| {
                        (77 * u32::from(pixel[0])
                            + 150 * u32::from(pixel[1])
                            + 29 * u32::from(pixel[2])
                            + 128)
                            / 256
                    }),
            );
        }
    }
    let mut horizontal = vec![0_u32; w * h];
    // Replicating edge pixels equals clamping each tap's column.
    let mut padded = vec![0_u32; w + 2 * center];
    for (source, target) in gray.chunks_exact(w).zip(horizontal.chunks_exact_mut(w)) {
        for (j, value) in padded.iter_mut().enumerate() {
            *value = source[j.saturating_sub(center).min(w - 1)];
        }
        for (x, value) in target.iter_mut().enumerate() {
            *value = weights.iter().zip(&padded[x..]).map(|(a, g)| a * g).sum();
        }
    }
    let mut out = vec![0_u8; w * h];
    let mut rows = vec![0_usize; weights.len()];
    for y in 0..h {
        for (i, row) in rows.iter_mut().enumerate() {
            *row = (y + i).saturating_sub(center).min(h - 1) * w;
        }
        for x in 0..w {
            let smooth: u32 = weights
                .iter()
                .zip(&rows)
                .map(|(a, row)| a * horizontal[row + x])
                .sum();
            let value = (5 * i32::try_from(gray[y * w + x]).expect("gray")
                - 3 * i32::try_from((smooth + normalizer / 2) / normalizer).expect("smooth")
                + 1)
                / 2;
            out[y * w + x] = u8::try_from(value.clamp(0, 255)).expect("clamped gray");
        }
    }
    out
}

#[cfg(not(feature = "low"))]
pub(super) fn recover_restored_regions(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    proposals: &[Proposal],
    scan: &mut crate::ScanResult,
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
                    crate::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
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
            if crate::geometry::overlap_quads(&read.detection.polygon, &p.polygon).0 < 0.3 {
                continue;
            }
            if protected.iter().chain(&scan.frame.barcodes).any(|b| {
                crate::geometry::overlap_quads(&read.detection.polygon, &b.detection.polygon).0
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
pub(super) fn restoration_bounds(image: Image<'_>, polygon: Quad) -> (usize, usize, usize, usize) {
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
pub(super) fn recover_threshold_regions(
    scanner: &mut Scanner,
    image: Image<'_>,
    proposals: &[Proposal],
    scan: &mut crate::ScanResult,
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
                    crate::geometry::overlap_quads(&p.polygon, &b.detection.polygon).1 >= 0.3
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
                crate::geometry::overlap_quads(&read.detection.polygon, &b.detection.polygon).0
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

#[cfg(feature = "medium")]
pub(super) fn restore_alternate_source_image(image: Image<'_>) -> Vec<u8> {
    restore_with_kernel(image, &[1, 4, 11, 21, 26, 21, 11, 4, 1])
}

#[cfg(feature = "medium")]
pub(super) fn recover_late_wide_crop(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    proposals: &[Proposal],
    scan: &mut crate::ScanResult,
) -> std::result::Result<(), Error> {
    use barcode_research_core::numeric::usize_f64;
    let mut candidates: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame.candidates.get(*i).is_some_and(|c| {
                c.detections.is_empty() && (c.work.guard_pass >= 4 || p.score >= 0.95)
            })
        })
        .collect();
    candidates.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    for (_, p) in candidates.into_iter().take(1) {
        let (x, y, w, h) = restoration_bounds(image, p.polygon);
        if w < 3 || h < 3 || w * h > 131_072 {
            continue;
        }
        let crop = Image {
            data: &image.data[y * image.stride + x * image.channels..],
            width: w,
            height: h,
            channels: image.channels,
            stride: image.stride,
        };
        let pixels = restore_alternate_source_image(crop);
        let enhanced = Image {
            data: &pixels,
            width: w,
            height: h,
            channels: 1,
            stride: w,
        };
        let crop_coverage: Vec<_> = coverage
            .iter()
            .map(|q| q.map(|v| [v[0] - usize_f64(x), v[1] - usize_f64(y)]))
            .collect();
        let proposal = Proposal {
            polygon: p
                .polygon
                .map(|v| [v[0] - usize_f64(x), v[1] - usize_f64(y)]),
            score: p.score,
        };
        let policy = scan_policy(enhanced, &[proposal], &crop_coverage, options);
        scanner.regions.retail_configure(1)?;
        let retry = scanner
            .regions
            .scan(checked_image(enhanced)?, &[proposal.polygon], policy)?;
        scan.frame.unfinished |= retry.frame.unfinished;
        for mut read in retry.frame.barcodes {
            if read.detection.support < 5 {
                continue;
            }
            for point in &mut read.detection.polygon {
                point[0] += usize_f64(x);
                point[1] += usize_f64(y);
            }
            if crate::geometry::overlap_quads(&read.detection.polygon, &p.polygon).0 < 0.3 {
                continue;
            }
            if scan.frame.barcodes.iter().any(|old| {
                old.detection.digits == read.detection.digits
                    || crate::geometry::overlap_quads(
                        &read.detection.polygon,
                        &old.detection.polygon,
                    )
                    .0 > 0.
            }) {
                continue;
            }
            if !recovered_ean_source_agreement(
                image,
                read.detection.polygon,
                &read.detection.digits,
            ) || source_contradiction(
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

/// Source pixels allowed in one reduced crop; equal to the restoration crop budget.
#[cfg(feature = "medium")]
const SCALED_CROP_PIXELS: usize = 131_072;

/// Area-averaged grayscale reduction of a source crop by `factor` (< 1).
#[cfg(feature = "medium")]
fn reduce_gray(image: Image<'_>, factor: f64) -> (Vec<u8>, usize, usize) {
    use barcode_research_core::numeric::{f64_usize, usize_f64};
    let w = f64_usize((usize_f64(image.width) * factor).floor()).max(3);
    let h = f64_usize((usize_f64(image.height) * factor).floor()).max(3);
    let span = |o: usize, length: usize| {
        let start = f64_usize((usize_f64(o) / factor).floor()).min(length - 1);
        let end = f64_usize((usize_f64(o + 1) / factor).ceil()).clamp(start + 1, length);
        start..end
    };
    let mut out = vec![0u8; w * h];
    for (oy, row) in out.chunks_exact_mut(w).enumerate() {
        let rows = span(oy, image.height);
        for (ox, value) in row.iter_mut().enumerate() {
            let columns = span(ox, image.width);
            let mut sum = 0u32;
            for line in image
                .data
                .chunks(image.stride)
                .skip(rows.start)
                .take(rows.len())
            {
                for x in columns.clone() {
                    let p = &line[x * image.channels..];
                    sum += if image.channels == 1 {
                        u32::from(p[0])
                    } else {
                        (u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29) >> 8
                    };
                }
            }
            let count = u32::try_from(rows.len() * columns.len()).unwrap_or(u32::MAX);
            *value = u8::try_from((sum + count / 2) / count).unwrap_or(u8::MAX);
        }
    }
    (out, w, h)
}

/// Large unresolved symbols exceed the restoration crop budgets, and decoding a
/// sharp proposal can fail at one sampling scale only. Rescan up to two such
/// proposals with retail guard evidence (or blurred high-scoring stripes) from
/// an area-reduced crop, keeping source agreement and contradiction checks.
#[cfg(feature = "medium")]
pub(super) fn recover_scaled_crop(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    coverage: &[Quad],
    proposals: &[Proposal],
    scan: &mut crate::ScanResult,
) -> std::result::Result<(), Error> {
    use barcode_research_core::numeric::usize_f64;
    let mut candidates: Vec<_> = proposals
        .iter()
        .enumerate()
        .filter(|(i, p)| {
            scan.frame.candidates.get(*i).is_some_and(|c| {
                c.detections.is_empty()
                    && (c.work.guard_pass >= 1
                        || (p.score >= 0.9 && c.work.forward_blur_windows >= 2))
            }) && {
                let (_, _, w, h) = restoration_bounds(image, p.polygon);
                w * h > SCALED_CROP_PIXELS
            }
        })
        .collect();
    candidates.sort_by_key(|(i, _)| std::cmp::Reverse(scan.frame.candidates[*i].work.guard_pass));
    for (_, p) in candidates.into_iter().take(2) {
        let (x, y, w, h) = restoration_bounds(image, p.polygon);
        let factor = (usize_f64(SCALED_CROP_PIXELS) / usize_f64(w * h)).sqrt();
        if factor < 0.2 {
            continue;
        }
        let crop = Image {
            data: &image.data[y * image.stride + x * image.channels..],
            width: w,
            height: h,
            channels: image.channels,
            stride: image.stride,
        };
        let (pixels, rw, rh) = reduce_gray(crop, factor);
        let reduced = Image {
            data: &pixels,
            width: rw,
            height: rh,
            channels: 1,
            stride: rw,
        };
        let crop_coverage: Vec<_> = coverage
            .iter()
            .map(|q| {
                q.map(|v| {
                    [
                        (v[0] - usize_f64(x)) * factor,
                        (v[1] - usize_f64(y)) * factor,
                    ]
                })
            })
            .collect();
        let crop = ReducedCrop {
            x,
            y,
            factor,
            proposal: p.polygon,
            coverage: &crop_coverage,
        };
        // Sharpen once with the late-crop kernel when the plain reduction reads nothing.
        if !admit_reduced_reads(scanner, image, options, &crop, reduced, scan)? {
            let sharpened = restore_alternate_source_image(reduced);
            let sharpened = Image {
                data: &sharpened,
                width: rw,
                height: rh,
                channels: 1,
                stride: rw,
            };
            admit_reduced_reads(scanner, image, options, &crop, sharpened, scan)?;
        }
    }
    Ok(())
}

/// Source placement of an area-reduced recovery crop.
#[cfg(feature = "medium")]
struct ReducedCrop<'a> {
    x: usize,
    y: usize,
    factor: f64,
    proposal: Quad,
    coverage: &'a [Quad],
}

/// Scan one reduced crop and admit source-confirmed reads; true when any was added.
#[cfg(feature = "medium")]
fn admit_reduced_reads(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    crop: &ReducedCrop<'_>,
    variant: Image<'_>,
    scan: &mut crate::ScanResult,
) -> std::result::Result<bool, Error> {
    use barcode_research_core::numeric::usize_f64;
    // Keep decoded geometry for source confirmation, as for restored crops.
    let retry = scan_prepared_impl(
        scanner,
        variant,
        options,
        crop.coverage,
        false,
        false,
        false,
    )?;
    scan.frame.unfinished |= retry.scan.frame.unfinished;
    let mut accepted = false;
    for mut read in retry.scan.frame.barcodes {
        if read.detection.support < 3 {
            continue;
        }
        for point in &mut read.detection.polygon {
            point[0] = point[0] / crop.factor + usize_f64(crop.x);
            point[1] = point[1] / crop.factor + usize_f64(crop.y);
        }
        if crate::geometry::overlap_quads(&read.detection.polygon, &crop.proposal).0 < 0.3 {
            continue;
        }
        if scan.frame.barcodes.iter().any(|old| {
            old.detection.digits == read.detection.digits
                || crate::geometry::overlap_quads(&read.detection.polygon, &old.detection.polygon).0
                    > 0.
        }) {
            continue;
        }
        if !recovered_ean_source_agreement(image, read.detection.polygon, &read.detection.digits)
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
        accepted = true;
    }
    Ok(accepted)
}

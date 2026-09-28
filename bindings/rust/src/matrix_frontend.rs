//! Matrix reader orchestration with shared baseline thresholds and bounded Aztec recovery.
//! Reader order and non-Aztec policies mirror the pinned multiformat orchestrator.
use barcode_multiformat::{
    binarization, dm_detect, maxicode_detect, pdf417, qr_detect, regions, Detection, Scan,
};
pub(crate) fn scan(image: &[u8], width: usize, height: usize, mask: u32, effort: usize) -> Scan {
    let mut regions = regions::Regions::default();
    let mut binary_images = binarization::Images::new(image, width, height);
    let (mut matrix_results, mut matrix_unfinished) = if mask & 512 != 0 {
        let (mut reads, mut limited) = if effort > 2 {
            qr_detect::detect_curved(width, height, &mut regions, &mut binary_images)
        } else if effort > 1 {
            qr_detect::detect_extended(width, height, &mut regions, &mut binary_images)
        } else {
            qr_detect::detect(width, height, &mut regions, &mut binary_images)
        };
        if effort > 1 && width * height <= 1_048_576 {
            let enhanced = sharpen(image, width, height);
            let mut recovery = binarization::Images::new(&enhanced, width, height);
            let (extra, extra_limited) = if effort > 2 {
                qr_detect::detect_curved(width, height, &mut regions, &mut recovery)
            } else {
                qr_detect::detect_extended(width, height, &mut regions, &mut recovery)
            };
            limited |= extra_limited;
            for read in extra {
                if !reads.iter().any(|prior| {
                    prior.text == read.text
                        && prior.structured_append == read.structured_append
                        && barcode_multiformat::regions::overlap(&prior.polygon, &read.polygon)
                            >= 0.65
                }) {
                    reads.push(read);
                }
            }
        }
        (reads, limited)
    } else {
        (vec![], false)
    };
    if mask & 1024 != 0 {
        let (mut reads, limited) =
            dm_detect::detect(width, height, &mut regions, &mut binary_images);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
    }
    if mask & 4096 != 0 {
        let mut search = barcode_research_core::aztec_frontend::Search::default();
        for mode in 0..4 {
            if binary_images.is_duplicate(mode) {
                continue;
            }
            let (bits, rows) = binary_images.get_with_runs(mode, (height / 600).max(1));
            search.threshold(
                barcode_research_core::aztec_frontend::Threshold {
                    bits,
                    gray: image,
                    width,
                    height,
                    mode,
                },
                &mut regions,
                &|y, row, out| rows.copy_or_extract(y, row, out),
            );
        }
        search.contrast(image, width, height, &mut regions);
        matrix_results.append(&mut search.reads);
        matrix_unfinished |= search.limited;
    }
    if mask & 131_072 != 0 {
        let (mut reads, limited) =
            maxicode_detect::detect(width, height, &mut regions, &mut binary_images);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
    }
    if mask & 2048 != 0 {
        let (mut reads, limited) = pdf417::detect(image, width, height, &mut regions);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
    }
    matrix_results.sort_by_key(|r| std::cmp::Reverse(r.support));
    let mut distinct: Vec<Detection> = Vec::new();
    for read in matrix_results {
        if !distinct.iter().any(|r| {
            r.format == read.format
                && r.text == read.text
                && r.addon == read.addon
                && r.structured_append == read.structured_append
                && r.reader_initialization == read.reader_initialization
                && regions::overlap(&r.polygon, &read.polygon) >= 0.65
        }) {
            distinct.push(read);
        }
    }
    let matrix_results = distinct;
    let (regions, limited) = regions.finish(&matrix_results);
    Scan {
        barcodes: matrix_results,
        regions,
        unfinished: matrix_unfinished || limited,
        lines: 0,
    }
}
fn sharpen(gray: &[u8], width: usize, height: usize) -> Vec<u8> {
    let weights = [1_u32, 4, 6, 4, 1];
    let mut horizontal = vec![0_u32; width * height];
    for y in 0..height {
        for x in 0..width {
            horizontal[y * width + x] = weights
                .iter()
                .enumerate()
                .map(|(i, &weight)| {
                    let xx = x.saturating_add(i).saturating_sub(2).min(width - 1);
                    u32::from(gray[y * width + xx]) * weight
                })
                .sum();
        }
    }
    let mut output = vec![0_u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let smooth: u32 = weights
                .iter()
                .enumerate()
                .map(|(i, &weight)| {
                    let yy = y.saturating_add(i).saturating_sub(2).min(height - 1);
                    horizontal[yy * width + x] * weight
                })
                .sum();
            let center = i32::from(gray[y * width + x]);
            let restored = center * 3
                - i32::try_from((smooth + 128) / 256).expect("smoothed pixel fits i32") * 2;
            output[y * width + x] =
                u8::try_from(restored.clamp(0, 255)).expect("clamped pixel fits u8");
        }
    }
    output
}

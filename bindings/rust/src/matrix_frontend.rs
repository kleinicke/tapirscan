//! Matrix reader orchestration with shared baseline thresholds and bounded Aztec recovery.
//! Reader order and non-Aztec policies mirror the pinned multiformat orchestrator.
use barcode_multiformat::{
    Detection, Scan, binarization, dm_detect, maxicode_detect, pdf417, qr_detect, regions,
};
/// Stack for reader threads: they used to run on the caller's (usually larger) stack.
#[cfg(not(target_family = "wasm"))]
const READER_STACK: usize = 8 << 20;

/// One matrix reader's reads, work flag and recorded regions.
type ReaderRun = (Vec<Detection>, bool, regions::Regions);

/// The matrix readers selected by `mask`, in their fixed result order.
fn readers(mask: u32) -> Vec<u32> {
    [512, 1024, 4096, 131_072, 2048]
        .into_iter()
        .filter(|bit| mask & bit != 0)
        .collect()
}

/// Run one matrix reader with the shared regions and binarization cache.
fn run_reader(
    bit: u32,
    image: &[u8],
    width: usize,
    height: usize,
    effort: usize,
    regions: &mut regions::Regions,
    binary_images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    match bit {
        512 => {
            let (mut reads, mut limited) = if effort > 2 {
                qr_detect::detect_curved(width, height, regions, binary_images)
            } else if effort > 1 {
                qr_detect::detect_extended(width, height, regions, binary_images)
            } else {
                qr_detect::detect(width, height, regions, binary_images)
            };
            if effort > 1 && width * height <= 1_048_576 {
                let enhanced = sharpen(image, width, height);
                let mut recovery = binarization::Images::new(&enhanced, width, height);
                let (extra, extra_limited) = if effort > 2 {
                    qr_detect::detect_curved(width, height, regions, &mut recovery)
                } else {
                    qr_detect::detect_extended(width, height, regions, &mut recovery)
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
        }
        1024 => dm_detect::detect(width, height, regions, binary_images, effort > 1),
        4096 => {
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
                    regions,
                    &|y, row, out| rows.copy_or_extract(y, row, out),
                );
            }
            search.contrast(image, width, height, regions);
            (search.reads, search.limited)
        }
        131_072 => maxicode_detect::detect(width, height, regions, binary_images),
        _ => pdf417::detect(image, width, height, regions),
    }
}

/// Run the readers concurrently, each with private regions and binarization.
///
/// Readers only add regions, merging within their own format, and PDF417 reads
/// back only PDF417 regions. Concatenating the private regions in reader order
/// therefore equals the shared sequence, unless the shared 256-region cap would
/// have dropped one; that case returns `None` and the caller runs sequentially.
#[cfg(not(target_family = "wasm"))]
fn parallel_readers(
    bits: &[u32],
    image: &[u8],
    width: usize,
    height: usize,
    effort: usize,
) -> Option<(Vec<ReaderRun>, regions::Regions)> {
    // The base thresholds every grid reader starts from are computed once and
    // shared read-only (cloning the cache shares the images); binarization is
    // deterministic, so each reader sees what a fresh cache would compute.
    let mut shared = binarization::Images::new(image, width, height);
    if bits.iter().any(|&bit| bit != 2048) {
        // A second thresholding thread pays off from about one megapixel.
        if width * height >= 1 << 20 {
            shared.prefill_parallel();
        } else {
            for mode in 0..4 {
                let _ = shared.get(mode);
            }
        }
        let _ = shared.is_duplicate(1);
    }
    let shared = &shared;
    let run = |bit: u32| {
        let mut regions = regions::Regions::default();
        let mut binary_images = if bit == 2048 {
            binarization::Images::new(image, width, height)
        } else {
            shared.clone()
        };
        let (reads, limited) = run_reader(
            bit,
            image,
            width,
            height,
            effort,
            &mut regions,
            &mut binary_images,
        );
        (reads, limited, regions)
    };
    let runs: Vec<ReaderRun> = std::thread::scope(|scope| {
        // A reader whose thread cannot start runs here instead; its private regions
        // keep the result identical either way.
        let pending: Vec<_> = bits
            .iter()
            .map(|&bit| {
                std::thread::Builder::new()
                    .stack_size(READER_STACK)
                    .spawn_scoped(scope, move || run(bit))
                    .map_err(|_| bit)
            })
            .collect();
        pending
            .into_iter()
            .map(|handle| match handle {
                Ok(handle) => handle.join().expect("matrix reader thread panicked"),
                Err(bit) => run(bit),
            })
            .collect()
    });
    let total: usize = runs
        .iter()
        .map(|(_, _, regions)| regions.candidates.len())
        .sum();
    if total > regions::MAX_CANDIDATES {
        return None;
    }
    let mut merged = regions::Regions::default();
    for (_, _, regions) in &runs {
        merged.limited |= regions.limited;
        merged.candidates.extend(regions.candidates.iter().cloned());
    }
    Some((runs, merged))
}

/// Whether several matrix readers run on separate threads: on multi-core hosts
/// unless `TAPIRSCAN_THREADS=1` (for servers already running one scanner per core).
fn parallel_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        let allowed = std::env::var("TAPIRSCAN_THREADS")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .is_none_or(|threads| threads > 1);
        allowed && std::thread::available_parallelism().is_ok_and(|cores| cores.get() > 1)
    })
}

/// Run `scan` for each format group, concurrently on multi-core hosts. Groups are
/// independent scans of the same pixels, so results equal the sequential order.
pub(crate) fn concurrent(
    groups: &[(u32, usize)],
    scan: &(dyn Fn(u32, usize) -> Scan + Sync),
) -> Vec<Scan> {
    #[cfg(not(target_family = "wasm"))]
    if let Some((&(last, last_level), rest)) = groups.split_last()
        && !rest.is_empty()
        && parallel_enabled()
    {
        return std::thread::scope(|scope| {
            // The last group runs here; callers put their heaviest group last.
            let pending: Vec<_> = rest
                .iter()
                .map(|&(selected, level)| {
                    std::thread::Builder::new()
                        .stack_size(READER_STACK)
                        .spawn_scoped(scope, move || scan(selected, level))
                        .map_err(|_| (selected, level))
                })
                .collect();
            let tail = scan(last, last_level);
            let mut scans: Vec<Scan> = pending
                .into_iter()
                .map(|handle| match handle {
                    Ok(handle) => handle.join().expect("format group thread panicked"),
                    Err((selected, level)) => scan(selected, level),
                })
                .collect();
            scans.push(tail);
            scans
        });
    }
    groups
        .iter()
        .map(|&(selected, level)| scan(selected, level))
        .collect()
}

pub(crate) fn scan(image: &[u8], width: usize, height: usize, mask: u32, effort: usize) -> Scan {
    scan_with(image, width, height, mask, effort, parallel_enabled())
}

fn scan_with(
    image: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
    allow_parallel: bool,
) -> Scan {
    let bits = readers(mask);
    let mut matrix_results = Vec::new();
    let mut matrix_unfinished = false;
    #[cfg(not(target_family = "wasm"))]
    let parallel = if bits.len() > 1 && allow_parallel {
        parallel_readers(&bits, image, width, height, effort)
    } else {
        None
    };
    #[cfg(target_family = "wasm")]
    let parallel: Option<(Vec<ReaderRun>, regions::Regions)> = {
        let _ = allow_parallel;
        None
    };
    let regions = if let Some((runs, merged)) = parallel {
        for (mut reads, limited, _) in runs {
            matrix_results.append(&mut reads);
            matrix_unfinished |= limited;
        }
        merged
    } else {
        let mut regions = regions::Regions::default();
        let mut binary_images = binarization::Images::new(image, width, height);
        for bit in bits {
            let (mut reads, limited) = run_reader(
                bit,
                image,
                width,
                height,
                effort,
                &mut regions,
                &mut binary_images,
            );
            matrix_results.append(&mut reads);
            matrix_unfinished |= limited;
        }
        regions
    };
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

#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::{concurrent, scan_with};

    fn report(scan: &barcode_multiformat::Scan) -> String {
        format!(
            "{:?}",
            (
                scan.barcodes
                    .iter()
                    .map(|read| (&read.format, &read.text, read.polygon, read.support))
                    .collect::<Vec<_>>(),
                scan.regions
                    .iter()
                    .map(|region| (&region.format, region.polygon, region.support))
                    .collect::<Vec<_>>(),
                scan.unfinished,
            )
        )
    }

    #[test]
    fn concurrent_groups_keep_their_order() {
        let scan = |selected: u32, level: usize| barcode_multiformat::Scan {
            barcodes: Vec::new(),
            regions: Vec::new(),
            unfinished: selected % 2 == 1,
            lines: selected as usize * 10 + level,
        };
        let groups = [(7, 1), (2, 0), (5, 3)];
        let lines: Vec<_> = concurrent(&groups, &scan).iter().map(|s| s.lines).collect();
        assert_eq!(lines, [71, 20, 53]);
        assert_eq!(concurrent(&groups[..1], &scan)[0].lines, 71);
        assert!(concurrent(&[], &scan).is_empty());
    }

    #[test]
    fn parallel_readers_match_the_sequential_scan() {
        let (width, height) = (320, 240);
        let mut state = 0x9e37_79b9_u32;
        let mut images = vec![vec![255; width * height], vec![0; width * height]];
        // Blocky noise yields many candidate regions, including past the shared cap.
        for block in [1, 3, 8] {
            let mut image = vec![0; width * height];
            for (i, pixel) in image.iter_mut().enumerate() {
                let (x, y) = (i % width / block, i / width / block);
                let (x, y) = (u32::try_from(x).unwrap(), u32::try_from(y).unwrap());
                let mut v = state ^ x.wrapping_mul(0x85eb_ca6b) ^ y.wrapping_mul(0xc2b2_ae35);
                v ^= v >> 15;
                *pixel = if v & 1 == 0 { 0 } else { 255 };
            }
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            images.push(image);
        }
        for image in &images {
            for effort in 0..4 {
                for mask in [512 | 1024, 512 | 1024 | 4096 | 131_072 | 2048, 1024 | 2048] {
                    assert_eq!(
                        report(&scan_with(image, width, height, mask, effort, true)),
                        report(&scan_with(image, width, height, mask, effort, false)),
                        "effort {effort} mask {mask}"
                    );
                }
            }
        }
    }
}

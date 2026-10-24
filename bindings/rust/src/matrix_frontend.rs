//! Matrix reader orchestration with shared baseline thresholds and bounded Aztec recovery.
//! Reader order and non-Aztec policies mirror `barcode_multiformat::scan_observed_for_mode`.
use barcode_multiformat::matrix_readers::{self, Reader};
use barcode_multiformat::{Detection, Scan, binarization, regions};
/// Stack for reader threads, sized like a typical main-thread stack.
#[cfg(not(target_family = "wasm"))]
const READER_STACK: usize = 8 << 20;

/// One matrix reader's reads, work flag and recorded regions.
type ReaderRun = (Vec<Detection>, bool, regions::Regions);

/// The matrix readers selected by `mask`, in their fixed result order.
fn readers(mask: u32) -> Vec<Reader> {
    matrix_readers::ORDER
        .into_iter()
        .filter_map(|reader| reader.select(mask))
        .collect()
}

/// Run one matrix reader with the shared regions and binarization cache.
fn run_reader(
    reader: Reader,
    image: &[u8],
    width: usize,
    height: usize,
    effort: usize,
    regions: &mut regions::Regions,
    binary_images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    matrix_readers::read(
        reader,
        image,
        [width, height],
        effort,
        crate::MODE_ID as usize,
        regions,
        binary_images,
    )
}

/// Run the readers concurrently, each with private regions and binarization.
///
/// Readers only add regions, merging within their own format, and PDF417 reads
/// back only PDF417 regions. Concatenating the private regions in reader order
/// therefore equals the shared sequence, unless the shared 256-region cap would
/// have dropped one; that case returns `None` and the caller runs sequentially.
#[cfg(not(target_family = "wasm"))]
fn parallel_readers(
    readers: &[Reader],
    image: &[u8],
    width: usize,
    height: usize,
    effort: usize,
) -> Option<(Vec<ReaderRun>, regions::Regions)> {
    // The base thresholds every grid reader starts from are computed once and
    // shared read-only (cloning the cache shares the images); binarization is
    // deterministic, so each reader sees what a fresh cache would compute.
    let mut shared = binarization::Images::new(image, width, height);
    if readers.iter().any(|&reader| reader != Reader::Pdf417) {
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
    let run = |reader: Reader| {
        let mut regions = regions::Regions::default();
        let mut binary_images = if reader == Reader::Pdf417 {
            binarization::Images::new(image, width, height)
        } else {
            shared.clone()
        };
        let (reads, limited) = run_reader(
            reader,
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
        let pending: Vec<_> = readers
            .iter()
            .map(|&reader| {
                std::thread::Builder::new()
                    .stack_size(READER_STACK)
                    .spawn_scoped(scope, move || run(reader))
                    .map_err(|_| reader)
            })
            .collect();
        pending
            .into_iter()
            .map(|handle| match handle {
                Ok(handle) => handle.join().expect("matrix reader thread panicked"),
                Err(reader) => run(reader),
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

thread_local! {
    static PARALLEL: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Run `body` with threading allowed or not for scans started on this thread.
///
/// The scanner's `parallel` option reaches the deeply nested reader calls through
/// this setting; threads spawned for readers inherit it explicitly.
pub(crate) fn with_parallel<T>(allowed: bool, body: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            PARALLEL.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(PARALLEL.with(|cell| cell.replace(allowed)));
    body()
}

/// Whether several matrix readers run on separate threads: when the scan allows
/// it and the host has more than one core.
fn parallel_enabled() -> bool {
    static MULTI_CORE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    PARALLEL.with(std::cell::Cell::get)
        && *MULTI_CORE
            .get_or_init(|| std::thread::available_parallelism().is_ok_and(|cores| cores.get() > 1))
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
                        .spawn_scoped(scope, move || with_parallel(true, || scan(selected, level)))
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

/// Scan the matrix readers of `mask`. On one thread they use `binary_images`, a threshold
/// cache of the same `image` that other reader groups of the frame share.
pub(crate) fn scan(
    image: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
    binary_images: &mut binarization::Images<'_>,
) -> Scan {
    scan_with(
        image,
        width,
        height,
        mask,
        effort,
        parallel_enabled(),
        binary_images,
    )
}

fn scan_with(
    image: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
    allow_parallel: bool,
    binary_images: &mut binarization::Images<'_>,
) -> Scan {
    let readers = readers(mask);
    let mut matrix_results = Vec::new();
    let mut matrix_unfinished = false;
    #[cfg(not(target_family = "wasm"))]
    let parallel = if readers.len() > 1 && allow_parallel {
        parallel_readers(&readers, image, width, height, effort)
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
        for reader in readers {
            let (mut reads, limited) = run_reader(
                reader,
                image,
                width,
                height,
                effort,
                &mut regions,
                binary_images,
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
#[cfg(all(test, not(target_family = "wasm")))]
mod tests {
    use super::{concurrent, scan_with};
    use barcode_multiformat::formats::{AZTEC, DATA_MATRIX, MAXI_CODE, PDF417, QR_CODE};

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
            let fresh = || barcode_multiformat::binarization::Images::new(image, width, height);
            for effort in 0..4 {
                for mask in [
                    QR_CODE | DATA_MATRIX,
                    QR_CODE | DATA_MATRIX | AZTEC | MAXI_CODE | PDF417,
                    DATA_MATRIX | PDF417,
                ] {
                    assert_eq!(
                        report(&scan_with(
                            image,
                            width,
                            height,
                            mask,
                            effort,
                            true,
                            &mut fresh()
                        )),
                        report(&scan_with(
                            image,
                            width,
                            height,
                            mask,
                            effort,
                            false,
                            &mut fresh()
                        )),
                        "effort {effort} mask {mask}"
                    );
                }
            }
        }
    }
}

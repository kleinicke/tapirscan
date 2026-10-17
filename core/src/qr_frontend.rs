//! QR-only production frontend: the shared multiformat detector under this
//! mode's [`Policy`], plus block-threshold and sharpened retries. The
//! multi-format groups use the same detector and policy.
use barcode_multiformat::qr_detect::{self, FULL_BUDGET, Pass, Policy};
use barcode_multiformat::{Detection, Scan, binarization, qr_enhance};

/// This mode's QR detection choices.
const POLICY: Policy = Policy::for_mode(if cfg!(feature = "mode-low") {
    0
} else if cfg!(feature = "mode-medium") {
    1
} else if cfg!(feature = "mode-high") {
    2
} else {
    3
});
/// Run one detection pass under this mode's policy.
fn detect(
    width: usize,
    height: usize,
    regions: &mut barcode_multiformat::regions::Regions,
    binary: &mut binarization::Images<'_>,
    pass: Pass,
) -> (Vec<Detection>, bool) {
    qr_detect::detect_with(width, height, regions, binary, POLICY, pass)
}

/// Scan QR at effort 0.
#[must_use]
pub fn scan(image: &[u8], width: usize, height: usize) -> Scan {
    scan_impl(image, width, height, false, 0)
}
/// Scan QR with the block thresholds, sharpening and curved grids of `effort`.
#[must_use]
pub fn scan_with_effort(image: &[u8], width: usize, height: usize, effort: usize) -> Scan {
    scan_impl(image, width, height, false, effort)
}
/// Retry the two additional foreground thresholds on an unresolved source image.
#[must_use]
pub fn scan_foreground(image: &[u8], width: usize, height: usize) -> Scan {
    scan_impl(image, width, height, true, 0)
}
/// Decode attempts for each block-threshold pass.
const HYBRID_BUDGET: usize = 200;
fn scan_impl(image: &[u8], width: usize, height: usize, foreground: bool, effort: usize) -> Scan {
    if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > image.len()) {
        return Scan {
            barcodes: vec![],
            regions: vec![],
            unfinished: false,
            lines: 0,
        };
    }
    let mut regions = barcode_multiformat::regions::Regions::default();
    let mut binary = binarization::Images::new(image, width, height);
    // The foreground retry tries only the additional foreground splits; the
    // base threshold passes remain owned by the caller.
    let pass = if foreground {
        Pass {
            modes: 4..8,
            curved: false,
            budget: FULL_BUDGET,
        }
    } else {
        Pass::effort(effort > 1, effort > 2)
    };
    let (mut reads, mut limited) = detect(width, height, &mut regions, &mut binary, pass);
    // Block thresholds follow sharp shadow edges and large modules that the
    // 33-pixel local mean misses. Medium tries them where a QR candidate stayed
    // unread; High and Very High on every unread frame. Each pass has a bounded
    // budget and stops quietly.
    if !foreground
        && reads.is_empty()
        && effort >= 1
        && (effort >= 2 || !regions.candidates.is_empty())
    {
        let mut hybrid = binarization::Images::new_hybrid(image, width, height);
        for modes in [1..2, 3..4] {
            let pass = Pass {
                modes,
                curved: false,
                budget: HYBRID_BUDGET,
            };
            let (extra, _) = detect(width, height, &mut regions, &mut hybrid, pass);
            reads.extend(extra);
        }
    }
    if !foreground && effort > 1 && width * height <= 1_048_576 {
        let enhanced = qr_enhance::sharpen(image, width, height);
        let mut recovery = binarization::Images::new(&enhanced, width, height);
        let pass = Pass::effort(true, effort > 2);
        let (extra, extra_limited) = detect(width, height, &mut regions, &mut recovery, pass);
        limited |= extra_limited;
        for read in extra {
            if !reads.iter().any(|prior| {
                prior.text == read.text
                    && prior.structured_append == read.structured_append
                    && barcode_multiformat::regions::overlap(&prior.polygon, &read.polygon) >= 0.65
            }) {
                reads.push(read);
            }
        }
    }

    reads.sort_by_key(|r| std::cmp::Reverse(r.support));
    let mut distinct: Vec<Detection> = Vec::new();
    for read in reads {
        if !distinct.iter().any(|prior| {
            prior.format == read.format
                && prior.text == read.text
                && prior.addon == read.addon
                && prior.structured_append == read.structured_append
                && prior.reader_initialization == read.reader_initialization
                && barcode_multiformat::regions::overlap(&prior.polygon, &read.polygon) >= 0.65
        }) {
            distinct.push(read);
        }
    }
    let (regions, region_limited) = regions.finish(&distinct);
    Scan {
        barcodes: distinct,
        regions,
        unfinished: limited || region_limited,
        lines: 0,
    }
}

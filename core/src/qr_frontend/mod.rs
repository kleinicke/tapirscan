//! QR-only production frontend. Decoder/ECC and region ownership use the pinned
//! shared implementation. Multi-matrix groups retain the shared threshold pipeline.
mod binarization;
mod component_geometry;
mod dm_detect;
mod qr_detect;
mod qr_enhance;
use barcode_multiformat::{Detection, Scan};

/// Scan ordinary QR effort with unchanged finder and recovery work budgets.
#[must_use]
pub fn scan(image: &[u8], width: usize, height: usize) -> Scan {
    scan_impl(image, width, height, false, 0)
}
/// Scan QR with the existing effort-specific thresholds, sharpening and curved grids.
#[must_use]
pub fn scan_with_effort(image: &[u8], width: usize, height: usize, effort: usize) -> Scan {
    scan_impl(image, width, height, false, effort)
}
/// Retry the two additional foreground thresholds on an unresolved source image.
#[must_use]
pub fn scan_foreground(image: &[u8], width: usize, height: usize) -> Scan {
    scan_impl(image, width, height, true, 0)
}
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
    let (mut reads, mut limited) = if foreground {
        qr_detect::detect_foreground(width, height, &mut regions, &mut binary)
    } else if effort == 0 {
        qr_detect::detect(width, height, &mut regions, &mut binary)
    } else {
        qr_detect::detect_effort(width, height, &mut regions, &mut binary, effort)
    };
    if !foreground && effort > 1 && width * height <= 1_048_576 {
        let enhanced = qr_enhance::sharpen(image, width, height);
        let mut recovery = binarization::Images::new(&enhanced, width, height);
        let (extra, extra_limited) =
            qr_detect::detect_effort(width, height, &mut regions, &mut recovery, effort);
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
fn histogram(row: &[u8]) -> [usize; 256] {
    let mut hist = [0usize; 256];
    if row.len() >= 512 {
        // Independent counters avoid a serial load/store chain on broad flat backgrounds.
        let mut lanes = [[0usize; 256]; 4];
        let mut chunks = row.chunks_exact(4);
        for pixels in &mut chunks {
            lanes[0][usize::from(pixels[0])] += 1;
            lanes[1][usize::from(pixels[1])] += 1;
            lanes[2][usize::from(pixels[2])] += 1;
            lanes[3][usize::from(pixels[3])] += 1;
        }
        for (value, count) in hist.iter_mut().enumerate() {
            *count = lanes.iter().map(|lane| lane[value]).sum();
        }
        for &value in chunks.remainder() {
            hist[usize::from(value)] += 1;
        }
    } else {
        for &v in row {
            hist[usize::from(v)] += 1;
        }
    }
    hist
}
fn transition_offsets_into(bits: &[bool], offsets: &mut Vec<usize>) {
    offsets.clear();
    offsets.push(0);
    if bits.is_empty() {
        return;
    }
    let mut previous = bits[0];
    let mut i = 1;
    while bits.len() - i >= 8 {
        let mut bytes = [0u8; 8];
        for (byte, &bit) in bytes.iter_mut().zip(&bits[i..i + 8]) {
            *byte = u8::from(bit);
        }
        let word = u64::from_le_bytes(bytes);
        let transitions = word ^ (word << 8 | u64::from(u8::from(previous)));
        let mut pending = transitions;
        while pending != 0 {
            offsets.push(i + pending.trailing_zeros() as usize / 8);
            pending &= pending - 1;
        }
        previous = bits[i + 7];
        i += 8;
    }
    while i < bits.len() {
        if bits[i] != previous {
            offsets.push(i);
        }
        previous = bits[i];
        i += 1;
    }
    offsets.push(bits.len());
}
fn threshold(row: &[u8], mode: usize) -> Vec<bool> {
    assert_eq!(mode, 0);
    let mut bits = Vec::with_capacity(row.len());
    let hist = histogram(row);
    let sum: u64 = hist
        .iter()
        .enumerate()
        .map(|(i, &n)| i as u64 * n as u64)
        .sum();
    let mut mass = 0;
    let mut left = 0u64;
    let mut best = 0.;
    let mut cut = 128;
    for (i, &n) in hist.iter().enumerate() {
        // Empty bins repeat the exact same masses, means and score; they
        // cannot improve the strict maximum or move its first threshold.
        if n == 0 {
            continue;
        }
        mass += n;
        left += n as u64 * i as u64;
        if mass == 0 || mass == row.len() {
            continue;
        }
        let delta = barcode_multiformat::numeric::u64_f64(left)
            / barcode_multiformat::numeric::usize_f64(mass)
            - barcode_multiformat::numeric::u64_f64(sum - left)
                / barcode_multiformat::numeric::usize_f64(row.len() - mass);
        let score = barcode_multiformat::numeric::usize_f64(mass)
            * barcode_multiformat::numeric::usize_f64(row.len() - mass)
            * delta
            * delta;
        if score > best {
            best = score;
            cut = i;
        }
    }
    bits.extend(row.iter().map(|&v| v as usize <= cut));
    bits
}

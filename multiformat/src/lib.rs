pub mod aztec;
pub mod aztec_detect;
pub mod binarization;
mod component_geometry;
pub mod databar;
pub mod datamatrix;
pub mod dm_detect;
pub mod encoding;
mod encoding_tables;
pub mod expanded;
pub mod linear;
mod linear_geometry;
pub mod maxicode;
pub mod maxicode_detect;
mod maxicode_tables;
#[doc(hidden)]
pub mod numeric;
pub mod pdf417;
mod pdf417_tables;
pub mod pdf_localize;
pub mod qr;
pub mod qr_detect;
mod qr_enhance;
mod qr_tables;
pub mod reed_binary;
pub mod reed_prime;
pub mod reed_solomon;
pub mod regions;
mod retail_gray;
mod retail_profile;
use serde::Serialize;

/// One symbol in a structured-append sequence; index is one-based. Assembly is
/// deliberately left to callers because distinct frames can contain repeats.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct StructuredAppend {
    pub index: usize,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parity: Option<u8>,
}
#[derive(Clone, Serialize)]
pub struct Detection {
    /// Decoded payload bytes before character-set interpretation, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<Vec<u8>>,
    #[serde(rename = "structuredAppend", skip_serializing_if = "Option::is_none")]
    pub structured_append: Option<StructuredAppend>,
    #[serde(
        rename = "readerInitialization",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub reader_initialization: bool,
    #[serde(rename = "eanAddOn", skip_serializing_if = "Option::is_none")]
    pub addon: Option<String>,
    pub format: String,
    pub text: String,
    pub polygon: [[f32; 2]; 4],
    pub support: usize,
    pub error: f32,
    pub gs1: bool,
}
#[derive(Serialize)]
pub struct Scan {
    pub barcodes: Vec<Detection>,
    pub regions: Vec<regions::Region>,
    pub unfinished: bool,
    pub lines: usize,
}
struct Group {
    profile_only: bool,
    decoded: bool,
    addon: Option<String>,
    addon_support: usize,
    format: String,
    text: String,
    angle: f32,
    lo: f32,
    hi: f32,
    top: f32,
    bottom: f32,
    support: usize,
    error: f32,
    gs1: bool,
    last_line: usize,
}
pub struct Session {
    input: Vec<u8>,
    rgba: Vec<u8>,
    output: Vec<u8>,
    width: usize,
    height: usize,
}

fn histogram(row: &[u8]) -> [usize; 256] {
    let mut hist = [0usize; 256];
    if row.len() >= 2048 {
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

fn threshold(row: &[u8], mode: usize) -> Vec<bool> {
    let mut bits = Vec::with_capacity(row.len());
    threshold_into(row, mode, &mut bits);
    bits
}
fn nested_threshold_into(row: &[u8], upper: u8, bits: &mut Vec<bool>) {
    bits.clear();
    bits.reserve(row.len());
    {
        let mut hist = [0usize; 256];
        for &v in row {
            if v <= upper {
                hist[v as usize] += 1;
            }
        }
        let sum: u64 = hist
            .iter()
            .enumerate()
            .map(|(i, &n)| i as u64 * n as u64)
            .sum();
        let count: usize = hist.iter().sum();
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
            if mass == 0 || mass == count {
                continue;
            }
            let delta = crate::numeric::u64_f64(left) / crate::numeric::usize_f64(mass)
                - crate::numeric::u64_f64(sum - left) / crate::numeric::usize_f64(count - mass);
            let score = crate::numeric::usize_f64(mass)
                * crate::numeric::usize_f64(count - mass)
                * delta
                * delta;
            if score > best {
                best = score;
                cut = i;
            }
        }
        bits.extend(row.iter().map(|&v| v as usize <= cut));
    }
}
fn threshold_into(row: &[u8], mode: usize, bits: &mut Vec<bool>) {
    bits.clear();
    bits.reserve(row.len());
    if mode == 0 {
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
            let delta = crate::numeric::u64_f64(left) / crate::numeric::usize_f64(mass)
                - crate::numeric::u64_f64(sum - left) / crate::numeric::usize_f64(row.len() - mass);
            let score = crate::numeric::usize_f64(mass)
                * crate::numeric::usize_f64(row.len() - mass)
                * delta
                * delta;
            if score > best {
                best = score;
                cut = i;
            }
        }
        bits.extend(row.iter().map(|&v| v as usize <= cut));
    } else {
        let radius = if mode == 1 { 24 } else { 64 };
        adaptive_threshold_into(row, radius, bits);
    }
}
#[expect(
    clippy::cast_possible_truncation,
    reason = "Private callers select only radii 24 or 64; window counts fit u32."
)]
fn adaptive_threshold_into(row: &[u8], radius: usize, bits: &mut Vec<bool>) {
    if row.is_empty() {
        return;
    }
    // A short row has no fixed-size interior. Keep the original expanding /
    // shrinking recurrence as one bounded fallback.
    if row.len() <= radius * 2 + 1 {
        let mut sum: u32 = row.iter().take(radius + 1).map(|&v| u32::from(v)).sum();
        let mut count: u32 = row.iter().take(radius + 1).map(|_| 1).sum();
        for (i, &v) in row.iter().enumerate() {
            if i > radius {
                sum -= u32::from(row[i - radius - 1]);
                count -= 1;
            }
            if i > 0 && i + radius < row.len() {
                sum += u32::from(row[i + radius]);
                count += 1;
            }
            bits.push((u32::from(v) + 3) * count < sum);
        }
        return;
    }
    let mut sum: u32 = row[..=radius].iter().map(|&v| u32::from(v)).sum();
    let mut count = (radius + 1) as u32;
    bits.push((u32::from(row[0]) + 3) * count < sum);
    for i in 1..=radius {
        sum += u32::from(row[i + radius]);
        count += 1;
        bits.push((u32::from(row[i]) + 3) * count < sum);
    }
    count = (radius * 2 + 1) as u32;
    for i in radius + 1..row.len() - radius {
        sum -= u32::from(row[i - radius - 1]);
        sum += u32::from(row[i + radius]);
        bits.push((u32::from(row[i]) + 3) * count < sum);
    }
    for i in row.len() - radius..row.len() {
        sum -= u32::from(row[i - radius - 1]);
        count -= 1;
        bits.push((u32::from(row[i]) + 3) * count < sum);
    }
}
#[cfg(test)]
fn runs(bits: &[bool]) -> (Vec<f32>, Vec<usize>) {
    let mut widths = Vec::new();
    let mut offsets = Vec::new();
    runs_into(bits, &mut widths, &mut offsets);
    (widths, offsets)
}
#[expect(
    clippy::cast_precision_loss,
    reason = "Run widths intentionally retain the existing usize-to-f32 conversion semantics."
)]
fn runs_into(bits: &[bool], widths: &mut Vec<f32>, offsets: &mut Vec<usize>) {
    widths.clear();
    offsets.clear();
    transition_offsets_into(bits, offsets);
    for pair in offsets.windows(2) {
        widths.push((pair[1] - pair[0]) as f32);
    }
}

// Reversing a binary row reverses its run lengths and reflects every boundary.
// Keep boundary arithmetic integer-exact before downstream floating sampling.
fn reverse_runs(widths: &mut [f32], offsets: &mut [usize], length: usize) {
    widths.reverse();
    offsets.reverse();
    for offset in offsets {
        *offset = length - *offset;
    }
}

fn run_offsets(bits: &[bool]) -> Vec<usize> {
    let mut offsets = Vec::new();
    run_offsets_into(bits, &mut offsets);
    offsets
}
fn run_offsets_into(bits: &[bool], offsets: &mut Vec<usize>) {
    transition_offsets_into(bits, offsets);
    if bits.is_empty() {
        offsets.push(0);
    }
}
pub(crate) fn transition_offsets_into(bits: &[bool], offsets: &mut Vec<usize>) {
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
#[cfg(test)]
fn refined_runs(bits: &[bool], row: &[u8], reverse: bool) -> (Vec<f32>, Vec<usize>) {
    refine_run_offsets(run_offsets(bits), row, reverse)
}
#[inline]
fn refined_edge(i: usize, row: &[u8], reverse: bool) -> f32 {
    if i == 0 || i == row.len() {
        return crate::numeric::usize_f32(i);
    }
    let pixel = |i: usize| f32::from(row[if reverse { row.len() - 1 - i } else { i }]);
    let mut low = 255f32;
    let mut high = 0f32;
    for j in i.saturating_sub(2)..(i + 2).min(row.len()) {
        low = low.min(pixel(j));
        high = high.max(pixel(j));
    }
    let a = pixel(i - 1);
    let b = pixel(i);
    let frac = if (b - a).abs() > 1. {
        (((low + high) * 0.5 - a) / (b - a)).clamp(0., 1.)
    } else {
        0.5
    };
    crate::numeric::usize_f32(i) - 0.5 + frac
}
fn refine_run_offsets(offsets: Vec<usize>, row: &[u8], reverse: bool) -> (Vec<f32>, Vec<usize>) {
    let mut widths = Vec::with_capacity(offsets.len() - 1);
    refine_run_offsets_into(&offsets, row, reverse, &mut widths);
    (widths, offsets)
}
fn refine_run_offsets_into(offsets: &[usize], row: &[u8], reverse: bool, widths: &mut Vec<f32>) {
    widths.clear();
    widths.reserve(offsets.len().saturating_sub(1));
    let mut last_edge = 0.;
    for &i in offsets.iter().skip(1).take(offsets.len().saturating_sub(2)) {
        let edge = refined_edge(i, row, reverse);
        widths.push(edge - last_edge);
        last_edge = edge;
    }
    widths.push(crate::numeric::usize_f32(row.len()) - last_edge);
}
fn direction_agrees(image: &[u8], width: usize, height: usize, group: &Group) -> bool {
    let cos_angle = group.angle.cos();
    let sin_angle = group.angle.sin();
    let mut xx = 0f64;
    let mut yy = 0f64;
    let mut xy = 0f64;
    for j in 0_u8..5 {
        for i in 0_u8..64 {
            let along = group.lo + (group.hi - group.lo) * (f32::from(i) + 0.5) / 64.;
            let cross = group.top + (group.bottom - group.top) * (f32::from(j) + 0.5) / 5.;
            let x = crate::numeric::f32_isize((along * cos_angle - cross * sin_angle).round());
            let y = crate::numeric::f32_isize((along * sin_angle + cross * cos_angle).round());
            if x < 1 || y < 1 || x >= (width).cast_signed() - 1 || y >= (height).cast_signed() - 1 {
                continue;
            }
            let at = (y).cast_unsigned() * width + (x).cast_unsigned();
            let dx = f64::from(image[at + 1]) - f64::from(image[at - 1]);
            let dy = f64::from(image[at + width]) - f64::from(image[at - width]);
            xx += dx * dx;
            yy += dy * dy;
            xy += dx * dy;
        }
    }
    let anisotropy = (xx - yy).hypot(2. * xy);
    if anisotropy < (xx + yy) * 0.35 || anisotropy < 100. {
        return true;
    }
    let agreement = ((xx - yy) * f64::from((2. * group.angle).cos())
        + 2. * xy * f64::from((2. * group.angle).sin()))
        / anisotropy;
    agreement >= 0.55
}
/// Scan a row-major grayscale image for the formats selected by `mask`.
///
/// Effort 0 uses axial scanlines; 1 adds diagonal and adaptive-threshold retries,
/// and larger values add intermediate angles. Invalid dimensions or a short
/// image buffer return an empty scan. Results include qualified unread regions.
#[must_use]
pub fn scan(image: &[u8], width: usize, height: usize, mask: u32, effort: usize) -> Scan {
    scan_observed(image, width, height, mask, effort, |_, _| {})
}

/// Native diagnostic hook called after each enabled stage. The normal scanner
/// uses a no-op callback; no clocks or profiling allocations enter that path.
/// `limited` includes previously raised shared work-limit signals, not a timeout.
#[expect(
    clippy::too_many_lines,
    reason = "The scanner orchestrator keeps reader order, deduplication and aggregate unfinished-work accounting in one transaction."
)]
pub fn scan_observed(
    image: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
    mut observe: impl FnMut(&'static str, bool),
) -> Scan {
    if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > image.len()) {
        return Scan {
            barcodes: vec![],
            regions: vec![],
            unfinished: false,
            lines: 0,
        };
    }
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
            let enhanced = qr_enhance::sharpen(image, width, height);
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
                        && crate::regions::overlap(&prior.polygon, &read.polygon) >= 0.65
                }) {
                    reads.push(read);
                }
            }
        }
        observe("QRCode", limited || regions.limited);
        (reads, limited)
    } else {
        (vec![], false)
    };
    if mask & 1024 != 0 {
        let (mut reads, limited) =
            dm_detect::detect(width, height, &mut regions, &mut binary_images);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
        observe("DataMatrix", matrix_unfinished || regions.limited);
    }
    if mask & 4096 != 0 {
        let (mut reads, limited) =
            aztec_detect::detect(width, height, &mut regions, &mut binary_images);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
        observe("Aztec", matrix_unfinished || regions.limited);
    }
    if mask & 131_072 != 0 {
        let (mut reads, limited) =
            maxicode_detect::detect(width, height, &mut regions, &mut binary_images);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
        observe("MaxiCode", matrix_unfinished || regions.limited);
    }
    if mask & 2048 != 0 {
        let (mut reads, limited) = pdf417::detect(image, width, height, &mut regions);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
        observe("PDF417", matrix_unfinished || regions.limited);
    }
    if mask & (511 | 8192 | 16384) == 0 {
        linear_geometry::distinct(&mut matrix_results);
        let (regions, limited) = regions.finish(&matrix_results);
        return Scan {
            barcodes: matrix_results,
            regions,
            unfinished: matrix_unfinished || limited,
            lines: 0,
        };
    }
    // Exact constant-background exclusion: all pixels outside this rectangle
    // equal image[0]. Skip only lines whose rounded sample positions cannot
    // intersect it. No threshold, format, successful read or annotation gates it.
    let background = image[0];
    let mut active = [width, height, 0usize, 0usize];
    for (y, row) in image[..width * height].chunks_exact(width).enumerate() {
        if let Some(left) = row.iter().position(|&value| value != background) {
            let right = row
                .iter()
                .rposition(|&value| value != background)
                .unwrap_or(left);
            active[0] = active[0].min(left);
            active[1] = active[1].min(y);
            active[2] = active[2].max(right);
            active[3] = active[3].max(y);
        }
    }
    let step = crate::numeric::usize_f32(
        (width.min(height) / if effort > 0 { 120 } else { 64 }).clamp(2, 16),
    );
    let angles: &[f32] = if effort > 1 {
        &[0., 90., 45., -45., 22.5, -22.5, 67.5, -67.5]
    } else if effort > 0 {
        &[0., 90., 45., -45.]
    } else {
        &[0., 90.]
    };
    let mut groups: Vec<Group> = Vec::new();
    let mut databar_stacked = databar::Stacked::default();
    let mut expanded_stacked = expanded::Stacked::default();
    let mut row = Vec::new();
    let mut bits = Vec::new();
    let mut previous_bits = Vec::new();
    let mut refined = Vec::new();
    let mut offsets = Vec::new();
    let mut integer_runs = Vec::new();
    let mut dummy_integer_offsets = Vec::new();
    let mut cached_row = Vec::new();
    let mut cached_reads: Vec<(usize, linear::Read, usize, usize)> = Vec::new();
    let mut line_id = 0;
    for &deg in angles {
        cached_row.clear();
        cached_reads.clear();
        let angle = deg.to_radians();
        #[expect(
            clippy::float_cmp,
            reason = "Preset angles are exact constants; equality selects axial fast paths."
        )]
        let (cos_angle, sin_angle) = if deg == 0. {
            (1., 0.)
        } else if deg == 90. {
            (0., 1.)
        } else {
            (angle.cos(), angle.sin())
        };
        let corners = [
            [0., 0.],
            [crate::numeric::usize_f32(width) - 1., 0.],
            [
                crate::numeric::usize_f32(width) - 1.,
                crate::numeric::usize_f32(height) - 1.,
            ],
            [0., crate::numeric::usize_f32(height) - 1.],
        ];
        let mut amin = f32::INFINITY;
        let mut amax = f32::NEG_INFINITY;
        let mut bmin = f32::INFINITY;
        let mut bmax = f32::NEG_INFINITY;
        for [x, y] in corners {
            amin = amin.min(x * cos_angle + y * sin_angle);
            amax = amax.max(x * cos_angle + y * sin_angle);
            bmin = bmin.min(-x * sin_angle + y * cos_angle);
            bmax = bmax.max(-x * sin_angle + y * cos_angle);
        }
        let (mut active_min, mut active_max) = (f32::INFINITY, f32::NEG_INFINITY);
        if active[0] < width {
            for x in [active[0], active[2]] {
                for y in [active[1], active[3]] {
                    let projected = -crate::numeric::usize_f32(x) * sin_angle
                        + crate::numeric::usize_f32(y) * cos_angle;
                    active_min = active_min.min(projected - 2.);
                    active_max = active_max.max(projected + 2.);
                }
            }
        }
        let mut b = bmin + (step * 0.5).min((bmax - bmin) * 0.5);
        while b <= bmax {
            if b < active_min || b > active_max {
                line_id += 1;
                b += step;
                continue;
            }
            row.clear();
            bits.clear();
            previous_bits.clear();
            refined.clear();
            offsets.clear();
            integer_runs.clear();
            dummy_integer_offsets.clear();
            let mut start = amin;
            #[expect(
                clippy::float_cmp,
                reason = "Preset angles are exact constants; equality selects axial fast paths."
            )]
            if deg == 0. {
                let y = crate::numeric::f32_usize(b.round());
                row.extend_from_slice(&image[y * width..(y + 1) * width]);
            } else if deg == 90. {
                let x = crate::numeric::f32_usize((-b).round());
                row.extend((0..height).map(|y| image[y * width + x]));
            } else {
                // Intersect this scanline with the image before sampling. Keep
                // a one-pixel conservative margin and the original rounded
                // coordinate checks, preserving the exact previous row grid.
                let mut lo = amin;
                let mut hi = amax;
                for (coefficient, offset, extent) in [
                    (cos_angle, -b * sin_angle, width),
                    (sin_angle, b * cos_angle, height),
                ] {
                    if coefficient.abs() > 1e-6 {
                        let a = (-0.5 - offset) / coefficient;
                        let z = (crate::numeric::usize_f32(extent) - 0.5 - offset) / coefficient;
                        lo = lo.max(a.min(z));
                        hi = hi.min(a.max(z));
                    }
                }
                let first =
                    ((crate::numeric::f32_isize((lo - amin).floor()) - 1).max(0)).cast_unsigned();
                let last =
                    ((crate::numeric::f32_isize((hi - amin).ceil()) + 1).max(0)).cast_unsigned();
                for i in first..=last.min(crate::numeric::f32_usize((amax - amin).ceil())) {
                    let a = amin + crate::numeric::usize_f32(i);
                    let x = crate::numeric::f32_isize((a * cos_angle - b * sin_angle).round());
                    let y = crate::numeric::f32_isize((a * sin_angle + b * cos_angle).round());
                    if x >= 0
                        && y >= 0
                        && (x).cast_unsigned() < width
                        && (y).cast_unsigned() < height
                    {
                        if row.is_empty() {
                            start = a;
                        }
                        row.push(image[(y).cast_unsigned() * width + (x).cast_unsigned()]);
                    } else if !row.is_empty() {
                        break;
                    }
                }
            }
            line_id += 1;
            if row.len() >= 40 && row.iter().any(|&v| v != row[0]) {
                let reusable = mask & (linear::DATABAR | linear::DATABAR_EXPANDED) == 0;
                if !reusable || row != cached_row {
                    cached_reads.clear();
                    let mut sharpened_row = None;
                    let ordinary_passes = if effort == 0 {
                        1
                    } else if mask & linear::EAN8 != 0
                        && (deg.abs() < 0.01 || (deg - 90.).abs() < 0.01)
                    {
                        5
                    } else {
                        2
                    };
                    let mut nested_upper = None;
                    let passes = if effort > 0 && mask & 127 != 0 {
                        ordinary_passes + 1
                    } else {
                        ordinary_passes
                    };
                    for mode in 0..passes {
                        let nested = mode == ordinary_passes;
                        if nested {
                            let Some(upper) = nested_upper else {
                                continue;
                            };
                            nested_threshold_into(&row, upper, &mut bits);
                            run_offsets_into(&bits, &mut offsets);
                            refine_run_offsets_into(&offsets, &row, false, &mut refined);
                        } else if mode == 2 {
                            let (profile_bits, profile_r, profile_offsets) =
                                retail_profile::runs(&row);
                            bits = profile_bits;
                            refined = profile_r;
                            offsets = profile_offsets;
                        } else if mode >= 3 {
                            let sharpened = sharpened_row
                                .get_or_insert_with(|| retail_profile::sharpen_row(&row));
                            threshold_into(sharpened, mode - 3, &mut bits);
                            run_offsets_into(&bits, &mut offsets);
                            refine_run_offsets_into(&offsets, sharpened, false, &mut refined);
                        } else {
                            threshold_into(&row, mode, &mut bits);
                            // Refinement is unnecessary when the exact existing
                            // duplicate-threshold check would discard this pass.
                            if bits == previous_bits {
                                continue;
                            }
                            run_offsets_into(&bits, &mut offsets);
                            refine_run_offsets_into(&offsets, &row, false, &mut refined);
                        }
                        if mode == 0 && refined.len() < 16 {
                            let mut lower = 255;
                            let mut upper = 0;
                            for (&value, &black) in row.iter().zip(&bits) {
                                if black {
                                    lower = lower.min(value);
                                    upper = upper.max(value);
                                }
                            }
                            if upper.saturating_sub(lower) >= 6 {
                                nested_upper = Some(upper);
                            }
                        }
                        let mask = if nested {
                            mask & (127 | linear::ADDON_READ | linear::ADDON_REQUIRE)
                        } else if mode >= 2 {
                            mask & (linear::EAN8 | linear::ADDON_READ | linear::ADDON_REQUIRE)
                        } else {
                            mask
                        };
                        if mask & linear::CODABAR != 0 {
                            runs_into(&bits, &mut integer_runs, &mut dummy_integer_offsets);
                        } else {
                            integer_runs.clear();
                        }
                        for reverse in [false, true] {
                            if reverse {
                                refined.reverse();
                                offsets.reverse();
                                for offset in &mut offsets {
                                    *offset = row.len() - *offset;
                                }
                                integer_runs.reverse();
                            }
                            let Some(&first_black) =
                                (if reverse { bits.last() } else { bits.first() })
                            else {
                                continue;
                            };
                            if mask & linear::DATABAR != 0 {
                                databar_stacked.observe(databar::ScanLine {
                                    runs: &refined,
                                    offsets: &offsets,
                                    first_black,
                                    reverse,
                                    length: row.len(),
                                    start,
                                    cross: b,
                                    angle,
                                    line: line_id,
                                    step,
                                });
                            }
                            if mask & linear::DATABAR_EXPANDED != 0 {
                                expanded_stacked.observe(databar::ScanLine {
                                    runs: &refined,
                                    offsets: &offsets,
                                    first_black,
                                    reverse,
                                    length: row.len(),
                                    start,
                                    cross: b,
                                    angle,
                                    line: line_id,
                                    step,
                                });
                            }
                            let mut reads = linear::decode_candidates(
                                &refined,
                                first_black,
                                mask,
                                &mut regions.limited,
                            );
                            if mask & linear::CODABAR != 0 {
                                reads.extend(linear::decode_candidates(
                                    &integer_runs,
                                    first_black,
                                    linear::CODABAR,
                                    &mut regions.limited,
                                ));
                            }
                            for mut read in reads {
                                if mode == 2 && !read.decoded && read.format == "EAN8" {
                                    if let Some((text, error)) =
                                        retail_gray::decode(&row, &refined, read.start, reverse)
                                    {
                                        read.text = text;
                                        read.decoded = true;
                                        read.error = error;
                                    }
                                }
                                let (left, right) = if reverse {
                                    (
                                        row.len() - offsets[read.end],
                                        row.len() - offsets[read.start],
                                    )
                                } else {
                                    (offsets[read.start], offsets[read.end])
                                };
                                cached_reads.push((
                                    if nested { 0 } else { mode },
                                    read,
                                    left,
                                    right,
                                ));
                            }
                        }
                        std::mem::swap(&mut bits, &mut previous_bits);
                    }
                    if reusable {
                        cached_row.clone_from(&row);
                    }
                }
                for &(mode, ref read, left, right) in &cached_reads {
                    let lo = start + crate::numeric::usize_f32(left);
                    let hi = start + crate::numeric::usize_f32(right);
                    let length = hi - lo;
                    if let Some(g) = groups.iter_mut().find(|g| {
                        g.format == read.format
                            && g.decoded == read.decoded
                            && g.text == read.text
                            && (g.addon.is_none() || read.addon.is_none() || g.addon == read.addon)
                            && (g.angle - angle).abs() < 0.01
                            && (g.lo - lo).abs() < length * 0.25
                            && (g.hi - hi).abs() < length * 0.25
                            && b - g.bottom <= step * 3.
                    }) {
                        g.profile_only &= mode >= 2;
                        if g.last_line != line_id {
                            g.support += 1;
                            if read.addon.is_some() {
                                g.addon.clone_from(&read.addon);
                                g.addon_support += 1;
                            }
                            g.last_line = line_id;
                            g.bottom = b;
                            g.lo = g.lo.min(lo);
                            g.hi = g.hi.max(hi);
                            g.error = g.error.min(read.error);
                        }
                    } else {
                        groups.push(Group {
                            profile_only: mode >= 2,
                            decoded: read.decoded,
                            addon_support: usize::from(read.addon.is_some()),
                            addon: read.addon.clone(),
                            format: read.format.into(),
                            text: read.text.clone(),
                            angle,
                            lo,
                            hi,
                            top: b,
                            bottom: b,
                            support: 1,
                            error: read.error,
                            gs1: read.gs1,
                            last_line: line_id,
                        });
                    }
                }
            }
            b += step;
        }
    }
    let mut results: Vec<Detection> = Vec::new();
    groups.sort_by(|a, b| b.support.cmp(&a.support));
    for g in groups {
        if g.profile_only && g.error > 0.14 {
            continue;
        }
        if g.support < 2 && width.min(height) > 4 {
            continue;
        }
        if !g.decoded && (g.support < 3 || g.error > 0.08) {
            continue;
        }
        let addon_confirmed = g.addon_support >= if width.min(height) > 4 { 2 } else { 1 };
        if g.decoded
            && mask & linear::ADDON_REQUIRE != 0
            && matches!(g.format.as_str(), "EAN13" | "UPCA" | "EAN8" | "UPCE")
            && !addon_confirmed
        {
            continue;
        }
        // A scanline crossing bar ends can accidentally satisfy another
        // symbology's guards/checksum. Require agreement with the actual bar
        // gradient for every linear read, not just checkless ITF.
        if !direction_agrees(image, width, height, &g) {
            continue;
        }
        let cos_angle = g.angle.cos();
        let sin_angle = g.angle.sin();
        let top = g.top - step * 0.5;
        let bottom = g.bottom + step * 0.5;
        let polygon = [[g.lo, top], [g.hi, top], [g.hi, bottom], [g.lo, bottom]]
            .map(|[a, b]| [a * cos_angle - b * sin_angle, a * sin_angle + b * cos_angle]);
        let polygon = if g.format == "EAN8" {
            linear_geometry::refine_retail(image, width, height, polygon)
        } else {
            linear_geometry::refine(image, width, height, polygon)
        };
        if !g.decoded {
            regions.add(&g.format, polygon, 1. - g.error, g.support);
            continue;
        }
        let center = polygon
            .iter()
            .fold([0., 0.], |p, q| [p[0] + q[0] / 4., p[1] + q[1] / 4.]);
        let duplicate = results.iter().any(|r| {
            if r.format != g.format || r.text != g.text {
                return false;
            }
            let xs = r.polygon.map(|p| p[0]);
            let ys = r.polygon.map(|p| p[1]);
            center[0] >= xs.iter().copied().fold(f32::INFINITY, f32::min) - step
                && center[0] <= xs.iter().copied().fold(f32::NEG_INFINITY, f32::max) + step
                && center[1] >= ys.iter().copied().fold(f32::INFINITY, f32::min) - step
                && center[1] <= ys.iter().copied().fold(f32::NEG_INFINITY, f32::max) + step
        });
        if !duplicate {
            results.push(Detection {
                bytes: None,
                structured_append: None,
                reader_initialization: false,
                addon: if addon_confirmed { g.addon } else { None },
                format: g.format,
                text: g.text,
                polygon,
                support: g.support,
                error: g.error,
                gs1: g.gs1,
            });
        }
    }
    let (mut stacked, limited) = databar_stacked.finish(step);
    results.append(&mut stacked);
    matrix_unfinished |= limited;
    let (mut expanded, limited) = expanded_stacked.finish(step);
    results.append(&mut expanded);
    matrix_unfinished |= limited;
    results.append(&mut matrix_results);
    observe("Linear", matrix_unfinished || regions.limited);
    linear_geometry::distinct(&mut results);
    suppress_itf_fragments(&mut results);
    let (regions, limited) = regions.finish(&results);
    observe("Finalize", matrix_unfinished || limited);
    Scan {
        barcodes: results,
        regions,
        unfinished: matrix_unfinished || limited,
        lines: line_id,
    }
}

/// Host routing capabilities supported by this frozen scanner version.
/// Bit 0 enables the shared all-format retail pass; absent exports in older
/// WASM keep the legacy host route for reproducible archived comparisons.
/// Bit 1 applies a stricter acceptance gate to supplemental retail reads.
/// Bit 2 supports packed RGBA upload and exact integer-luma preparation.
/// Bit 3 supports timing-guided curved QR grids at effort >=3.
#[must_use]
#[no_mangle]
pub extern "C" fn multi_capabilities() -> u32 {
    15
}

#[no_mangle]
pub extern "C" fn multi_new() -> *mut Session {
    Box::into_raw(Box::new(Session {
        input: vec![],
        rgba: vec![],
        output: vec![],
        width: 0,
        height: 0,
    }))
}
#[no_mangle]
/// Release a session created by `multi_new`.
///
/// # Safety
/// `s` must be null or an exclusively owned, live pointer returned by `multi_new`.
/// A non-null session and every buffer pointer obtained from it become invalid.
pub unsafe extern "C" fn multi_free(s: *mut Session) {
    if !s.is_null() {
        drop(Box::from_raw(s));
    }
}
#[no_mangle]
/// Resize the grayscale input buffer.
///
/// # Safety
/// `s` must be null or a live session pointer, exclusively accessible for this
/// call. Previously obtained input pointers must not be used after this call.
pub unsafe extern "C" fn multi_prepare(s: *mut Session, w: usize, h: usize) -> u32 {
    let Some(s) = s.as_mut() else {
        return 1;
    };
    let Some(n) = w.checked_mul(h) else {
        return 2;
    };
    if n > 32 * 1024 * 1024 || w < 1 || h < 1 {
        return 2;
    }
    s.input.resize(n, 0);
    s.width = w;
    s.height = h;
    0
}
#[no_mangle]
/// Obtain the prepared grayscale input buffer.
///
/// # Safety
/// `s` must be a live, non-null session pointer. The returned buffer permits
/// exactly the prepared width times height bytes until the next prepare/free.
/// Writes must not overlap another session operation.
pub unsafe extern "C" fn multi_input(s: *mut Session) -> *mut u8 {
    (*s).input.as_mut_ptr()
}
#[no_mangle]
/// Scan the prepared grayscale input and replace the serialized result.
///
/// # Safety
/// `s` must be null or a live session pointer, exclusively accessible for this
/// call. The prepared input must be initialized. Previous output pointers expire.
pub unsafe extern "C" fn multi_scan(s: *mut Session, mask: u32, effort: usize) -> u32 {
    let Some(s) = s.as_mut() else {
        return 1;
    };
    let result = scan(&s.input, s.width, s.height, mask, effort);
    s.output = serde_json::to_vec(&result).unwrap_or_default();
    0
}
#[no_mangle]
/// Obtain the serialized result buffer.
///
/// # Safety
/// `s` must be a live, non-null session pointer. Read at most `multi_output_len`
/// bytes, and do not retain the pointer across the next scan/free operation.
pub unsafe extern "C" fn multi_output(s: *mut Session) -> *const u8 {
    (*s).output.as_ptr()
}
#[no_mangle]
/// Obtain the serialized result length.
///
/// # Safety
/// `s` must be a live, non-null session pointer without concurrent mutation.
pub unsafe extern "C" fn multi_output_len(s: *mut Session) -> usize {
    (*s).output.len()
}

#[cfg(test)]
mod image_safety_tests {
    #[test]
    fn all_readers_accept_small_and_random_valid_buffers() {
        let mut state = 0x9026_0913_u32;
        for case in 0..96 {
            let w = if case < 16 {
                case % 4 + 1
            } else {
                17 + case * 13 % 91
            };
            let h = if case < 16 {
                case / 4 + 1
            } else {
                13 + case * 17 % 97
            };
            let pixels: Vec<u8> = (0..w * h)
                .map(|i| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    match case % 4 {
                        0 => (state >> 24) as u8,
                        1 => {
                            if state & 4 == 0 {
                                0
                            } else {
                                255
                            }
                        }
                        2 => {
                            if (i / w / 3 + i % w / 5) % 2 == 0 {
                                0
                            } else {
                                255
                            }
                        }
                        _ => 255,
                    }
                })
                .collect();
            let scan = super::scan(&pixels, w, h, 163_839, 1);
            assert!(scan.barcodes.iter().all(|r| r
                .polygon
                .iter()
                .flatten()
                .all(|p| p.is_finite())));
            assert!(scan
                .regions
                .iter()
                .all(|r| r.polygon.iter().flatten().all(|p| p.is_finite())));
        }
    }
}

#[cfg(test)]
mod threshold_tests {
    #[test]
    fn reflected_runs_match_reextracting_reversed_pixels() {
        let mut seed = 1487_u32;
        for length in [0, 1, 2, 9, 31, 128, 2049] {
            for kind in 0..4 {
                let mut bits: Vec<bool> = (0..length)
                    .map(|i| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        match kind {
                            0 => seed >> 31 != 0,
                            1 => i % 17 < 8,
                            2 => true,
                            _ => false,
                        }
                    })
                    .collect();
                let (mut widths, mut offsets) = super::runs(&bits);
                super::reverse_runs(&mut widths, &mut offsets, length);
                bits.reverse();
                assert_eq!((widths, offsets), super::runs(&bits));
            }
        }
    }
    use super::{runs, runs_into, threshold, threshold_into};

    #[test]
    fn reusable_threshold_and_run_buffers_match_allocating_wrappers() {
        let mut state = 0x0050_0511_u32;
        let mut bits_buffer = Vec::new();
        let mut widths_buffer = Vec::new();
        let mut offsets_buffer = Vec::new();
        for length in [0, 1, 2, 7, 24, 65, 257, 4097, 65, 2, 0, 257] {
            let row: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    state.to_be_bytes()[0]
                })
                .collect();
            for mode in 0..3 {
                let expected = threshold(&row, mode);
                threshold_into(&row, mode, &mut bits_buffer);
                assert_eq!(bits_buffer, expected);
                let expected_runs = runs(&bits_buffer);
                runs_into(&bits_buffer, &mut widths_buffer, &mut offsets_buffer);
                assert_eq!(
                    (&widths_buffer, &offsets_buffer),
                    (&expected_runs.0, &expected_runs.1)
                );
            }
        }
    }

    fn reference(row: &[u8], radius: usize) -> Vec<bool> {
        row.iter()
            .enumerate()
            .map(|(i, &pixel)| {
                let window = &row[i.saturating_sub(radius)..(i + radius + 1).min(row.len())];
                let sum: u64 = window.iter().map(|&value| u64::from(value)).sum();
                (u64::from(pixel) + 3) * u64::try_from(window.len()).unwrap() < sum
            })
            .collect()
    }

    #[test]
    fn adaptive_windows_match_reference_at_edges_and_on_noisy_rows() {
        let mut state = 0x9026_0913_u32;
        for length in [0, 1, 2, 23, 24, 25, 48, 49, 63, 64, 65, 128, 129, 257, 4097] {
            let noise: Vec<u8> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    state.to_be_bytes()[0]
                })
                .collect();
            for row in [noise, vec![0; length], vec![127; length], vec![255; length]] {
                for (mode, radius) in [(1, 24), (2, 64)] {
                    assert_eq!(threshold(&row, mode), reference(&row, radius));
                }
            }
        }
    }

    #[test]
    fn adaptive_sum_does_not_overflow_on_long_bright_rows() {
        // The old u32 prefix overflowed at this length, within the FFI's
        // 32-megapixel input limit. Each local window still sums to <= 32,895.
        let length = usize::try_from(u32::MAX / 255).unwrap() + 2;
        let row = vec![255; length];
        assert!(threshold(&row, 1).iter().all(|&black| !black));
    }
}

#[cfg(test)]
mod adaptive_split_tests {
    use super::{threshold, threshold_into};

    fn reference(row: &[u8], radius: usize) -> Vec<bool> {
        let mut sum: u32 = row.iter().take(radius + 1).map(|&v| u32::from(v)).sum();
        let mut count: u32 = row.iter().take(radius + 1).map(|_| 1).sum();
        row.iter()
            .enumerate()
            .map(|(i, &v)| {
                if i > radius {
                    sum -= u32::from(row[i - radius - 1]);
                    count -= 1;
                }
                if i > 0 && i + radius < row.len() {
                    sum += u32::from(row[i + radius]);
                    count += 1;
                }
                (u32::from(v) + 3) * count < sum
            })
            .collect()
    }

    #[test]
    fn split_adaptive_windows_match_reference_with_reused_shrinking_output() {
        let mut state = 0x0050_0515_u32;
        let mut output = Vec::new();
        for radius in [24_usize, 64] {
            for length in [
                0,
                1,
                radius,
                radius + 1,
                radius * 2,
                radius * 2 + 1,
                radius * 2 + 2,
                radius * 2 + 3,
                257,
                4097,
            ] {
                for kind in 0..3 {
                    let row: Vec<u8> = (0..length)
                        .map(|i| match kind {
                            0 => 0,
                            1 => 255,
                            _ => {
                                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                                state.to_be_bytes()[0].wrapping_add(i.to_le_bytes()[0])
                            }
                        })
                        .collect();
                    let expected = reference(&row, radius);
                    threshold_into(&row, radius / 40 + 1, &mut output);
                    assert_eq!(output, expected);
                    assert_eq!(threshold(&row, radius / 40 + 1), expected);
                }
            }
        }
    }
}

#[cfg(test)]
mod packed_run_tests {
    use super::{run_offsets_into, runs_into};

    #[expect(
        clippy::cast_precision_loss,
        reason = "Scalar reference intentionally mirrors production usize-to-f32 run-width conversion."
    )]
    fn reference(bits: &[bool]) -> (Vec<f32>, Vec<usize>) {
        let mut widths = Vec::new();
        let mut offsets = vec![0];
        if bits.is_empty() {
            return (widths, offsets);
        }
        let mut at = 0;
        for i in 1..bits.len() {
            if bits[i] != bits[at] {
                widths.push((i - at) as f32);
                offsets.push(i);
                at = i;
            }
        }
        widths.push((bits.len() - at) as f32);
        offsets.push(bits.len());
        (widths, offsets)
    }

    #[test]
    fn packed_transitions_match_scalar_reference_at_boundaries() {
        let mut state = 0x0050_0516_u32;
        for length in [
            0, 1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 257, 4097,
        ] {
            for kind in 0..4 {
                let bits: Vec<bool> = (0..length)
                    .map(|i| match kind {
                        0 => false,
                        1 => i % 2 == 0,
                        2 => i % 17 < 8,
                        _ => {
                            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                            state & 1 != 0
                        }
                    })
                    .collect();
                let expected = reference(&bits);
                let mut widths = Vec::new();
                let mut offsets = Vec::new();
                runs_into(&bits, &mut widths, &mut offsets);
                assert_eq!((widths, offsets.clone()), expected);
                run_offsets_into(&bits, &mut offsets);
                let expected_offsets = if bits.is_empty() {
                    vec![0, 0]
                } else {
                    expected.1.clone()
                };
                assert_eq!(offsets, expected_offsets);
            }
        }
    }
}

#[no_mangle]
/// Prepare a packed RGBA input buffer, retaining the integer grayscale rule.
///
/// # Safety
/// `s` must be null or a live exclusively accessible session. All earlier input
/// pointers expire; initialize exactly width*height*4 bytes before scanning.
pub unsafe extern "C" fn multi_prepare_rgba(s: *mut Session, w: usize, h: usize) -> u32 {
    let status = multi_prepare(s, w, h);
    if status != 0 {
        return status;
    }
    (*s).rgba.resize(w * h * 4, 0);
    0
}
#[no_mangle]
/// Obtain the prepared packed RGBA buffer.
///
/// # Safety
/// `s` must be live and prepared by `multi_prepare_rgba`. The pointer expires
/// at the next prepare/free call; no session operation may overlap a write.
pub unsafe extern "C" fn multi_input_rgba(s: *mut Session) -> *mut u8 {
    (*s).rgba.as_mut_ptr()
}
#[no_mangle]
/// Convert packed RGBA and scan; alpha is ignored exactly as in the JS host.
///
/// # Safety
/// `s` must be null or a live exclusively accessible session with initialized
/// RGBA pixels. Previous output pointers expire at this call.
pub unsafe extern "C" fn multi_scan_rgba(s: *mut Session, mask: u32, effort: usize) -> u32 {
    let Some(state) = s.as_mut() else {
        return 1;
    };
    if state.rgba.len() != state.input.len() * 4 {
        return 3;
    }
    for (gray, pixel) in state.input.iter_mut().zip(state.rgba.chunks_exact(4)) {
        let value =
            (u16::from(pixel[0]) * 77 + u16::from(pixel[1]) * 150 + u16::from(pixel[2]) * 29 + 128)
                >> 8;
        *gray = value.to_le_bytes()[0];
    }
    multi_scan(s, mask, effort)
}

#[cfg(test)]
mod qr_speed_regressions {
    #[test]
    fn histogram_lanes_match_scalar_counts_across_admission_and_tail_boundaries() {
        let mut state = 0xa511_e9b3_u32;
        for size in [0, 1, 2047, 2048, 2049, 2050, 2051, 4095, 4096, 65_537] {
            let mut input = vec![0_u8; size];
            for pixel in &mut input {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *pixel = state.to_le_bytes()[3];
            }
            let mut reference = [0_usize; 256];
            for &pixel in &input {
                reference[usize::from(pixel)] += 1;
            }
            assert_eq!(super::histogram(&input), reference);
            input.fill(255);
            let mut constant = [0_usize; 256];
            constant[255] = size;
            assert_eq!(super::histogram(&input), constant);
        }
    }
    #[test]
    fn local_prefix_wrap_is_exact_on_a_wide_bright_image() {
        let width = 520_000;
        let height = 33;
        let image = vec![255_u8; width * height];
        // The full row prefix exceeds u32; every 33x33 local sum still fits.
        let threshold = crate::qr_detect::binarize(&image, width, height, true);
        assert_eq!(threshold.len(), image.len());
        assert!(threshold.iter().all(|&bit| !bit));
    }
}

/// A strong full ITF read can explain a weak shorter fragment of the same bars.
/// Require both numeric-pair alignment and an almost-contained, parallel region.
fn suppress_itf_fragments(reads: &mut Vec<Detection>) {
    let mut keep = vec![true; reads.len()];
    for (i, short) in reads.iter().enumerate() {
        if short.format != "ITF" {
            continue;
        }
        for long in reads.iter() {
            if long.format != "ITF"
                || long.text.len() <= short.text.len()
                || long.support < short.support.saturating_mul(2)
                || !long
                    .text
                    .match_indices(&short.text)
                    .any(|(at, _)| at % 2 == 0)
            {
                continue;
            }
            let origin = long.polygon[0];
            let axis = [
                long.polygon[1][0] - origin[0],
                long.polygon[1][1] - origin[1],
            ];
            let side = [
                long.polygon[3][0] - origin[0],
                long.polygon[3][1] - origin[1],
            ];
            let determinant = axis[0] * side[1] - axis[1] * side[0];
            if determinant.abs() < 1. {
                continue;
            }
            let short_axis = [
                short.polygon[1][0] - short.polygon[0][0],
                short.polygon[1][1] - short.polygon[0][1],
            ];
            let lengths = axis[0].hypot(axis[1]) * short_axis[0].hypot(short_axis[1]);
            if (axis[0] * short_axis[0] + axis[1] * short_axis[1]).abs() < lengths * 0.98 {
                continue;
            }
            // Corner slack describes uncertain bounds of the same bars. It
            // must not absorb a separate symbol outside the actual long band.
            let center = short.polygon.iter().fold([0_f32; 2], |sum, point| {
                [sum[0] + point[0] * 0.25, sum[1] + point[1] * 0.25]
            });
            let x = center[0] - origin[0];
            let y = center[1] - origin[1];
            let center_u = (x * side[1] - y * side[0]) / determinant;
            let center_v = (axis[0] * y - axis[1] * x) / determinant;
            if !(0.0..=1.0).contains(&center_u) || !(0.0..=1.0).contains(&center_v) {
                continue;
            }
            let inside = short.polygon.iter().all(|p| {
                let x = p[0] - origin[0];
                let y = p[1] - origin[1];
                let u = (x * side[1] - y * side[0]) / determinant;
                let v = (axis[0] * y - axis[1] * x) / determinant;
                (-0.03..=1.03).contains(&u) && (-0.08..=1.08).contains(&v)
            });
            if inside {
                keep[i] = false;
                break;
            }
        }
    }
    let mut index = 0;
    reads.retain(|_| {
        let retained = keep[index];
        index += 1;
        retained
    });
}

#[cfg(test)]
mod itf_fragment_tests {
    use super::{suppress_itf_fragments, Detection};

    #[test]
    fn a_nearby_thin_symbol_is_not_a_fragment_of_the_long_one() {
        let read = |text: &str, top, bottom, support| Detection {
            bytes: None,
            structured_append: None,
            reader_initialization: false,
            addon: None,
            format: "ITF".into(),
            text: text.into(),
            polygon: [[0., top], [100., top], [100., bottom], [0., bottom]],
            support,
            error: 0.,
            gs1: false,
        };
        let long = read("123456789012", 0., 120., 60);
        let mut adjacent = vec![long.clone(), read("345678", 122., 128., 3)];
        suppress_itf_fragments(&mut adjacent);
        assert_eq!(adjacent.len(), 2);
        let mut contained = vec![long, read("345678", 30., 60., 3)];
        suppress_itf_fragments(&mut contained);
        assert_eq!(contained.len(), 1);
        assert_eq!(contained[0].text, "123456789012");
    }
}

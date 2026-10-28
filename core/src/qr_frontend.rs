//! QR-only production frontend: the shared multiformat detector under this
//! mode's [`Policy`], plus block-threshold, evidence, finder-pair window and sharpened
//! retries. The multi-format groups use the same detector and policy without the
//! window retry.
use barcode_multiformat::numeric::{f32_usize, usize_f32};
use barcode_multiformat::qr_detect::{self, FULL_BUDGET, Pass, Policy, Symbols};
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
/// [`scan_with_effort`] with a threshold cache of the same `image` that other readers
/// of the frame share. Thresholds are deterministic, so the scan equals one with a
/// fresh cache.
#[must_use]
pub fn scan_with_images(
    image: &[u8],
    width: usize,
    height: usize,
    effort: usize,
    binary: &mut binarization::Images<'_>,
) -> Scan {
    scan_frame(image, width, height, false, effort, binary, false)
}
/// Retry the two additional foreground thresholds on an unresolved source image.
#[must_use]
pub fn scan_foreground(image: &[u8], width: usize, height: usize) -> Scan {
    scan_impl(image, width, height, true, 0)
}
/// Decode attempts for each block-threshold pass.
const HYBRID_BUDGET: usize = 200;
fn scan_impl(image: &[u8], width: usize, height: usize, foreground: bool, effort: usize) -> Scan {
    let mut binary = binarization::Images::new(image, width, height);
    scan_frame(image, width, height, foreground, effort, &mut binary, false)
}

/// Windows around unread finder pairs searched again per frame. Low serves live
/// frames, where the next frame is the retry. High and above read more symbols before
/// their windows, so their unread finders pair differently from Medium's; more windows
/// let them reach at least the symbols Medium's windows reach.
const WINDOWS: usize = if cfg!(feature = "mode-low") {
    0
} else if cfg!(feature = "mode-medium") {
    3
} else {
    12
};
/// Longest side in pixels of the image a window is scanned in. It bounds the cost
/// of a window: a larger window is scanned on an area-averaged copy reduced by an
/// integer factor to fit.
const WINDOW_MAX: usize = 640;
/// Longest window side in frame pixels. Large symbols beside a read can lose a
/// finder to the frame-wide thresholds too, such as dark symbols on a sheet that
/// a lamp lights unevenly.
const WINDOW_REACH: f32 = 3. * usize_f32(WINDOW_MAX);
/// Share of a window's half side within which a read's center skips the window. Medium
/// skips any window that holds a read. High, which reads more symbols before its windows,
/// keeps windows that hold a read away from their core, so it searches at least the
/// symbols beside a read that Medium's windows reach.
const READ_CENTER_REACH: f32 = if cfg!(feature = "mode-medium") {
    1.
} else {
    0.5
};
/// Smallest module width in pixels a window may hold once scanned (reduced or not). Pairs
/// of finders this small rarely form a symbol a window reads; skipping them leaves the
/// window for a pair that does.
const WINDOW_MODULE: f32 = 2.5;
/// Reduced windows per frame. Each costs a full window scan plus the reduction, on
/// frames that mostly read nothing more: Medium takes one, High and above three
/// (Low has none).
const REDUCED_WINDOWS: usize = if cfg!(feature = "mode-medium") { 1 } else { 3 };
/// Decode attempts of each window pass. A window holds one candidate symbol, whose
/// triples rank first; the rest are triples of texture around it.
const WINDOW_BUDGET: usize = 60;
/// Threshold modes of a window's first pass: the global and local thresholds,
/// recomputed for the window at full row density.
const WINDOW_MODES: std::ops::Range<usize> = 0..2;
/// Threshold modes of a window's second pass: the foreground split, which separates a
/// symbol from a dark surround. On the development sets the inverse and upper-contrast
/// modes read nothing further in windows.
const WINDOW_FOREGROUND: std::ops::Range<usize> = 4..6;
/// Windows run at Medium's effort in every mode: the window is the retry.
const WINDOW_EFFORT: usize = 1;
/// Cell size in pixels of the finder counts that rank pairs.
const COUNT_CELL: usize = 16;

fn center(polygon: &[[f32; 2]; 4]) -> [f32; 2] {
    polygon
        .iter()
        .fold([0., 0.], |c, p| [c[0] + p[0] / 4., c[1] + p[1] / 4.])
}

/// Whether `point` lies in `polygon` grown by a quarter about its center, the margin
/// in which a read's own finders and quiet-zone patterns lie.
fn near(point: [f32; 2], polygon: &[[f32; 2]; 4]) -> bool {
    let [cx, cy] = center(polygon);
    let grown = polygon.map(|p| [cx + (p[0] - cx) * 1.25, cy + (p[1] - cy) * 1.25]);
    let mut sign = 0.;
    (0..4).all(|i| {
        let (a, b) = (grown[i], grown[(i + 1) % 4]);
        let side = (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0]);
        if sign == 0. {
            sign = side.signum();
        }
        side * sign >= 0.
    })
}

/// Finder counts over a grid of [`COUNT_CELL`] cells as a summed-area table, so each
/// pair's window count costs four lookups.
struct Counts {
    columns: usize,
    rows: usize,
    table: Vec<u32>,
}
impl Counts {
    fn new(points: &[[f32; 4]], width: usize, height: usize) -> Self {
        let columns = width.div_ceil(COUNT_CELL);
        let rows = height.div_ceil(COUNT_CELL);
        let stride = columns + 1;
        let mut table = vec![0u32; stride * (rows + 1)];
        for p in points {
            let x = (f32_usize(p[0].max(0.)) / COUNT_CELL).min(columns - 1);
            let y = (f32_usize(p[1].max(0.)) / COUNT_CELL).min(rows - 1);
            table[(y + 1) * stride + x + 1] += 1;
        }
        for y in 1..=rows {
            for x in 1..=columns {
                table[y * stride + x] += table[(y - 1) * stride + x] + table[y * stride + x - 1]
                    - table[(y - 1) * stride + x - 1];
            }
        }
        Self {
            columns,
            rows,
            table,
        }
    }
    /// Finders in the cells that the square of half side `half` about `middle` touches.
    fn around(&self, middle: [f32; 2], half: f32) -> u32 {
        let cell = usize_f32(COUNT_CELL);
        let span = |v: f32, count: usize| {
            let first = f32_usize((v - half).max(0.) / cell).min(count);
            let last = (f32_usize((v + half).max(0.) / cell) + 1).min(count);
            (first, last.max(first))
        };
        let (x0, x1) = span(middle[0], self.columns);
        let (y0, y1) = span(middle[1], self.rows);
        let stride = self.columns + 1;
        let at = |x: usize, y: usize| self.table[y * stride + x];
        at(x1, y1) + at(x0, y0) - at(x0, y1) - at(x1, y0)
    }
}

/// Windows around pairs of unread QR finders: two corners of a symbol whose third
/// finder the frame's row step or thresholds lost.
///
/// A pair needs modules within a factor 1.43 and a distance of 12 to 75 modules
/// (version 1 side to version 10 diagonal); the window must fit [`WINDOW_REACH`],
/// and its modules, after the reduction it needs, must be [`WINDOW_MODULE`] wide. The
/// square window reaches 1.15 finder distances from the pair's midpoint, which covers
/// a symbol on either side of a side pair (its far corners lie 1.12 distances away),
/// plus 8 modules of finder half-width and quiet zone. Bars and text produce clusters
/// of finder candidates while a symbol's data area holds few, so pairs rank by
/// support over the candidates around them. A window that holds a read's center (within
/// [`READ_CENTER_REACH`]), or whose pair lies in the core of a chosen window, is skipped.
fn local_windows(
    finders: &[[f32; 4]],
    reads: &[Detection],
    width: usize,
    height: usize,
    limit: usize,
) -> Vec<[usize; 4]> {
    if limit == 0 {
        return Vec::new();
    }
    let mut unread: Vec<[f32; 4]> = Vec::new();
    for f in finders {
        if reads.iter().any(|r| near([f[0], f[1]], &r.polygon)) {
            continue;
        }
        // The threshold modes find the same pattern repeatedly; keep the best support.
        if let Some(same) = unread
            .iter_mut()
            .find(|u| (u[0] - f[0]).hypot(u[1] - f[1]) < 1.5 * u[2].max(f[2]))
        {
            if f[3] > same[3] {
                *same = *f;
            }
        } else {
            unread.push(*f);
        }
    }
    if unread.len() < 2 {
        return Vec::new();
    }
    let counts = Counts::new(&unread, width, height);
    let read_centers: Vec<[f32; 2]> = reads.iter().map(|r| center(&r.polygon)).collect();
    let mut pairs = Vec::new();
    for (i, a) in unread.iter().enumerate() {
        for b in &unread[i + 1..] {
            let module = f32::midpoint(a[2], b[2]);
            let distance = (a[0] - b[0]).hypot(a[1] - b[1]);
            let half = 1.15 * distance + 8. * module;
            let reduction = (2. * half / usize_f32(WINDOW_MAX)).ceil().max(1.);
            if !(0.7..=1.43).contains(&(a[2] / b[2]))
                || !(12. ..=75.).contains(&(distance / module))
                || 2. * half > WINDOW_REACH
                || module / reduction < WINDOW_MODULE
            {
                continue;
            }
            let middle = [f32::midpoint(a[0], b[0]), f32::midpoint(a[1], b[1])];
            if read_centers.iter().any(|c| {
                (c[0] - middle[0]).abs() < half * READ_CENTER_REACH
                    && (c[1] - middle[1]).abs() < half * READ_CENTER_REACH
            }) {
                continue;
            }
            let crowd = counts.around(middle, half).max(2);
            pairs.push(((a[3] + b[3]) / usize_f32(crowd as usize), middle, half));
        }
    }
    pairs.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (wf, hf) = (usize_f32(width), usize_f32(height));
    // Whether the window, clamped to the frame as it is scanned, needs a reduction.
    let reduced = |[x, y]: [f32; 2], half: f32| {
        let side = |c: f32, max: f32| (c + half).min(max) - (c - half).max(0.);
        side(x, wf).max(side(y, hf)) > usize_f32(WINDOW_MAX)
    };
    let mut chosen: Vec<([f32; 2], f32)> = Vec::new();
    for (_, middle, half) in pairs {
        if chosen.len() == limit {
            break;
        }
        if reduced(middle, half)
            && chosen.iter().filter(|&&(m, h)| reduced(m, h)).count() >= REDUCED_WINDOWS
        {
            continue;
        }
        if !chosen
            .iter()
            .any(|(c, h)| (c[0] - middle[0]).abs() < h / 2. && (c[1] - middle[1]).abs() < h / 2.)
        {
            chosen.push((middle, half));
        }
    }
    chosen
        .into_iter()
        .map(|([x, y], half)| {
            let clamp = |v: f32, max: f32| f32_usize(v.clamp(0., max));
            [
                clamp(x - half, wf),
                clamp(y - half, hf),
                clamp(x + half, wf),
                clamp(y + half, hf),
            ]
        })
        // A window covering most of the frame at full resolution repeats the frame's own
        // search; a reduced one does not, as its coarser grid sees the symbol differently.
        .filter(|&[x0, y0, x1, y1]| {
            x1 > x0 + 16
                && y1 > y0 + 16
                && (4 * (x1 - x0) * (y1 - y0) <= width * height
                    || (x1 - x0).max(y1 - y0) > WINDOW_MAX)
        })
        .collect()
}

/// The pixels of window `[x0, y0, x1, y1]` of a frame `width` pixels wide, averaged
/// over `factor` x `factor` blocks; a partial last block is dropped.
fn window_pixels(
    image: &[u8],
    width: usize,
    [x0, y0, x1, y1]: [usize; 4],
    factor: usize,
) -> (Vec<u8>, usize, usize) {
    let (w, h) = ((x1 - x0) / factor, (y1 - y0) / factor);
    if factor == 1 {
        let crop = (y0..y1)
            .flat_map(|y| image[y * width + x0..y * width + x1].iter().copied())
            .collect();
        return (crop, w, h);
    }
    let area = factor * factor;
    let mut sums = vec![0_usize; w];
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        sums.fill(0);
        for row in y0 + y * factor..y0 + (y + 1) * factor {
            let pixels = &image[row * width + x0..row * width + x0 + w * factor];
            for (sum, block) in sums.iter_mut().zip(pixels.chunks_exact(factor)) {
                *sum += block.iter().map(|&v| usize::from(v)).sum::<usize>();
            }
        }
        out.extend(
            sums.iter()
                // A rounded mean of bytes fits a byte.
                .map(|&sum| u8::try_from((sum + area / 2) / area).unwrap_or(u8::MAX)),
        );
    }
    (out, w, h)
}

/// Scan one window with its own thresholds, then its foreground split if that read
/// nothing, and map the reads back to frame coordinates. Windows beyond
/// [`WINDOW_MAX`] are scanned reduced.
fn scan_window(image: &[u8], width: usize, window: [usize; 4]) -> Vec<Detection> {
    let [x0, y0, x1, y1] = window;
    let factor = (x1 - x0).max(y1 - y0).div_ceil(WINDOW_MAX).max(1);
    let (crop, w, h) = window_pixels(image, width, window, factor);
    let mut binary = binarization::Images::new(&crop, w, h);
    let mut reads = scan_frame(&crop, w, h, false, WINDOW_EFFORT, &mut binary, true).barcodes;
    if reads.is_empty() {
        reads = scan_frame(&crop, w, h, true, 0, &mut binary, true).barcodes;
    }
    let (dx, dy, scale) = (usize_f32(x0), usize_f32(y0), usize_f32(factor));
    for read in &mut reads {
        for p in &mut read.polygon {
            p[0] = p[0] * scale + dx;
            p[1] = p[1] * scale + dy;
        }
    }
    reads
}

fn scan_frame(
    image: &[u8],
    width: usize,
    height: usize,
    foreground: bool,
    effort: usize,
    binary: &mut binarization::Images<'_>,
    window: bool,
) -> Scan {
    if width == 0 || height == 0 || width.checked_mul(height).is_none_or(|n| n > image.len()) {
        return Scan {
            barcodes: vec![],
            regions: vec![],
            unfinished: false,
            lines: 0,
        };
    }
    let mut regions = barcode_multiformat::regions::Regions::default();
    // The foreground retry tries only the additional foreground splits; the
    // base threshold passes remain owned by the caller.
    let pass = match (foreground, window) {
        (true, false) => Pass {
            modes: 4..8,
            curved: false,
            budget: FULL_BUDGET,
        },
        (false, false) => Pass::effort(effort > 1, effort > 2),
        (true, true) => Pass {
            modes: WINDOW_FOREGROUND,
            curved: false,
            budget: WINDOW_BUDGET,
        },
        (false, true) => Pass {
            modes: WINDOW_MODES,
            curved: false,
            budget: WINDOW_BUDGET,
        },
    };
    let (mut reads, mut limited) = detect(width, height, &mut regions, binary, pass);
    // Block thresholds follow sharp shadow edges and large modules that the
    // 33-pixel local mean misses. Medium tries them where a QR candidate stayed
    // unread; High and Very High on every unread frame. Each pass has a bounded
    // budget and stops quietly.
    if !foreground
        && !window
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
    // Low retries a frame that read nothing but holds QR evidence.
    if !foreground && !window {
        qr_detect::evidence_retry(
            image,
            [width, height],
            &mut reads,
            &regions,
            binary,
            POLICY,
            Symbols::QR,
        );
    }
    // A second symbol whose third finder was lost is searched in windows around its
    // finder pairs. Only beside a read: a frame without one takes the facade's
    // foreground and reduced-copy retries, which a window read would suppress.
    if !window && !foreground && !reads.is_empty() {
        for found in local_windows(&regions.qr_finders, &reads, width, height, WINDOWS) {
            reads.extend(scan_window(image, width, found));
        }
    }
    if !foreground && effort > 1 && width * height <= qr_detect::SHARPEN_MAX_PIXELS {
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

#[cfg(test)]
mod tests {
    use super::{
        REDUCED_WINDOWS, WINDOW_MAX, WINDOW_REACH, local_windows, scan_window, scan_with_effort,
        window_pixels,
    };

    /// Version 1-M "TAPIRSCAN", drawn dark on light with `scale`-pixel modules at
    /// `origin` in a `width` x `height` frame.
    fn frame(width: usize, height: usize, origin: [usize; 2], scale: usize) -> Vec<u8> {
        let rows = [
            "111111101101001111111",
            "100000101100001000001",
            "101110100011001011101",
            "101110101110101011101",
            "101110100001001011101",
            "100000100111101000001",
            "111111101010101111111",
            "000000001111100000000",
            "101101110111101001011",
            "000001001001111100011",
            "011110110111000001001",
            "100011010001001011111",
            "001100100010100100000",
            "000000001101001010111",
            "111111101111100101101",
            "100000101010000110100",
            "101110100010111000111",
            "101110101101001101010",
            "101110101000101100000",
            "100000100110010100100",
            "111111101000010011010",
        ];
        let mut gray = vec![220; width * height];
        for (y, row) in rows.iter().enumerate() {
            for (x, bit) in row.bytes().enumerate() {
                if bit == b'1' {
                    for dy in 0..scale {
                        let start = (origin[1] + y * scale + dy) * width + origin[0] + x * scale;
                        gray[start..start + scale].fill(30);
                    }
                }
            }
        }
        gray
    }

    #[test]
    fn window_reads_keep_source_coordinates() {
        let (width, height) = (400, 300);
        let gray = frame(width, height, [150, 90], 4);
        let whole = scan_with_effort(&gray, width, height, 1).barcodes;
        assert_eq!(whole.len(), 1);
        let local = scan_window(&gray, width, [100, 50, 300, 250]);
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].text, "TAPIRSCAN");
        for (a, b) in local[0].polygon.iter().zip(&whole[0].polygon) {
            assert!(
                (a[0] - b[0]).abs() <= 1. && (a[1] - b[1]).abs() <= 1.,
                "{a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn large_windows_read_reduced_in_source_coordinates() {
        let (width, height) = (1000, 900);
        let gray = frame(width, height, [300, 250], 16);
        let whole = scan_with_effort(&gray, width, height, 1).barcodes;
        assert_eq!(whole.len(), 1);
        // A 900 x 800 window is scanned reduced by 2.
        let local = scan_window(&gray, width, [50, 50, 950, 850]);
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].text, "TAPIRSCAN");
        for (a, b) in local[0].polygon.iter().zip(&whole[0].polygon) {
            assert!(
                (a[0] - b[0]).abs() <= 4. && (a[1] - b[1]).abs() <= 4.,
                "{a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn reduced_windows_average_whole_blocks() {
        let image: Vec<u8> = (0..36).map(|v| v * 7).collect();
        assert_eq!(
            window_pixels(&image, 6, [1, 1, 4, 3], 1).0,
            [49, 56, 63, 91, 98, 105]
        );
        // Blocks of 2 x 2 from (1, 1); the partial last column is dropped.
        let (reduced, w, h) = window_pixels(&image, 6, [1, 1, 6, 5], 2);
        assert_eq!((w, h), (2, 2));
        assert_eq!(reduced, [74, 88, 158, 172]);
    }

    #[test]
    fn reduced_windows_per_frame_are_bounded() {
        // Three distant pairs with 12-pixel modules, each needing a reduction by 3.
        let finders: Vec<[f32; 4]> = [1000., 3000., 5000.]
            .iter()
            .flat_map(|&x| [[x, 1000., 12., 9.], [x, 1600., 12., 9.]])
            .collect();
        let windows = local_windows(&finders, &[], 6000, 6000, 3);
        assert_eq!(windows.len(), REDUCED_WINDOWS.min(3), "{windows:?}");
        assert!(windows.iter().all(|w| w[2] - w[0] > WINDOW_MAX));
    }

    #[test]
    fn windows_need_a_plausible_unread_pair() {
        let (width, height) = (1000, 1000);
        // Two finders 100 pixels apart with 4-pixel modules: one window around them.
        let pair = [[400., 400., 4., 3.], [400., 500., 4., 3.]];
        let windows = local_windows(&pair, &[], width, height, 3);
        assert_eq!(windows, vec![[253, 303, 547, 597]]);
        // No windows without a limit, without a second finder, for unequal modules,
        // for finders closer than a symbol side, or for windows wider than the bound.
        assert_eq!(
            local_windows(&pair, &[], width, height, 0),
            Vec::<[usize; 4]>::new()
        );
        assert_eq!(
            local_windows(&pair[..1], &[], width, height, 3),
            Vec::<[usize; 4]>::new()
        );
        let unequal = [pair[0], [400., 500., 8., 3.]];
        assert_eq!(
            local_windows(&unequal, &[], width, height, 3),
            Vec::<[usize; 4]>::new()
        );
        let close = [pair[0], [400., 440., 4., 3.]];
        assert_eq!(
            local_windows(&close, &[], width, height, 3),
            Vec::<[usize; 4]>::new()
        );
        let far = [
            [100., 100., 4., 3.],
            [100., 100. + WINDOW_REACH / 2., 4., 3.],
        ];
        assert_eq!(
            local_windows(&far, &[], width, height, 3),
            Vec::<[usize; 4]>::new()
        );
        // Nor for 2-pixel modules, at full resolution or in a 754-pixel window halved.
        let small = [[400., 400., 2., 3.], [400., 500., 2., 3.]];
        assert_eq!(
            local_windows(&small, &[], width, height, 3),
            Vec::<[usize; 4]>::new()
        );
        let fine = [[400., 400., 4., 3.], [400., 700., 4., 3.]];
        assert_eq!(
            local_windows(&fine, &[], width, height, 3),
            Vec::<[usize; 4]>::new()
        );
        // A window covering most of a small frame repeats the frame's own search...
        let wide = [[150., 100., 4., 3.], [150., 200., 4., 3.]];
        assert_eq!(local_windows(&wide, &[], 300, 300, 3).len(), 0);
        // ...but not when it is scanned reduced.
        let large = [[500., 300., 8., 3.], [500., 700., 8., 3.]];
        assert_eq!(local_windows(&large, &[], 1100, 1000, 3).len(), 1);
        // Windows are clamped to the frame.
        let corner = [[10., 10., 4., 3.], [10., 110., 4., 3.]];
        assert_eq!(
            local_windows(&corner, &[], width, height, 3),
            vec![[0, 0, 157, 207]]
        );
    }

    #[test]
    fn read_symbols_add_no_windows() {
        let (width, height) = (400, 300);
        let gray = frame(width, height, [150, 90], 4);
        let reads = scan_with_effort(&gray, width, height, 1).barcodes;
        // The read's own corners would pair, but they lie within the read.
        let corners: Vec<[f32; 4]> = reads[0]
            .polygon
            .iter()
            .map(|p| [p[0], p[1], 4., 5.])
            .collect();
        assert_eq!(
            local_windows(&corners, &reads, width, height, 3),
            Vec::<[usize; 4]>::new()
        );
    }
}

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
pub mod pdf417;
mod pdf417_tables;
pub mod pdf_localize;
pub mod qr;
pub mod qr_detect;
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
    output: Vec<u8>,
    width: usize,
    height: usize,
}

fn threshold(row: &[u8], mode: usize) -> Vec<bool> {
    if mode == 0 {
        let mut hist = [0usize; 256];
        for &v in row {
            hist[v as usize] += 1;
        }
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
            let delta = left as f64 / mass as f64 - (sum - left) as f64 / (row.len() - mass) as f64;
            let score = mass as f64 * (row.len() - mass) as f64 * delta * delta;
            if score > best {
                best = score;
                cut = i;
            }
        }
        row.iter().map(|&v| v as usize <= cut).collect()
    } else {
        let radius = if mode == 1 { 24 } else { 64 };
        // Only the local window is needed. Its at most 129 pixels fit in u32,
        // unlike a cumulative sum over an arbitrarily long scanline.
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
}
fn runs(bits: &[bool]) -> (Vec<f32>, Vec<usize>) {
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

fn run_offsets(bits: &[bool]) -> Vec<usize> {
    let mut offsets = vec![0];
    for i in 1..bits.len() {
        if bits[i] != bits[i - 1] {
            offsets.push(i);
        }
    }
    offsets.push(bits.len());
    offsets
}
fn refined_runs(bits: &[bool], row: &[u8], reverse: bool) -> (Vec<f32>, Vec<usize>) {
    refine_run_offsets(run_offsets(bits), row, reverse)
}
#[inline]
fn refined_edge(i: usize, row: &[u8], reverse: bool) -> f32 {
    if i == 0 || i == row.len() {
        return i as f32;
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
    i as f32 - 0.5 + frac
}
fn refine_run_offsets(offsets: Vec<usize>, row: &[u8], reverse: bool) -> (Vec<f32>, Vec<usize>) {
    let mut widths = Vec::with_capacity(offsets.len() - 1);
    let mut last_edge = 0.;
    for &i in offsets.iter().skip(1).take(offsets.len().saturating_sub(2)) {
        let edge = refined_edge(i, row, reverse);
        widths.push(edge - last_edge);
        last_edge = edge;
    }
    widths.push(row.len() as f32 - last_edge);
    (widths, offsets)
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
            let x = (along * cos_angle - cross * sin_angle).round() as isize;
            let y = (along * sin_angle + cross * cos_angle).round() as isize;
            if x < 1 || y < 1 || x >= width as isize - 1 || y >= height as isize - 1 {
                continue;
            }
            let at = y as usize * width + x as usize;
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
        qr_detect::detect(width, height, &mut regions, &mut binary_images)
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
        let (mut reads, limited) =
            aztec_detect::detect(width, height, &mut regions, &mut binary_images);
        matrix_results.append(&mut reads);
        matrix_unfinished |= limited;
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
    let step = (width.min(height) / if effort > 0 { 120 } else { 64 }).clamp(2, 16) as f32;
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
    let mut line_id = 0;
    for &deg in angles {
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
            [width as f32 - 1., 0.],
            [width as f32 - 1., height as f32 - 1.],
            [0., height as f32 - 1.],
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
        let mut b = bmin + (step * 0.5).min((bmax - bmin) * 0.5);
        while b <= bmax {
            let mut row = Vec::new();
            let mut start = amin;
            #[expect(
                clippy::float_cmp,
                reason = "Preset angles are exact constants; equality selects axial fast paths."
            )]
            if deg == 0. {
                let y = b.round() as usize;
                row.extend_from_slice(&image[y * width..(y + 1) * width]);
            } else if deg == 90. {
                let x = (-b).round() as usize;
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
                        let z = (extent as f32 - 0.5 - offset) / coefficient;
                        lo = lo.max(a.min(z));
                        hi = hi.min(a.max(z));
                    }
                }
                let first = ((lo - amin).floor() as isize - 1).max(0) as usize;
                let last = ((hi - amin).ceil() as isize + 1).max(0) as usize;
                for i in first..=last.min((amax - amin).ceil() as usize) {
                    let a = amin + i as f32;
                    let x = (a * cos_angle - b * sin_angle).round() as isize;
                    let y = (a * sin_angle + b * cos_angle).round() as isize;
                    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                        if row.is_empty() {
                            start = a;
                        }
                        row.push(image[y as usize * width + x as usize]);
                    } else if !row.is_empty() {
                        break;
                    }
                }
            }
            line_id += 1;
            if row.len() >= 40 && row.iter().any(|&v| v != row[0]) {
                let mut previous_bits = Vec::new();
                let passes = if effort == 0 {
                    1
                } else if mask & linear::EAN8 != 0 && (deg.abs() < 0.01 || (deg - 90.).abs() < 0.01)
                {
                    5
                } else {
                    2
                };
                for mode in 0..passes {
                    let (bits, mut r, mut offsets) = if mode == 2 {
                        retail_profile::runs(&row)
                    } else if mode >= 3 {
                        let sharpened = retail_profile::sharpen_row(&row);
                        let bits = threshold(&sharpened, mode - 3);
                        let (r, offsets) = refined_runs(&bits, &sharpened, false);
                        (bits, r, offsets)
                    } else {
                        let bits = threshold(&row, mode);
                        let (r, offsets) = refined_runs(&bits, &row, false);
                        (bits, r, offsets)
                    };
                    if mode < 2 && bits == previous_bits {
                        continue;
                    }
                    let mask = if mode >= 2 {
                        mask & (linear::EAN8 | linear::ADDON_READ | linear::ADDON_REQUIRE)
                    } else {
                        mask
                    };
                    let mut integer_runs = if mask & linear::CODABAR != 0 {
                        runs(&bits).0
                    } else {
                        vec![]
                    };
                    for reverse in [false, true] {
                        if reverse {
                            r.reverse();
                            offsets.reverse();
                            for offset in &mut offsets {
                                *offset = row.len() - *offset;
                            }
                            integer_runs.reverse();
                        }
                        let Some(&first_black) = (if reverse { bits.last() } else { bits.first() })
                        else {
                            continue;
                        };
                        if mask & linear::DATABAR != 0 {
                            databar_stacked.observe(databar::ScanLine {
                                runs: &r,
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
                                runs: &r,
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
                        let mut reads =
                            linear::decode_candidates(&r, first_black, mask, &mut regions.limited);
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
                                    retail_gray::decode(&row, &r, read.start, reverse)
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
                            let lo = start + left as f32;
                            let hi = start + right as f32;
                            let length = hi - lo;
                            if let Some(g) = groups.iter_mut().find(|g| {
                                g.format == read.format
                                    && g.decoded == read.decoded
                                    && g.text == read.text
                                    && (g.addon.is_none()
                                        || read.addon.is_none()
                                        || g.addon == read.addon)
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
                                    addon: read.addon,
                                    format: read.format.into(),
                                    text: read.text,
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
                    previous_bits = bits;
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
    linear_geometry::distinct(&mut results);
    let (regions, limited) = regions.finish(&results);
    Scan {
        barcodes: results,
        regions,
        unfinished: matrix_unfinished || limited,
        lines: line_id,
    }
}

#[no_mangle]
pub extern "C" fn multi_new() -> *mut Session {
    Box::into_raw(Box::new(Session {
        input: vec![],
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
    use super::threshold;

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

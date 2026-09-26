//! Source-gray sampling hypotheses around already localized matrix candidates.
use barcode_multiformat::numeric::{f32_usize, usize_f32, usize_f64};
use barcode_multiformat::{aztec, datamatrix, qr, qr_detect, Detection, Scan};
type Quad = [[f32; 2]; 4];

fn sample(
    pixels: &[u8],
    width: usize,
    height: usize,
    q: Quad,
    cols: usize,
    rows: usize,
    offset: f32,
    budget: &mut usize,
) -> Option<Vec<u8>> {
    let count = cols.checked_mul(rows)?;
    if count > *budget {
        return None;
    }
    *budget -= count;
    let t = qr_detect::homography(
        [
            [0., 0.],
            [usize_f32(cols), 0.],
            [usize_f32(cols), usize_f32(rows)],
            [0., usize_f32(rows)],
        ],
        q,
    )?;
    let mut values = Vec::with_capacity(cols * rows);
    for y in 0..rows {
        for x in 0..cols {
            let [px, py] =
                qr_detect::map(&t, usize_f32(x) + 0.5 + offset, usize_f32(y) + 0.5 + offset);
            // Pixels are areas centered at half-integer source coordinates.
            let sx = px - 0.5;
            let sy = py - 0.5;
            if !sx.is_finite()
                || !sy.is_finite()
                || sx < 0.
                || sy < 0.
                || sx > usize_f32(width - 1)
                || sy > usize_f32(height - 1)
            {
                return None;
            }
            let x0 = f32_usize(sx.floor());
            let y0 = f32_usize(sy.floor());
            let x1 = (x0 + 1).min(width - 1);
            let y1 = (y0 + 1).min(height - 1);
            let fx = sx - usize_f32(x0);
            let fy = sy - usize_f32(y0);
            let a = f32::from(pixels[y0 * width + x0]) * (1. - fx)
                + f32::from(pixels[y0 * width + x1]) * fx;
            let b = f32::from(pixels[y1 * width + x0]) * (1. - fx)
                + f32::from(pixels[y1 * width + x1]) * fx;
            values.push(
                u8::try_from(f32_usize((a * (1. - fy) + b * fy).round().clamp(0., 255.)))
                    .expect("clamped pixel"),
            );
        }
    }
    Some(values)
}
fn threshold(values: &[u8]) -> Vec<bool> {
    let mut hist = [0usize; 256];
    for &v in values {
        hist[usize::from(v)] += 1;
    }
    let total = values.len();
    let sum: usize = values.iter().map(|&v| usize::from(v)).sum();
    let (mut count, mut left, mut best, mut cut) = (0, 0, 0_f64, 128);
    for (i, &n) in hist.iter().enumerate() {
        count += n;
        left += i * n;
        if count == 0 || count == total {
            continue;
        }
        let d =
            usize_f64(left) / usize_f64(count) - usize_f64(sum - left) / usize_f64(total - count);
        let score = usize_f64(count) * usize_f64(total - count) * d * d;
        if score > best {
            best = score;
            cut = i;
        }
    }
    values.iter().map(|&v| usize::from(v) <= cut).collect()
}
fn dm_border(bits: &[bool], w: usize, h: usize) -> usize {
    let mut errors = 0;
    for x in 0..w {
        errors += usize::from(bits[x] != (x % 2 == 0));
        errors += usize::from(!bits[(h - 1) * w + x]);
    }
    for y in 0..h {
        errors += usize::from(!bits[y * w]);
        errors += usize::from(bits[y * w + w - 1] != (y % 2 == 1));
    }
    errors
}
fn aztec_size(bits: &[bool], n: usize) -> bool {
    [true, false].into_iter().any(|compact| {
        aztec::read_mode(bits, n, compact).is_some_and(|mode| {
            let base = mode.layers * 4 + if compact { 11 } else { 14 };
            n == if compact {
                base
            } else {
                base + 1 + 2 * ((base / 2 - 1) / 15)
            }
        })
    })
}
fn detection(read: qr::Payload, q: Quad, mask: u32, support: usize) -> Detection {
    Detection {
        bytes: Some(read.bytes),
        structured_append: read.structured_append,
        reader_initialization: read.reader_initialization,
        addon: None,
        format: if mask == 1024 { "DataMatrix" } else { "Aztec" }.into(),
        text: read.text,
        polygon: q,
        support,
        error: usize_f32(read.corrected),
        gs1: read.gs1,
    }
}
pub(crate) fn recover(
    pixels: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    scan: &Scan,
    budget: &mut usize,
) -> Scan {
    let mut reads: Vec<Detection> = Vec::new();
    let mut attempts = 0;
    let sizes: Vec<_> = if mask == 1024 {
        datamatrix::SIZES.iter().map(|s| (s.w, s.h)).collect()
    } else {
        let mut values: Vec<_> = (1..=32)
            .map(|layers| {
                let base = layers * 4 + 14;
                let n = base + 1 + 2 * ((base / 2 - 1) / 15);
                (n, n)
            })
            .collect();
        values.extend((1..=4).map(|layers| {
            let n = layers * 4 + 11;
            (n, n)
        }));
        values.sort_unstable();
        values.dedup();
        values
    };
    'region: for region in scan.regions.iter().take(8) {
        if reads
            .iter()
            .any(|r| barcode_multiformat::regions::overlap(&r.polygon, &region.polygon) > 0.75)
        {
            continue;
        }
        let mut hypotheses = Vec::new();
        'search: for mirror in [false, true] {
            for turn in 0..4 {
                let q: Quad = std::array::from_fn(|i| {
                    region.polygon[(turn + if mirror { 4 - i } else { i }) % 4]
                });
                let dx = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]);
                let dy = (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1]);
                for &(cols, rows) in &sizes {
                    if dx / usize_f32(cols) < 0.8 || dy / usize_f32(rows) < 0.8 {
                        continue;
                    }
                    let ratio = (dx / dy) / (usize_f32(cols) / usize_f32(rows));
                    if !(0.6..=1.6).contains(&ratio) {
                        continue;
                    }
                    for offset in [0., -0.15, 0.15] {
                        if cols * rows > *budget {
                            break 'search;
                        }
                        let Some(values) =
                            sample(pixels, width, height, q, cols, rows, offset, budget)
                        else {
                            continue;
                        };
                        let bits = threshold(&values);
                        let error = if mask == 1024 {
                            dm_border(&bits, cols, rows)
                        } else {
                            0
                        };
                        if (mask == 1024 && error * 5 > (cols + rows) * 2)
                            || (mask == 4096 && !aztec_size(&bits, cols))
                        {
                            continue;
                        }
                        hypotheses.push((error, cols, rows, bits, q));
                    }
                }
            }
        }
        hypotheses.sort_by_key(|h| h.0);
        for (_, cols, rows, bits, q) in hypotheses.into_iter().take(24) {
            attempts += 1;
            let read = if mask == 1024 {
                datamatrix::decode_matrix(&bits, cols, rows)
            } else {
                aztec::decode_matrix(&bits, cols)
            };
            if let Some(read) = read {
                reads.push(detection(read, q, mask, region.support));
                continue 'region;
            }
            if mask == 4096 {
                if let Some((read, refined)) = refine_aztec(pixels, width, height, q, cols, budget)
                {
                    reads.push(detection(read, refined, mask, region.support));
                    continue 'region;
                }
            }
        }
    }
    Scan {
        barcodes: reads,
        regions: vec![],
        unfinished: true,
        lines: attempts,
    }
}

fn expand(q: Quad, n: usize) -> Option<Quad> {
    let t = qr_detect::homography([[-5.5, -5.5], [5.5, -5.5], [5.5, 5.5], [-5.5, 5.5]], q)?;
    let half = usize_f32(n) * 0.5;
    Some(
        [[-half, -half], [half, -half], [half, half], [-half, half]]
            .map(|[x, y]| qr_detect::map(&t, x, y)),
    )
}
pub(crate) fn resolve_runes(
    pixels: &[u8],
    width: usize,
    height: usize,
    scan: &mut Scan,
    budget: &mut usize,
) {
    for candidate in scan.barcodes.iter_mut().take(8) {
        if candidate.format != "Aztec"
            || candidate.text.len() != 3
            || !candidate.text.bytes().all(|v| v.is_ascii_digit())
        {
            continue;
        }
        let base_q = candidate.polygon;
        let center = base_q.iter().fold([0., 0.], |mut p, q| {
            p[0] += q[0] * 0.25;
            p[1] += q[1] * 0.25;
            p
        });
        'found: for scale in [1., 0.95, 1.05, 0.9, 1.1, 0.8, 1.2] {
            for angle in [0_f32, -3., 3.] {
                let angle = angle.to_radians();
                let (sin, cos) = angle.sin_cos();
                let q = base_q.map(|p| {
                    let x = (p[0] - center[0]) * scale;
                    let y = (p[1] - center[1]) * scale;
                    [center[0] + cos * x - sin * y, center[1] + sin * x + cos * y]
                });
                for mirror in [false, true] {
                    for turn in 0..4 {
                        let oriented: Quad =
                            std::array::from_fn(|i| q[(turn + if mirror { 4 - i } else { i }) % 4]);
                        let Some(mode_q) = expand(oriented, 15) else {
                            continue;
                        };
                        for offset in [0., -0.15, 0.15] {
                            let Some(values) =
                                sample(pixels, width, height, mode_q, 15, 15, offset, budget)
                            else {
                                continue;
                            };
                            let bits = threshold(&values);
                            for compact in [true, false] {
                                let Some(mode) = aztec::read_mode(&bits, 15, compact) else {
                                    continue;
                                };
                                let base = mode.layers * 4 + if compact { 11 } else { 14 };
                                let n = if compact {
                                    base
                                } else {
                                    base + 1 + 2 * ((base / 2 - 1) / 15)
                                };
                                let Some(full_q) = expand(oriented, n) else {
                                    continue;
                                };
                                let Some(values) =
                                    sample(pixels, width, height, full_q, n, n, offset, budget)
                                else {
                                    continue;
                                };
                                let full = threshold(&values);
                                if let Some(read) = aztec::decode_matrix(&full, n) {
                                    *candidate = detection(read, full_q, 4096, candidate.support);
                                    break 'found;
                                }
                                {
                                    if let Some((read, refined)) =
                                        refine_aztec(pixels, width, height, full_q, n, budget)
                                    {
                                        *candidate =
                                            detection(read, refined, 4096, candidate.support);
                                        break 'found;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn reframe(q: Quad, from: usize, to: usize) -> Option<Quad> {
    let half = usize_f32(from) * 0.5;
    let t = qr_detect::homography(
        [[-half, -half], [half, -half], [half, half], [-half, half]],
        q,
    )?;
    let half = usize_f32(to) * 0.5;
    Some(
        [[-half, -half], [half, -half], [half, half], [-half, half]]
            .map(|[x, y]| qr_detect::map(&t, x, y)),
    )
}
fn ring_score(
    pixels: &[u8],
    width: usize,
    height: usize,
    q: Quad,
    n: usize,
    budget: &mut usize,
) -> f32 {
    let Some(values) = sample(pixels, width, height, q, n, n, 0., budget) else {
        return f32::NEG_INFINITY;
    };
    let mut score = 0_f32;
    let c = n / 2;
    for y in 0..n {
        for x in 0..n {
            let radius = x.abs_diff(c).max(y.abs_diff(c));
            let v = f32::from(values[y * n + x]);
            score += if radius % 2 == 0 { 255. - v } else { v };
        }
    }
    score / usize_f32(n * n)
}
fn refine_aztec(
    pixels: &[u8],
    width: usize,
    height: usize,
    q: Quad,
    n: usize,
    budget: &mut usize,
) -> Option<(qr::Payload, Quad)> {
    for compact in [true, false] {
        let inner = if compact { 9 } else { 13 };
        let mut ring = reframe(q, n, inner)?;
        let pitch = (ring[1][0] - ring[0][0]).hypot(ring[1][1] - ring[0][1]) / usize_f32(inner);
        let mut score = ring_score(pixels, width, height, ring, inner, budget);
        for factor in [0.3, 0.15, 0.075] {
            for _ in 0..2 {
                let mut changed = false;
                for corner in 0..4 {
                    for axis in 0..2 {
                        let mut best = ring;
                        for sign in [-1., 1.] {
                            let mut trial = ring;
                            trial[corner][axis] += pitch * factor * sign;
                            let value = ring_score(pixels, width, height, trial, inner, budget);
                            if value > score {
                                score = value;
                                best = trial;
                                changed = true;
                            }
                        }
                        ring = best;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        let full_q = reframe(ring, inner, n)?;
        for offset in [0., -0.15, 0.15] {
            let Some(values) = sample(pixels, width, height, full_q, n, n, offset, budget) else {
                continue;
            };
            if let Some(read) = aztec::decode_matrix(&threshold(&values), n) {
                return Some((read, full_q));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{reframe, sample, threshold};
    #[test]
    fn source_samples_share_a_hard_allowance() {
        let pixels = [127; 64];
        let quad = [[2., 1.], [5., 1.], [5., 4.], [2., 4.]];
        let mut budget = 17;
        assert!(sample(&pixels, 8, 8, quad, 3, 3, 0., &mut budget).is_some());
        assert_eq!(budget, 8);
        assert!(sample(&pixels, 8, 8, quad, 3, 3, 0., &mut budget).is_none());
        assert_eq!(budget, 8);
    }
    #[test]
    fn gray_sampling_uses_source_pixel_centers_and_translation() {
        let pixels: Vec<u8> = (0..64).map(|v| v * 3).collect();
        let quad = [[2., 1.], [5., 1.], [5., 4.], [2., 4.]];
        assert_eq!(
            sample(&pixels, 8, 8, quad, 3, 3, 0., &mut 100).unwrap(),
            vec![30, 33, 36, 54, 57, 60, 78, 81, 84]
        );
        assert_eq!(
            sample(&pixels, 8, 8, quad, 3, 3, 0.5, &mut 100).unwrap(),
            vec![44, 47, 50, 68, 71, 74, 92, 95, 98]
        );
        assert!(sample(&pixels, 8, 8, [[0., 0.]; 4], 3, 3, 0., &mut 100).is_none());
    }
    #[test]
    fn projective_reframing_preserves_the_original_quad() {
        let original = [[7., 11.], [91., 19.], [74., 95.], [13., 72.]];
        let restored = reframe(reframe(original, 31, 9).unwrap(), 9, 31).unwrap();
        for (a, b) in original
            .into_iter()
            .flatten()
            .zip(restored.into_iter().flatten())
        {
            assert!((a - b).abs() < 0.001);
        }
    }
    #[test]
    fn threshold_preserves_both_sides_of_low_contrast_modules() {
        assert_eq!(
            threshold(&[120, 120, 132, 132]),
            vec![true, true, false, false]
        );
        let flat = threshold(&[127; 25]);
        assert!(flat.iter().all(|v| *v == flat[0]));
    }
}

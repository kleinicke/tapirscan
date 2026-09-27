//! Bounded original-gray QR resampling around an existing qualified region.
use barcode_multiformat::numeric::{f32_usize, usize_f32};
use barcode_multiformat::{qr, qr_detect, Detection, Scan};
type Quad = [[f32; 2]; 4];
const BASE_OFFSETS: &[(f32, f32)] = &[(0., 0.), (-0.15, -0.15), (0.15, 0.15)];
const HIGH_OFFSETS: &[(f32, f32)] = &[
    (0., 0.),
    (-0.15, -0.15),
    (0.15, 0.15),
    (-0.15, 0.15),
    (0.15, -0.15),
    (-0.3, 0.),
    (0.3, 0.),
    (0., -0.3),
    (0., 0.3),
];

fn value(
    pixels: &[u8],
    width: usize,
    height: usize,
    projection: &[f32; 8],
    x: f32,
    y: f32,
) -> Option<f32> {
    let [px, py] = qr_detect::map(projection, x, y);
    let (sx, sy) = (px - 0.5, py - 0.5);
    if !sx.is_finite()
        || !sy.is_finite()
        || sx < 0.
        || sy < 0.
        || sx > usize_f32(width - 1)
        || sy > usize_f32(height - 1)
    {
        return None;
    }
    let (x0, y0) = (f32_usize(sx), f32_usize(sy));
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (fx, fy) = (sx - usize_f32(x0), sy - usize_f32(y0));
    let first =
        f32::from(pixels[y0 * width + x0]) * (1. - fx) + f32::from(pixels[y0 * width + x1]) * fx;
    let second =
        f32::from(pixels[y1 * width + x0]) * (1. - fx) + f32::from(pixels[y1 * width + x1]) * fx;
    Some(first * (1. - fy) + second * fy)
}
fn transform(quad: Quad, size: usize) -> Option<[f32; 8]> {
    let size = usize_f32(size);
    qr_detect::homography([[0., 0.], [size, 0.], [size, size], [0., size]], quad)
}
fn grid_quality(
    pixels: &[u8],
    width: usize,
    height: usize,
    projection: &[f32; 8],
    size: usize,
    budget: &mut usize,
) -> Option<(f32, f32, bool)> {
    let mut values = Vec::new();
    for i in 8..size - 8 {
        for (x, y) in [(6, i), (i, 6)] {
            values.push((x, y, i % 2 == 0));
        }
    }
    for (cx, cy) in [(3, 3), (size - 4, 3), (3, size - 4)] {
        for y in 0_usize..7 {
            for x in 0_usize..7 {
                values.push((
                    cx + x - 3,
                    cy + y - 3,
                    x.abs_diff(3).max(y.abs_diff(3)) != 2,
                ));
            }
        }
    }
    if values.len() > *budget {
        return None;
    }
    *budget -= values.len();
    let mut sums = [0.; 2];
    let mut counts = [0usize; 2];
    let mut observations = Vec::new();
    for (x, y, black) in values {
        let intensity = value(
            pixels,
            width,
            height,
            projection,
            usize_f32(x) + 0.5,
            usize_f32(y) + 0.5,
        )?;
        let color_index = usize::from(black);
        sums[color_index] += intensity;
        counts[color_index] += 1;
        observations.push((intensity, black));
    }
    let light = sums[0] / usize_f32(counts[0]);
    let dark = sums[1] / usize_f32(counts[1]);
    let cut = (light + dark) * 0.5;
    let inverse = dark > light;
    let errors = observations
        .iter()
        .filter(|&&(intensity, second)| ((intensity < cut) ^ inverse) != second)
        .count();
    let score = 1. - usize_f32(errors) / usize_f32(observations.len());
    (score >= 0.78 && (light - dark).abs() >= 5.).then_some((score, cut, inverse))
}
/// Resample a bounded set of unresolved QR regions from the original grayscale image.
#[must_use]
pub fn recover(
    pixels: &[u8],
    width: usize,
    height: usize,
    scan: &Scan,
    known: &[&Detection],
) -> Scan {
    let mut reads: Vec<Detection> = Vec::new();
    let higher = cfg!(any(feature = "mode-high", feature = "mode-very-high"));
    let mut budget = if higher { 1_000_000 } else { 250_000 };
    let offsets = if higher { HIGH_OFFSETS } else { BASE_OFFSETS };
    for region in scan.regions.iter().filter(|r| r.format == "QRCode").take(8) {
        if known
            .iter()
            .copied()
            .chain(&reads)
            .any(|r| covered(region.polygon, r.polygon))
        {
            continue;
        }
        let mut candidates = Vec::new();
        for turn in 0..4 {
            let quad = std::array::from_fn(|i| region.polygon[(i + turn) % 4]);
            for version in 1..=40 {
                let size = 17 + version * 4;
                let Some(projection) = transform(quad, size) else {
                    continue;
                };
                if let Some((score, cut, inverse)) =
                    grid_quality(pixels, width, height, &projection, size, &mut budget)
                {
                    candidates.push((score, size, projection, quad, cut, inverse));
                }
            }
        }
        candidates.sort_by(|first, second| second.0.total_cmp(&first.0));
        'candidate: for (_, size, projection, quad, cut, inverse) in
            candidates.into_iter().take(if higher { 16 } else { 8 })
        {
            for &(offset_x, offset_y) in offsets {
                if size * size > budget {
                    break 'candidate;
                }
                budget -= size * size;
                let mut bits = Vec::with_capacity(size * size);
                for y in 0..size {
                    for x in 0..size {
                        let Some(intensity) = value(
                            pixels,
                            width,
                            height,
                            &projection,
                            usize_f32(x) + 0.5 + offset_x,
                            usize_f32(y) + 0.5 + offset_y,
                        ) else {
                            continue 'candidate;
                        };
                        bits.push((intensity < cut) ^ inverse);
                    }
                }
                for mirror in [false, true] {
                    let matrix = if mirror {
                        (0..size * size)
                            .map(|i| bits[(i % size) * size + i / size])
                            .collect()
                    } else {
                        bits.clone()
                    };
                    if !qr::plausible_image_header(size, |x, y| Some(matrix[y * size + x])) {
                        continue;
                    }
                    if let Some(payload) = qr::decode_matrix(&matrix, size) {
                        reads.push(Detection {
                            bytes: Some(payload.bytes),
                            structured_append: payload.structured_append,
                            reader_initialization: payload.reader_initialization,
                            addon: None,
                            format: "QRCode".into(),
                            text: payload.text,
                            polygon: quad,
                            support: region.support,
                            error: usize_f32(payload.corrected),
                            gs1: payload.gs1,
                        });
                        break 'candidate;
                    }
                }
            }
        }
    }
    Scan {
        barcodes: reads,
        regions: Vec::new(),
        unfinished: true,
        lines: 0,
    }
}

fn covered(region: Quad, decoded: Quad) -> bool {
    let quad = decoded.map(|p| p.map(f64::from));
    region
        .into_iter()
        .all(|p| contains_point(p.map(f64::from), &quad))
}

fn contains_point(point: [f64; 2], quad: &[[f64; 2]; 4]) -> bool {
    if !point.iter().all(|intensity| intensity.is_finite())
        || !quad.iter().flatten().all(|intensity| intensity.is_finite())
    {
        return false;
    }
    let (mut positive, mut negative, mut area) = (false, false, 0.0_f64);
    for i in 0..4 {
        let (first, second) = (quad[i], quad[(i + 1) % 4]);
        let cross = (second[0] - first[0]) * (point[1] - first[1])
            - (second[1] - first[1]) * (point[0] - first[0]);
        positive |= cross > 1e-6;
        negative |= cross < -1e-6;
        area += first[0] * second[1] - second[0] * first[1];
    }
    area.abs() > 1e-6 && !(positive && negative)
}

#[cfg(test)]
mod tests {
    #[test]
    fn bilinear_source_samples_preserve_pixel_centers_and_reject_outside_points() {
        let pixels = [0, 10, 20, 30];
        let identity = [1., 0., 0., 0., 1., 0., 0., 0.];
        for (x, y, expected) in [(0.5, 0.5, 0.), (1., 1., 15.), (1.5, 1.5, 30.)] {
            let actual = super::value(&pixels, 2, 2, &identity, x, y).unwrap();
            assert!((actual - expected).abs() < f32::EPSILON);
        }
        for (x, y) in [
            (0.49, 0.5),
            (1.51, 1.5),
            (f32::NAN, 1.),
            (1., f32::INFINITY),
        ] {
            assert!(super::value(&pixels, 2, 2, &identity, x, y).is_none());
        }
    }
    #[test]
    fn insufficient_sample_budget_does_not_start_a_grid() {
        let pixels = vec![128; 21 * 21];
        let identity = [1., 0., 0., 0., 1., 0., 0., 0.];
        let mut budget = 10;
        assert!(super::grid_quality(&pixels, 21, 21, &identity, 21, &mut budget).is_none());
        assert_eq!(budget, 10);
    }
    #[test]
    fn only_fully_contained_finite_regions_are_covered() {
        let owner = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        assert!(super::covered(
            [[5., 1.], [9., 5.], [5., 9.], [1., 5.]],
            owner
        ));
        assert!(!super::covered(
            [[9., 1.], [11., 1.], [11., 3.], [9., 3.]],
            owner
        ));
        assert!(!super::covered(owner, [[0., 0.]; 4]));
        let mut invalid = owner;
        invalid[0][0] = f32::NAN;
        assert!(!super::covered(invalid, owner));
    }
}

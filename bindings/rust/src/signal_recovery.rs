//! Bounded source contrast recovery for selected matrix readers.
pub(crate) fn sharpen(gray: &[u8], width: usize, height: usize, strength: i32) -> Vec<u8> {
    let mut horizontal = vec![0_u16; width * height];
    for y in 0..height {
        let row = &gray[y * width..(y + 1) * width];
        for x in 0..width {
            horizontal[y * width + x] = u16::from(row[x.saturating_sub(2)])
                + u16::from(row[x.saturating_sub(1)]) * 4
                + u16::from(row[x]) * 6
                + u16::from(row[(x + 1).min(width - 1)]) * 4
                + u16::from(row[(x + 2).min(width - 1)]);
        }
    }
    let mut output = vec![0_u8; width * height];
    for y in 0..height {
        let rows = [
            y.saturating_sub(2),
            y.saturating_sub(1),
            y,
            (y + 1).min(height - 1),
            (y + 2).min(height - 1),
        ];
        for x in 0..width {
            let smooth = u32::from(horizontal[rows[0] * width + x])
                + u32::from(horizontal[rows[1] * width + x]) * 4
                + u32::from(horizontal[rows[2] * width + x]) * 6
                + u32::from(horizontal[rows[3] * width + x]) * 4
                + u32::from(horizontal[rows[4] * width + x]);
            let center = i32::from(gray[y * width + x]);
            let blur = i32::try_from((smooth + 128) / 256).expect("pixel fits");
            output[y * width + x] =
                u8::try_from((center + strength * (center - blur)).clamp(0, 255)).expect("clamped");
        }
    }
    output
}

pub(crate) fn region_retries(
    pixels: &[u8],
    width: usize,
    height: usize,
    mask: u32,
    effort: usize,
    scan: &barcode_multiformat::Scan,
    budget: &mut usize,
) -> Vec<barcode_multiformat::Scan> {
    let mut output = Vec::new();
    for region in scan.regions.iter().take(8) {
        let mut bounds = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for point in region.polygon {
            bounds[0] = bounds[0].min(point[0]);
            bounds[1] = bounds[1].min(point[1]);
            bounds[2] = bounds[2].max(point[0]);
            bounds[3] = bounds[3].max(point[1]);
        }
        if bounds.iter().any(|v| !v.is_finite()) {
            continue;
        }
        let pad = (bounds[2] - bounds[0]).max(bounds[3] - bounds[1]) * 0.2 + 8.;
        let x0 =
            barcode_multiformat::numeric::f32_usize((bounds[0] - pad).max(0.).floor()).min(width);
        let y0 =
            barcode_multiformat::numeric::f32_usize((bounds[1] - pad).max(0.).floor()).min(height);
        let x1 =
            barcode_multiformat::numeric::f32_usize((bounds[2] + pad).max(0.).ceil()).min(width);
        let y1 =
            barcode_multiformat::numeric::f32_usize((bounds[3] + pad).max(0.).ceil()).min(height);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        if w.min(h) < 3 || w * h > 262_144 || w * h > *budget {
            continue;
        }
        *budget -= w * h;
        let mut crop = Vec::with_capacity(w * h);
        for y in y0..y1 {
            crop.extend_from_slice(&pixels[y * width + x0..y * width + x1]);
        }
        for method in 0..3 {
            let (image, scale) = match method {
                0 => (crop.clone(), 1),
                1 => (sharpen(&crop, w, h, 2), 1),
                _ => (double(&crop, w, h), 2),
            };
            let mut retry = crate::formats::scan_reader(&image, w * scale, h * scale, mask, effort);
            retry.barcodes.retain(|r| {
                !(mask == 4096 && r.text.len() == 3 && r.text.bytes().all(|v| v.is_ascii_digit()))
            });
            for polygon in retry
                .barcodes
                .iter_mut()
                .map(|r| &mut r.polygon)
                .chain(retry.regions.iter_mut().map(|r| &mut r.polygon))
            {
                for point in polygon {
                    point[0] = point[0] / barcode_multiformat::numeric::usize_f32(scale)
                        + barcode_multiformat::numeric::usize_f32(x0);
                    point[1] = point[1] / barcode_multiformat::numeric::usize_f32(scale)
                        + barcode_multiformat::numeric::usize_f32(y0);
                }
            }
            let done = retry
                .barcodes
                .iter()
                .any(|r| covered(region.polygon, r.polygon));
            retry.unfinished = true;
            output.push(retry);
            if done {
                break;
            }
        }
    }
    output
}
fn double(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut output = vec![0; width * height * 4];
    for y in 0..height * 2 {
        let sy = y / 2;
        let ny = if y % 2 == 0 {
            sy.saturating_sub(1)
        } else {
            (sy + 1).min(height - 1)
        };
        for x in 0..width * 2 {
            let sx = x / 2;
            let nx = if x % 2 == 0 {
                sx.saturating_sub(1)
            } else {
                (sx + 1).min(width - 1)
            };
            let value = u16::from(pixels[sy * width + sx]) * 9
                + u16::from(pixels[sy * width + nx]) * 3
                + u16::from(pixels[ny * width + sx]) * 3
                + u16::from(pixels[ny * width + nx]);
            output[y * width * 2 + x] = u8::try_from((value + 8) / 16).expect("pixel fits");
        }
    }
    output
}

/// A small decoded polygon cannot hide a larger pending matrix candidate.
pub(crate) fn covered(region: [[f32; 2]; 4], decoded: [[f32; 2]; 4]) -> bool {
    let quad = decoded.map(|p| p.map(f64::from));
    region
        .into_iter()
        .all(|p| crate::formats::contains_point(p.map(f64::from), &quad))
}

#[cfg(test)]
mod tests {
    use super::{double, sharpen};
    #[test]
    fn constant_sources_stay_constant_at_edges() {
        for value in [0, 127, 255] {
            let pixels = vec![value; 35];
            assert_eq!(sharpen(&pixels, 7, 5, 2), pixels);
            assert_eq!(double(&pixels, 7, 5), vec![value; 140]);
        }
    }
    #[test]
    fn doubled_source_keeps_pixel_center_alignment() {
        assert_eq!(
            double(&[0, 160], 2, 1),
            vec![0, 40, 120, 160, 0, 40, 120, 160]
        );
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::covered;
    #[test]
    fn pending_coverage_requires_every_corner_not_small_polygon_overlap() {
        let region = [[0., 0.], [20., 0.], [20., 20.], [0., 20.]];
        assert!(covered(region, region));
        assert!(!covered(
            region,
            [[5., 5.], [15., 5.], [15., 15.], [5., 15.]]
        ));
        assert!(!covered(
            region,
            [[10., 0.], [30., 0.], [30., 20.], [10., 20.]]
        ));
        assert!(covered(
            region,
            [region[0], region[3], region[2], region[1]]
        ));
    }
}

/// Histogram normalization for bounded, otherwise unresolved higher-effort QR frames.
#[cfg(any(feature = "high", feature = "very-high"))]
pub(crate) fn equalize(pixels: &[u8]) -> Vec<u8> {
    let mut histogram = [0_usize; 256];
    for &value in pixels {
        histogram[usize::from(value)] += 1;
    }
    let last = histogram.iter().rposition(|&count| count != 0).unwrap_or(0);
    let step = (pixels.len() - histogram[last]) / 255;
    if step == 0 {
        return pixels.to_vec();
    }
    let mut sum = step / 2;
    let table: [u8; 256] = std::array::from_fn(|value| {
        let normalized = u8::try_from((sum / step).min(255)).expect("clamped intensity");
        sum += histogram[value];
        normalized
    });
    pixels
        .iter()
        .map(|&value| table[usize::from(value)])
        .collect()
}

#[cfg(all(test, any(feature = "high", feature = "very-high")))]
mod normalization_tests {
    #[test]
    fn histogram_normalization_matches_independent_reference_and_preserves_flat_frames() {
        // Expected intensities independently generated with Pillow ImageOps.equalize.
        let bands = [(0, 512), (64, 256), (128, 384), (192, 128), (255, 256)];
        let input: Vec<u8> = bands
            .into_iter()
            .flat_map(|(value, count)| std::iter::repeat_n(value, count))
            .collect();
        let output = super::equalize(&input);
        let mut start = 0;
        for ((_, count), expected) in bands.into_iter().zip([0, 102, 154, 230, 255]) {
            assert!(output[start..start + count].iter().all(|&v| v == expected));
            start += count;
        }
        for value in [0, 127, 255] {
            let flat = vec![value; 1024];
            assert_eq!(super::equalize(&flat), flat);
        }
        assert!(super::equalize(&[]).is_empty());
    }
}

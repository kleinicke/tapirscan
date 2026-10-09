//! Area-average reduction of images larger than the engine's working size.
// Pixel counts and coordinates stay far below 2^52, so the f64 casts are exact,
// and every float converted back is a rounded value within the target range.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]
use crate::Image;

/// Largest image the engine scans directly; larger images are reduced to fit.
pub(crate) const WORKING_PIXELS: usize = 32 * 1024 * 1024;

/// A reduced grayscale copy with packed rows. The engine scans luma, so color
/// would only multiply the reduction work.
pub(crate) struct Reduced {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}
impl Reduced {
    pub(crate) fn image(&self) -> Image<'_> {
        Image::gray(&self.pixels, self.width, self.height)
    }
    /// Factors from reduced to original coordinates, `[x, y]`.
    pub(crate) fn scale(&self, original: Image<'_>) -> [f64; 2] {
        [
            original.width as f64 / self.width as f64,
            original.height as f64 / self.height as f64,
        ]
    }
}

/// Reduce a validated image above [`WORKING_PIXELS`]; `None` when it already fits.
pub(crate) fn reduce(image: Image<'_>) -> Option<Reduced> {
    let pixels = image.width * image.height;
    if pixels <= WORKING_PIXELS {
        return None;
    }
    let factor = (WORKING_PIXELS as f64 / pixels as f64).sqrt();
    let width = ((image.width as f64 * factor) as usize).max(3);
    let height = ((image.height as f64 * factor) as usize).max(3);
    let columns = spans(image.width, width);
    let mut output = vec![0; width * height];
    // Rows are averaged into luma first, in a contiguous loop, then each output
    // row is reduced horizontally.
    let mut rows = vec![0_f32; image.width];
    for (target, (first, weights)) in output
        .chunks_exact_mut(width)
        .zip(spans(image.height, height))
    {
        rows.fill(0.);
        for (offset, &weight) in weights.iter().enumerate() {
            if weight == 0. {
                continue;
            }
            let start = (first + offset) * image.stride;
            let row = &image.data[start..start + image.width * image.channels];
            match image.channels {
                1 => add_luma::<1>(row, weight, &mut rows),
                3 => add_luma::<3>(row, weight, &mut rows),
                _ => add_luma::<4>(row, weight, &mut rows),
            }
        }
        for (pixel, (first, weights)) in target.iter_mut().zip(&columns) {
            let sum: f32 = rows[*first..first + TAPS]
                .iter()
                .zip(weights)
                .map(|(value, weight)| value * weight)
                .sum();
            *pixel = (sum + 0.5).clamp(0., 255.) as u8;
        }
    }
    Some(Reduced {
        pixels: output,
        width,
        height,
    })
}

/// Add `weight` times each pixel's luma, with the engine's 77/150/29 weights.
fn add_luma<const C: usize>(row: &[u8], weight: f32, rows: &mut [f32]) {
    if C == 1 {
        for (sum, &value) in rows.iter_mut().zip(row) {
            *sum += weight * f32::from(value);
        }
        return;
    }
    let [red, green, blue] = [77., 150., 29.].map(|luma: f32| weight * luma / 256.);
    for (sum, pixel) in rows.iter_mut().zip(row.chunks_exact(C)) {
        *sum +=
            red * f32::from(pixel[0]) + green * f32::from(pixel[1]) + blue * f32::from(pixel[2]);
    }
}

/// Source samples one target sample can cover. Reduction is at most 8× in
/// area (256 MP to 32 MP), so at most √8 ≈ 2.83 samples per axis, spread over
/// at most four.
const TAPS: usize = 4;

/// For each target sample, a first source index and the covered fraction of
/// the next [`TAPS`] source samples; the fractions of one target sum to 1.
/// The window is shifted left at the end of the axis to stay in bounds.
fn spans(source: usize, target: usize) -> Vec<(usize, [f32; TAPS])> {
    debug_assert!(source >= TAPS && source <= target * 3);
    let step = source as f64 / target as f64;
    (0..target)
        .map(|i| {
            let start = i as f64 * step;
            let end = start + step;
            let first = (start.floor() as usize).min(source - TAPS);
            let mut weights = [0_f32; TAPS];
            for (j, weight) in weights.iter_mut().enumerate() {
                let sample = (first + j) as f64;
                let covered = end.min(sample + 1.) - start.max(sample);
                *weight = (covered.max(0.) / step) as f32;
            }
            (first, weights)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{WORKING_PIXELS, reduce, spans};
    use crate::Image;

    #[test]
    fn spans_cover_each_source_sample_once() {
        for (source, target) in [(10, 4), (7, 7), (8193, 4096), (12, 5), (65_535, 23_170)] {
            let mut coverage = vec![0_f64; source];
            for (first, weights) in spans(source, target) {
                let total: f32 = weights.iter().sum();
                assert!((total - 1.).abs() < 1e-5);
                for (j, weight) in weights.iter().enumerate() {
                    coverage[first + j] += f64::from(*weight) * source as f64 / target as f64;
                }
            }
            assert!(coverage.iter().all(|value| (value - 1.).abs() < 1e-4));
        }
    }

    #[test]
    fn images_within_the_working_size_are_not_copied() {
        let pixels = vec![0; 4096 * 8192];
        assert!(reduce(Image::gray(&pixels, 4096, 8192)).is_none());
    }

    #[test]
    fn large_images_keep_aspect_and_flat_luma() {
        let (width, height) = (8192, 4097);
        let mut pixels = vec![0; width * height * 3];
        for pixel in pixels.chunks_exact_mut(3) {
            pixel.copy_from_slice(&[10, 128, 250]);
        }
        let image = Image::rgb(&pixels, width, height);
        let reduced = reduce(image).unwrap();
        assert!(reduced.width * reduced.height <= WORKING_PIXELS);
        let [x, y] = reduced.scale(image);
        assert!((x - y).abs() < 1e-3, "aspect changed: {x} vs {y}");
        // (77 * 10 + 150 * 128 + 29 * 250) / 256 = 106.3
        assert!(reduced.pixels.iter().all(|&value| value == 106));
    }
}

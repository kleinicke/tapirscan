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
    pub(crate) fn scale(&self, [width, height]: [usize; 2]) -> [f64; 2] {
        [
            width as f64 / self.width as f64,
            height as f64 / self.height as f64,
        ]
    }
}

/// Reduce a validated image above [`WORKING_PIXELS`]; `None` when it already fits.
pub(crate) fn reduce(image: Image<'_>) -> Option<Reduced> {
    let mut reduction = Reduction::new(image.width, image.height, image.channels)?;
    let row = image.width * image.channels;
    for y in 0..image.height {
        let start = y * image.stride;
        reduction.push_row(&image.data[start..start + row]);
    }
    Some(reduction.finish())
}

/// Row-at-a-time reduction: source rows arrive top to bottom, and only the few
/// output rows whose windows cover the current row stay open. Each output row
/// sums its source rows in the same order as a whole-image pass, so the result
/// does not depend on how the rows are delivered.
pub(crate) struct Reduction {
    source_width: usize,
    source_height: usize,
    channels: usize,
    width: usize,
    height: usize,
    columns: Vec<(usize, [f32; TAPS])>,
    rows: Vec<(usize, [f32; TAPS])>,
    output: Vec<u8>,
    /// Output rows still receiving source rows, with their luma sums.
    open: std::collections::VecDeque<(usize, Vec<f32>)>,
    /// Finished sums kept for reuse.
    spare: Vec<Vec<f32>>,
    next_output: usize,
    received: usize,
}

impl Reduction {
    /// Start reducing validated dimensions; `None` when the image already fits.
    pub(crate) fn new(source_width: usize, source_height: usize, channels: usize) -> Option<Self> {
        let pixels = source_width * source_height;
        if pixels <= WORKING_PIXELS {
            return None;
        }
        let factor = (WORKING_PIXELS as f64 / pixels as f64).sqrt();
        let width = ((source_width as f64 * factor) as usize).max(3);
        let height = ((source_height as f64 * factor) as usize).max(3);
        Some(Self {
            source_width,
            source_height,
            channels,
            width,
            height,
            columns: spans(source_width, width),
            rows: spans(source_height, height),
            output: vec![0; width * height],
            open: std::collections::VecDeque::new(),
            spare: Vec::new(),
            next_output: 0,
            received: 0,
        })
    }

    /// Source rows still expected.
    pub(crate) fn remaining_rows(&self) -> usize {
        self.source_height - self.received
    }

    /// Add the next packed source row of `source_width * channels` bytes.
    pub(crate) fn push_row(&mut self, row: &[u8]) {
        debug_assert!(self.received < self.source_height);
        debug_assert_eq!(row.len(), self.source_width * self.channels);
        let index = self.received;
        while self.next_output < self.height && self.rows[self.next_output].0 <= index {
            let mut sums = self.spare.pop().unwrap_or_default();
            sums.clear();
            sums.resize(self.source_width, 0.);
            self.open.push_back((self.next_output, sums));
            self.next_output += 1;
        }
        for (output, sums) in &mut self.open {
            let (first, weights) = self.rows[*output];
            let weight = weights[index - first];
            if weight == 0. {
                continue;
            }
            match self.channels {
                1 => add_luma::<1>(row, weight, sums),
                3 => add_luma::<3>(row, weight, sums),
                _ => add_luma::<4>(row, weight, sums),
            }
        }
        self.received += 1;
        // A window covers TAPS source rows; it is complete after its last one.
        while let Some((output, _)) = self.open.front() {
            if self.rows[*output].0 + TAPS > self.received {
                break;
            }
            let (output, sums) = self.open.pop_front().expect("checked above");
            let target = &mut self.output[output * self.width..(output + 1) * self.width];
            for (pixel, (first, weights)) in target.iter_mut().zip(&self.columns) {
                let sum: f32 = sums[*first..first + TAPS]
                    .iter()
                    .zip(weights)
                    .map(|(value, weight)| value * weight)
                    .sum();
                *pixel = (sum + 0.5).clamp(0., 255.) as u8;
            }
            self.spare.push(sums);
        }
    }

    /// The reduced image once every source row has arrived.
    pub(crate) fn finish(self) -> Reduced {
        debug_assert_eq!(self.received, self.source_height);
        debug_assert!(self.open.is_empty());
        Reduced {
            pixels: self.output,
            width: self.width,
            height: self.height,
        }
    }

    /// Source dimensions, `[width, height]`.
    pub(crate) fn source_size(&self) -> [usize; 2] {
        [self.source_width, self.source_height]
    }
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
    for (sum, pixel) in rows.iter_mut().zip(row.as_chunks::<C>().0) {
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
    use super::{Reduction, TAPS, WORKING_PIXELS, add_luma, reduce, spans};
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

    /// Reduces the whole image in one loop, as a reference for the streamed rows.
    fn whole_image_reference(image: Image<'_>) -> Vec<u8> {
        let mut reduction = Reduction::new(image.width, image.height, image.channels).unwrap();
        let (width, height) = (reduction.width, reduction.height);
        let columns = std::mem::take(&mut reduction.columns);
        let mut output = vec![0; width * height];
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
        output
    }

    #[test]
    fn streamed_rows_match_the_whole_image_reduction_bit_for_bit() {
        let mut state = 0x2545_f491_u32;
        let mut noise = |count: usize| -> Vec<u8> {
            (0..count)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state.to_le_bytes()[1]
                })
                .collect()
        };
        for (width, height, channels) in [(8193, 4100, 1), (6001, 5601, 3), (7000, 5000, 4)] {
            let pixels = noise(width * height * channels);
            let image = match channels {
                1 => Image::gray(&pixels, width, height),
                3 => Image::rgb(&pixels, width, height),
                _ => Image::rgba(&pixels, width, height),
            };
            assert_eq!(reduce(image).unwrap().pixels, whole_image_reference(image));
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
        for pixel in pixels.as_chunks_mut::<3>().0 {
            *pixel = [10, 128, 250];
        }
        let image = Image::rgb(&pixels, width, height);
        let reduced = reduce(image).unwrap();
        assert!(reduced.width * reduced.height <= WORKING_PIXELS);
        let [x, y] = reduced.scale([image.width, image.height]);
        assert!((x - y).abs() < 1e-3, "aspect changed: {x} vs {y}");
        // (77 * 10 + 150 * 128 + 29 * 250) / 256 = 106.3
        assert!(reduced.pixels.iter().all(|&value| value == 106));
    }
}

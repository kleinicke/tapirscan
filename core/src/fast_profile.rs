//! Reusable bounded source-resolution profiles for the experimental fast reader.
use crate::{
    numeric::{f64_f32, usize_f64},
    sampling::ImageView,
};

// Image-derived profiles are finite nonnegative values. The experimental
// comparison form avoids generic NaN handling while preserving these inputs.
fn minimum(a: f32, b: f32) -> f32 {
    if option_env!("TAPIRSCAN_TURBO_FINITE_EXTREMA").is_some() {
        if a < b {
            a
        } else {
            b
        }
    } else {
        a.min(b)
    }
}
fn maximum(a: f32, b: f32) -> f32 {
    if option_env!("TAPIRSCAN_TURBO_FINITE_EXTREMA").is_some() {
        if a > b {
            a
        } else {
            b
        }
    } else {
        a.max(b)
    }
}

#[derive(Default)]
pub struct Sampler {
    values: Vec<f32>,
    positions: Vec<f64>,
    envelope: Vec<[f32; 4]>,
    widths: Vec<f32>,
    extrema: Vec<(usize, f32)>,
    pub edges: Vec<f32>,
    pub runs: Vec<f32>,
    pub first_black: bool,
    pub samples: usize,
}
impl Sampler {
    /// Sample a source line. Out-of-image samples are white quiet-zone padding.
    pub fn sample(&mut self, image: ImageView<'_>, start: [f64; 2], end: [f64; 2]) {
        self.sample_scaled(image, start, end, 1.5, 4096);
    }
    /// Retry unresolved decoder evidence at twice the ordinary sample density.
    pub fn sample_dense(&mut self, image: ImageView<'_>, start: [f64; 2], end: [f64; 2]) {
        self.sample_scaled(image, start, end, 3., 4096);
    }
    /// Bounded sampling policy for private fast-reader research artifacts.
    pub fn sample_limited(
        &mut self,
        image: ImageView<'_>,
        start: [f64; 2],
        end: [f64; 2],
        density: f64,
        limit: usize,
    ) {
        self.sample_scaled(image, start, end, density, limit.clamp(32, 4096));
    }
    /// Sample an additional chromatic hypothesis only when source color varies.
    /// Channel 1 is blue; channel 2 is the maximum of the source RGB channels.
    pub fn sample_color_limited(
        &mut self,
        image: ImageView<'_>,
        start: [f64; 2],
        end: [f64; 2],
        density: f64,
        limit: usize,
        channel: u8,
    ) -> bool {
        if image.channels == 1 {
            return false;
        }
        let length = (end[0] - start[0]).hypot(end[1] - start[1]);
        self.samples = crate::numeric::f64_isize((length * density).ceil())
            .clamp(32, isize::try_from(limit.clamp(32, 4096)).unwrap_or(4096))
            .cast_unsigned();
        self.values.clear();
        if self.positions.len() != self.samples {
            self.positions.clear();
            self.positions
                .extend((0..self.samples).map(|i| usize_f64(i) / usize_f64(self.samples - 1)));
        }
        let pixel = |x: usize, y: usize| {
            let at = y * image.stride + x * image.channels;
            let r = image.data[at];
            let g = image.data[at + 1];
            let b = image.data[at + 2];
            [
                f64::from(if channel == 1 { b } else { r.max(g).max(b) }),
                0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b),
            ]
        };
        let (mut delta_min, mut delta_max) = (f64::INFINITY, f64::NEG_INFINITY);
        for &t in &self.positions {
            let x = start[0] + (end[0] - start[0]) * t;
            let y = start[1] + (end[1] - start[1]) * t;
            if x < 0. || y < 0. || x > usize_f64(image.width - 1) || y > usize_f64(image.height - 1)
            {
                self.values.push(255.);
                continue;
            }
            let ix = crate::numeric::f64_usize(x.floor());
            let iy = crate::numeric::f64_usize(y.floor());
            let fx = x - x.floor();
            let fy = y - y.floor();
            let upper_left = pixel(ix, iy);
            let upper_right = pixel((ix + 1).min(image.width - 1), iy);
            let lower_left = pixel(ix, (iy + 1).min(image.height - 1));
            let lower_right = pixel(
                (ix + 1).min(image.width - 1),
                (iy + 1).min(image.height - 1),
            );
            let interpolated: [f64; 2] = std::array::from_fn(|k| {
                (upper_left[k] * (1. - fx) + upper_right[k] * fx) * (1. - fy)
                    + (lower_left[k] * (1. - fx) + lower_right[k] * fx) * fy
            });
            let delta = interpolated[0] - interpolated[1];
            delta_min = delta_min.min(delta);
            delta_max = delta_max.max(delta);
            self.values.push(f64_f32(interpolated[0]));
        }
        delta_max - delta_min >= 8.
    }
    fn sample_scaled(
        &mut self,
        image: ImageView<'_>,
        start: [f64; 2],
        end: [f64; 2],
        density: f64,
        limit: usize,
    ) {
        let length = (end[0] - start[0]).hypot(end[1] - start[1]);
        self.samples = crate::numeric::f64_isize((length * density).ceil())
            .clamp(32, isize::try_from(limit).unwrap_or(4096))
            .cast_unsigned();
        self.values.clear();
        let mut cursor = crate::sampling::BilinearCursor::new(image);
        if self.positions.len() != self.samples {
            self.positions.clear();
            self.positions
                .extend((0..self.samples).map(|i| usize_f64(i) / usize_f64(self.samples - 1)));
        }
        for &t in &self.positions {
            let x = start[0] + (end[0] - start[0]) * t;
            let y = start[1] + (end[1] - start[1]) * t;
            self.values.push(
                if x < 0.
                    || y < 0.
                    || x > usize_f64(image.width - 1)
                    || y > usize_f64(image.height - 1)
                {
                    255.
                } else if option_env!("TAPIRSCAN_TURBO_DIRECT_SAMPLE").is_some() {
                    image.bilinear(x, y)
                } else {
                    cursor.sample(x, y)
                },
            );
        }
    }
    /// Restore a bounded profile contrast hypothesis without changing source positions.
    pub fn restore_contrast(&mut self, strength: f32) {
        let original = self.values.clone();
        let n = original.len();
        for i in 0..n {
            let blurred = (original[i.saturating_sub(2)]
                + original[i.saturating_sub(1)] * 4.
                + original[i] * 6.
                + original[(i + 1).min(n - 1)] * 4.
                + original[(i + 2).min(n - 1)])
                / 16.;
            self.values[i] = (original[i] + strength * (original[i] - blurred)).clamp(0., 255.);
        }
    }
    /// Wider bounded contrast hypothesis for independently checked EAN recovery.
    pub fn restore_contrast_wide(&mut self, strength: f32) {
        let original = self.values.clone();
        let n = original.len();
        if n == 0 {
            return;
        }
        let weights = [1., 4., 11., 21., 26., 21., 11., 4., 1.];
        for i in 0..n {
            let mut blurred = 0.;
            for (k, weight) in weights.into_iter().enumerate() {
                blurred += original[(i + k).saturating_sub(4).min(n - 1)] * weight;
            }
            self.values[i] =
                (original[i] + strength * (original[i] - blurred / 100.)).clamp(0., 255.);
        }
    }
    /// No threshold method can create transitions below eight gray levels.
    /// Stop as soon as contrast is proven; most useful profiles exit early.
    #[must_use]
    pub fn has_contrast(&self) -> bool {
        let (mut lo, mut hi) = (255_f32, 0_f32);
        for &value in &self.values {
            lo = minimum(lo, value);
            hi = maximum(hi, value);
            if hi - lo >= 8. {
                return true;
            }
        }
        false
    }

    /// Build subpixel run boundaries from local extrema or a whole-line threshold.
    pub fn threshold(&mut self, local: bool) {
        self.edges.clear();
        self.runs.clear();
        self.extrema.clear();
        self.edges.push(0.);
        let values = &self.values;
        if values.len() < 2 {
            self.first_black = false;
            return;
        }
        if local {
            let (mut low, mut high) = (values[0], values[0]);
            let (mut lo_at, mut hi_at, mut direction) = (0, 0, 0);
            for (i, &v) in values.iter().enumerate() {
                if direction >= 0 {
                    if v >= high {
                        high = v;
                        hi_at = i;
                    }
                    if high - v >= 10. {
                        self.extrema.push((hi_at, high));
                        direction = -1;
                        low = v;
                        lo_at = i;
                    }
                }
                if direction <= 0 {
                    if v <= low {
                        low = v;
                        lo_at = i;
                    }
                    if v - low >= 10. {
                        self.extrema.push((lo_at, low));
                        direction = 1;
                        high = v;
                        hi_at = i;
                    }
                }
            }
            self.extrema.push(if direction == 1 {
                (hi_at, high)
            } else {
                (lo_at, low)
            });
            self.first_black = self.extrema.len() > 1 && self.extrema[0].1 < self.extrema[1].1;
            for pair in self.extrema.windows(2) {
                let [(left, a), (right, b)] = [pair[0], pair[1]];
                let cut = (a + b) * 0.5;
                for i in left + 1..=right {
                    if (values[i - 1] >= cut) != (values[i] >= cut) {
                        self.edges.push(
                            f64_f32(usize_f64(i - 1))
                                + (cut - values[i - 1]) / (values[i] - values[i - 1]),
                        );
                        break;
                    }
                }
            }
        } else {
            let mut histogram = [0usize; 256];
            for &value in values {
                histogram[crate::numeric::f32_usize(value).min(255)] += 1;
            }
            let quantile = |fraction: usize| {
                let target = values.len() * fraction / 100;
                let mut count = 0;
                for (i, &n) in histogram.iter().enumerate() {
                    count += n;
                    if count > target {
                        return f64_f32(usize_f64(i));
                    }
                }
                255.
            };
            // Exclude a bright exterior around a lower-contrast printed label.
            let lo = quantile(10);
            let hi = quantile(65);
            let cut = (lo + hi) * 0.5;
            self.first_black = values[0] < cut;
            if hi - lo >= 16. {
                for i in 1..values.len() {
                    if (values[i - 1] >= cut) != (values[i] >= cut) {
                        self.edges.push(
                            f64_f32(usize_f64(i - 1))
                                + (cut - values[i - 1]) / (values[i] - values[i - 1]),
                        );
                    }
                }
            }
        }
        self.edges.push(f64_f32(usize_f64(values.len() - 1)));
        self.runs.extend(self.edges.windows(2).map(|p| p[1] - p[0]));
    }
}

impl Sampler {
    /// Local min/max envelopes follow illumination without repairing transitions.
    pub fn adaptive_threshold(&mut self) {
        let n = self.values.len();
        if n < 2 {
            self.edges.clear();
            self.runs.clear();
            self.first_black = false;
            return;
        }
        self.widths.clear();
        self.widths
            .extend(self.runs.iter().copied().filter(|w| *w >= 1.));
        let widths = &mut self.widths;
        let radius = if widths.len() >= 20 {
            let k = widths.len() / 4;
            let w = *widths.select_nth_unstable_by(k, f32::total_cmp).1;
            crate::numeric::f32_usize(w * 6.).clamp(6, 256)
        } else {
            (n / 32).clamp(6, 128)
        };
        let block = 2 * radius + 1;
        self.envelope.resize(n, [0.; 4]);
        // Prefix/suffix extrema reset at block boundaries. Iterating chunks
        // removes two dynamic remainder operations per sample without changing
        // the envelope, including the partial block at the end of the profile.
        for (values, envelope) in self
            .values
            .chunks(block)
            .zip(self.envelope.chunks_mut(block))
        {
            let (mut lo, mut hi) = (values[0], values[0]);
            for (&v, out) in values.iter().zip(envelope.iter_mut()) {
                lo = minimum(lo, v);
                hi = maximum(hi, v);
                out[0] = lo;
                out[1] = hi;
            }
            let (mut lo, mut hi) = (values[values.len() - 1], values[values.len() - 1]);
            for (&v, out) in values.iter().zip(envelope.iter_mut()).rev() {
                lo = minimum(lo, v);
                hi = maximum(hi, v);
                out[2] = lo;
                out[3] = hi;
            }
        }
        let values = &self.values;
        let envelope = &self.envelope;
        let normalized = |i: usize| {
            let left = i.saturating_sub(radius);
            let right = (i + radius).min(n - 1);
            let lo = minimum(envelope[left][2], envelope[right][0]);
            let hi = maximum(envelope[left][3], envelope[right][1]);
            if hi - lo >= 8. {
                values[i] - (lo + hi) * 0.5
            } else {
                0.
            }
        };
        self.edges.clear();
        self.runs.clear();
        self.edges.push(0.);
        let mut a = normalized(0);
        self.first_black = a < 0.;
        for i in 1..n {
            let b = normalized(i);
            if (a < 0.) != (b < 0.) {
                self.edges.push(f64_f32(usize_f64(i - 1)) - a / (b - a));
            }
            a = b;
        }
        self.edges.push(f64_f32(usize_f64(n - 1)));
        self.runs.extend(self.edges.windows(2).map(|p| p[1] - p[0]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_positions_preserve_profiles_when_lengths_repeat_or_change() {
        let data: Vec<u8> = (0..257 * 131 * 3)
            .map(|i| u8::try_from((i * 71 + i / 5) % 256).unwrap())
            .collect();
        let image = ImageView::new(&data, 257, 131, 3, 257 * 3).unwrap();
        let mut sampler = Sampler::default();
        for width in [10., 30., 30., 70., 70., 127., 127., 190., 390., 3000., 10.] {
            for (start, end) in [([4., 7.], [width, 7.]), ([width, 91.], [-7., 112.])] {
                sampler.sample(image, start, end);
                for (i, &sample) in sampler.values.iter().enumerate() {
                    let t = usize_f64(i) / usize_f64(sampler.samples - 1);
                    let x = start[0] + (end[0] - start[0]) * t;
                    let y = start[1] + (end[1] - start[1]) * t;
                    let expected = if x < 0. || y < 0. || x > 256. || y > 130. {
                        255_f32
                    } else {
                        image.bilinear(x, y)
                    };
                    assert_eq!(sample.to_bits(), expected.to_bits());
                }
            }
        }
    }

    #[test]
    fn block_envelope_preserves_partial_edges_and_plateaus() {
        const BLOCK: usize = 25;
        for n in [2, 12, 24, 25, 26, 50, 61, 4096] {
            for flat in [false, true] {
                let values: Vec<f32> = (0..n)
                    .map(|i| {
                        if flat {
                            127.
                        } else {
                            f32::from(u8::try_from((i * 17 + i / 7) % 256).unwrap())
                        }
                    })
                    .collect();
                let mut legacy = vec![[0_f32; 4]; n];
                for i in 0..n {
                    let v = values[i];
                    legacy[i][0] = if i % BLOCK == 0 {
                        v
                    } else {
                        legacy[i - 1][0].min(v)
                    };
                    legacy[i][1] = if i % BLOCK == 0 {
                        v
                    } else {
                        legacy[i - 1][1].max(v)
                    };
                }
                for i in (0..n).rev() {
                    let v = values[i];
                    legacy[i][2] = if i == n - 1 || (i + 1) % BLOCK == 0 {
                        v
                    } else {
                        legacy[i + 1][2].min(v)
                    };
                    legacy[i][3] = if i == n - 1 || (i + 1) % BLOCK == 0 {
                        v
                    } else {
                        legacy[i + 1][3].max(v)
                    };
                }
                let normalized: Vec<f32> = (0..n)
                    .map(|i| {
                        let lo =
                            legacy[i.saturating_sub(12)][2].min(legacy[(i + 12).min(n - 1)][0]);
                        let hi =
                            legacy[i.saturating_sub(12)][3].max(legacy[(i + 12).min(n - 1)][1]);
                        if hi - lo >= 8. {
                            values[i] - (lo + hi) * 0.5
                        } else {
                            0.
                        }
                    })
                    .collect();
                let mut expected = vec![0.];
                for i in 1..n {
                    let (a, b) = (normalized[i - 1], normalized[i]);
                    if (a < 0.) != (b < 0.) {
                        expected.push(f64_f32(usize_f64(i - 1)) - a / (b - a));
                    }
                }
                expected.push(f64_f32(usize_f64(n - 1)));
                let mut sampler = Sampler {
                    values,
                    runs: vec![2.; 40],
                    ..Sampler::default()
                };
                sampler.adaptive_threshold();
                assert_eq!(sampler.envelope, legacy);
                assert_eq!(
                    sampler
                        .edges
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
                );
                assert_eq!(sampler.first_black, normalized[0] < 0.);
            }
        }
    }

    #[test]
    fn flat_skip_keeps_the_weakest_adaptive_contrast() {
        for contrast in [0., 7.99, 8., 10., 16., 255.] {
            let sampler = Sampler {
                values: vec![0., contrast, 0., contrast],
                ..Sampler::default()
            };
            assert_eq!(sampler.has_contrast(), contrast >= 8.);
        }
    }

    #[test]
    fn printed_label_threshold_ignores_bright_exterior() {
        let digits = [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7];
        let bits = crate::ean::encode(&digits);
        let mut row = vec![255; 600];
        row[30..570].fill(154);
        for (i, &bit) in bits.iter().enumerate() {
            if bit > 0.5 {
                row[85 + i * 4..89 + i * 4].fill(103);
            }
        }
        let mut image = row.clone();
        image.extend_from_slice(&row);
        image.extend_from_slice(&row);
        let mut sampler = Sampler::default();
        sampler.sample(
            ImageView::new(&image, 600, 3, 1, 600).unwrap(),
            [0., 1.],
            [599., 1.],
        );
        sampler.threshold(false);
        let decoded = (usize::from(!sampler.first_black)..sampler.runs.len().saturating_sub(59))
            .step_by(2)
            .filter_map(|i| crate::run_ean::decode(&sampler.runs[i..i + 59]))
            .collect::<Vec<_>>();
        assert_eq!(decoded, vec![digits]);
    }
    #[test]
    fn contrast_restoration_preserves_source_coordinates_and_flat_profiles() {
        let pixels = [127; 300];
        let mut sampler = Sampler::default();
        sampler.sample(
            ImageView::new(&pixels, 100, 3, 1, 100).unwrap(),
            [0., 1.],
            [99., 1.],
        );
        let positions = sampler.positions.clone();
        let values = sampler.values.clone();
        let count = sampler.samples;
        sampler.restore_contrast(2.);
        assert_eq!(sampler.positions, positions);
        assert_eq!(sampler.samples, count);
        assert_eq!(sampler.values, values);
        sampler.threshold(true);
        assert!(sampler.runs.len() <= 2);
    }
    #[test]
    fn uniform_profiles_do_not_invent_bars() {
        for value in [0, 127, 255] {
            let image = vec![value; 300];
            let mut sampler = Sampler::default();
            sampler.sample(
                ImageView::new(&image, 100, 3, 1, 100).unwrap(),
                [0., 1.],
                [99., 1.],
            );
            sampler.threshold(true);
            assert!(sampler.runs.len() <= 2);
            sampler.adaptive_threshold();
            assert!(sampler.runs.len() <= 2);
            sampler.threshold(false);
            assert!(sampler.runs.len() <= 2);
        }
    }
}

#[cfg(test)]
mod chromatic_profile_tests {
    use super::*;

    #[test]
    fn chromatic_sampling_preserves_stride_alpha_and_skips_neutral_profiles() {
        let (width, height) = (64, 3);
        let mut packed = vec![0; width * height * 3];
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) * 3;
                let color = if (x / 4) % 2 == 0 {
                    [25, 40, 140]
                } else {
                    [220, 210, 180]
                };
                packed[at..at + 3].copy_from_slice(&color);
            }
        }
        let rgb = ImageView::new(&packed, width, height, 3, width * 3).unwrap();
        let mut reference = Sampler::default();
        assert!(reference.sample_color_limited(rgb, [0., 0.5], [63., 1.5], 1.5, 1024, 2));
        let stride = width * 4 + 13;
        let mut padded = vec![91; stride * height];
        for y in 0..height {
            for x in 0..width {
                let from = (y * width + x) * 3;
                let to = y * stride + x * 4;
                padded[to..to + 3].copy_from_slice(&packed[from..from + 3]);
                padded[to + 3] = u8::try_from(x).unwrap();
            }
        }
        padded.truncate((height - 1) * stride + width * 4);
        let rgba = ImageView::new(&padded, width, height, 4, stride).unwrap();
        let mut sampler = Sampler::default();
        assert!(sampler.sample_color_limited(rgba, [0., 0.5], [63., 1.5], 1.5, 1024, 2));
        assert_eq!(sampler.values, reference.values);
        assert_eq!(sampler.samples, reference.samples);
        for pixel in packed.chunks_exact_mut(3) {
            pixel.fill(pixel[0]);
        }
        let neutral = ImageView::new(&packed, width, height, 3, width * 3).unwrap();
        assert!(!sampler.sample_color_limited(neutral, [0., 0.5], [63., 1.5], 1.5, 1024, 2));
        let gray = vec![127; width * height];
        let gray = ImageView::new(&gray, width, height, 1, width).unwrap();
        assert!(!sampler.sample_color_limited(gray, [0., 0.5], [63., 1.5], 1.5, 1024, 2));
    }
}

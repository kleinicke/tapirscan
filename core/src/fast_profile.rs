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

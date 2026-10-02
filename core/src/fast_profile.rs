//! Reusable bounded source-resolution profiles for the experimental fast reader.
use crate::{
    numeric::{f64_f32, usize_f64},
    sampling::ImageView,
};

// Image-derived profiles are finite nonnegative values. Medium uses direct
// comparisons; the retained Turbo switch preserves its independent policy.
fn minimum(a: f32, b: f32) -> f32 {
    if cfg!(feature = "mode-medium") || option_env!("TAPIRSCAN_TURBO_FINITE_EXTREMA").is_some() {
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
    if cfg!(feature = "mode-medium") || option_env!("TAPIRSCAN_TURBO_FINITE_EXTREMA").is_some() {
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
struct ThresholdCache {
    valid: bool,
    radius: usize,
    edges: Vec<f32>,
    runs: Vec<f32>,
    first_black: bool,
}
struct SourceCacheEntry {
    image: [usize; 5],
    line: [u64; 6],
    values: Vec<f32>,
    thresholds: [ThresholdCache; 3],
}

#[derive(Default)]
pub struct Sampler {
    cache_active: bool,
    cache_image: Option<[usize; 5]>,
    cache_current: Option<usize>,
    source_cache_used: usize,
    source_cache: Vec<SourceCacheEntry>,
    values: Vec<f32>,
    /// Reused copy of `values` for contrast hypotheses.
    restore_scratch: Vec<f32>,
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
    /// Reuse exact source profiles only inside one immutable-image transaction.
    /// Restored values never replace the original cached samples.
    pub fn with_source_cache<T>(
        &mut self,
        image: ImageView<'_>,
        enabled: bool,
        run: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.source_cache_used = 0;
        self.cache_current = None;
        self.cache_image = Some([
            image.data.as_ptr().addr(),
            image.width,
            image.height,
            image.channels,
            image.stride,
        ]);
        self.cache_active = enabled;
        let result = run(self);
        self.cache_active = false;
        self.cache_current = None;
        self.cache_image = None;
        self.source_cache_used = 0;
        result
    }
    fn load_threshold(&mut self, slot: usize, radius: usize) -> bool {
        let Some(index) = self.cache_current else {
            return false;
        };
        let saved = &self.source_cache[index].thresholds[slot];
        if !saved.valid || saved.radius != radius {
            return false;
        }
        self.edges.clone_from(&saved.edges);
        self.runs.clone_from(&saved.runs);
        self.first_black = saved.first_black;
        true
    }
    fn save_threshold(&mut self, slot: usize, radius: usize) {
        let Some(index) = self.cache_current else {
            return;
        };
        let saved = &mut self.source_cache[index].thresholds[slot];
        saved.valid = true;
        saved.radius = radius;
        saved.edges.clone_from(&self.edges);
        saved.runs.clone_from(&self.runs);
        saved.first_black = self.first_black;
    }
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
        self.cache_current = None;
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
        self.cache_current = None;
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
        let cache_image = [
            image.data.as_ptr().addr(),
            image.width,
            image.height,
            image.channels,
            image.stride,
        ];
        let cache_line = [
            start[0].to_bits(),
            start[1].to_bits(),
            end[0].to_bits(),
            end[1].to_bits(),
            density.to_bits(),
            u64::try_from(limit).unwrap_or(u64::MAX),
        ];
        if self.cache_active && self.cache_image == Some(cache_image) {
            if let Some(index) = self.source_cache[..self.source_cache_used]
                .iter()
                .position(|entry| entry.image == cache_image && entry.line == cache_line)
            {
                self.values.clone_from(&self.source_cache[index].values);
                self.cache_current = Some(index);
                return;
            }
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
        if self.cache_active && self.cache_image == Some(cache_image) && self.source_cache_used < 64
        {
            let index = self.source_cache_used;
            if index == self.source_cache.len() {
                self.source_cache.push(SourceCacheEntry {
                    image: cache_image,
                    line: cache_line,
                    values: Vec::new(),
                    thresholds: std::array::from_fn(|_| ThresholdCache::default()),
                });
            }
            let entry = &mut self.source_cache[index];
            entry.image = cache_image;
            entry.line = cache_line;
            entry.values.clone_from(&self.values);
            for threshold in &mut entry.thresholds {
                threshold.valid = false;
            }
            self.source_cache_used += 1;
            self.cache_current = Some(index);
        }
    }
    /// Average three parallel original-pixel lines without changing module positions.
    pub fn sample_band_limited(
        &mut self,
        image: ImageView<'_>,
        start: [f64; 2],
        end: [f64; 2],
        density: f64,
        limit: usize,
        offset: f64,
    ) {
        self.sample_limited(image, start, end, density, limit);
        self.cache_current = None;
        let length = (end[0] - start[0]).hypot(end[1] - start[1]);
        if length <= 0. {
            return;
        }
        let normal = [
            -(end[1] - start[1]) / length * offset,
            (end[0] - start[0]) / length * offset,
        ];
        let mut upper = crate::sampling::BilinearCursor::new(image);
        let mut lower = crate::sampling::BilinearCursor::new(image);
        for (i, &t) in self.positions.iter().enumerate() {
            let x = start[0] + (end[0] - start[0]) * t;
            let y = start[1] + (end[1] - start[1]) * t;
            let inside = |x: f64, y: f64| {
                x >= 0.
                    && y >= 0.
                    && x <= usize_f64(image.width - 1)
                    && y <= usize_f64(image.height - 1)
            };
            let (ux, uy) = (x + normal[0], y + normal[1]);
            let (lx, ly) = (x - normal[0], y - normal[1]);
            let a = if inside(ux, uy) {
                upper.sample(ux, uy)
            } else {
                255.
            };
            let b = if inside(lx, ly) {
                lower.sample(lx, ly)
            } else {
                255.
            };
            self.values[i] = (self.values[i] + a + b) / 3.;
        }
    }
    /// Restore a bounded profile contrast hypothesis without changing source positions.
    pub fn restore_contrast(&mut self, strength: f32) {
        self.cache_current = None;
        let mut original = std::mem::take(&mut self.restore_scratch);
        original.clone_from(&self.values);
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
        self.restore_scratch = original;
    }
    /// Wider bounded contrast hypothesis for independently checked EAN recovery.
    pub fn restore_contrast_wide(&mut self, strength: f32) {
        self.cache_current = None;
        let n = self.values.len();
        if n == 0 {
            return;
        }
        let mut original = std::mem::take(&mut self.restore_scratch);
        original.clone_from(&self.values);
        let weights = [1., 4., 11., 21., 26., 21., 11., 4., 1.];
        for i in 0..n {
            let mut blurred = 0.;
            for (k, weight) in weights.into_iter().enumerate() {
                blurred += original[(i + k).saturating_sub(4).min(n - 1)] * weight;
            }
            self.values[i] =
                (original[i] + strength * (original[i] - blurred / 100.)).clamp(0., 255.);
        }
        self.restore_scratch = original;
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
        let slot = if local { 0 } else { 2 };
        if self.load_threshold(slot, 0) {
            return;
        }
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
        self.save_threshold(slot, 0);
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
        if self.load_threshold(1, radius) {
            return;
        }
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
        self.save_threshold(1, radius);
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

#[cfg(test)]
mod scoped_source_cache_tests {
    use super::*;

    fn compare(a: &Sampler, b: &Sampler) {
        assert_eq!(a.values, b.values);
        assert_eq!(a.positions, b.positions);
        assert_eq!(a.samples, b.samples);
        assert_eq!(a.edges, b.edges);
        assert_eq!(a.runs, b.runs);
        assert_eq!(a.first_black, b.first_black);
    }

    #[test]
    fn cache_preserves_thresholds_after_restoration_and_reversed_runs() {
        let pixels: Vec<u8> = (0..640 * 8)
            .map(|i| u8::try_from((i * 37 + i / 11) % 256).unwrap())
            .collect();
        let image = ImageView::new(&pixels, 640, 8, 1, 640).unwrap();
        let mut cached = Sampler::default();
        let mut plain = Sampler::default();
        cached.with_source_cache(image, true, |cached| {
            for repeat in 0..5 {
                for (a, b) in [([-10., 2.], [650., 5.]), ([610., 5.], [12., 1.])] {
                    cached.sample_limited(image, a, b, 1.5, 1536);
                    plain.sample_limited(image, a, b, 1.5, 1536);
                    // Adaptive threshold reads the previous run widths. Both histories
                    // include reversed runs and changes in source-restoration policy.
                    for local in [true, false] {
                        cached.threshold(local);
                        plain.threshold(local);
                        compare(cached, &plain);
                        cached.runs.reverse();
                        plain.runs.reverse();
                        cached.adaptive_threshold();
                        plain.adaptive_threshold();
                        compare(cached, &plain);
                    }
                    if repeat % 2 == 0 {
                        cached.restore_contrast(1.5);
                        plain.restore_contrast(1.5);
                    } else {
                        cached.restore_contrast_wide(1.5);
                        plain.restore_contrast_wide(1.5);
                    }
                    cached.threshold(true);
                    plain.threshold(true);
                    compare(cached, &plain);
                }
            }
        });
    }

    #[test]
    fn reused_image_address_cannot_reuse_previous_frame_pixels() {
        let mut pixels = vec![255; 320 * 3];
        let mut cached = Sampler::default();
        for frame in 0..8 {
            for (i, value) in pixels.iter_mut().enumerate() {
                *value = if (i / (frame + 2)) % 2 == 0 { 15 } else { 235 };
            }
            let image = ImageView::new(&pixels, 320, 3, 1, 320).unwrap();
            let mut plain = Sampler::default();
            plain.sample(image, [0., 1.], [319., 1.]);
            plain.threshold(true);
            cached.with_source_cache(image, true, |cached| {
                for _ in 0..2 {
                    cached.sample(image, [0., 1.], [319., 1.]);
                    cached.threshold(true);
                    compare(cached, &plain);
                }
            });
        }
    }

    #[test]
    fn cache_scope_does_not_alias_other_images_or_color_profiles() {
        let (width, height, stride) = (80, 4, 337);
        let mut pixels = vec![91; stride * height];
        for y in 0..height {
            for x in 0..width {
                let color = if x % 7 < 3 {
                    [15, 60, 140, 0]
                } else {
                    [230, 215, 180, 255]
                };
                pixels[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&color);
            }
        }
        let other = vec![127; width * height];
        let image = ImageView::new(&pixels, width, height, 4, stride).unwrap();
        let other = ImageView::new(&other, width, height, 1, width).unwrap();
        let mut cached = Sampler::default();
        let mut plain = Sampler::default();
        cached.with_source_cache(image, true, |cached| {
            for source in [image, other, image] {
                cached.sample(source, [-3., 1.], [82., 2.]);
                plain.sample(source, [-3., 1.], [82., 2.]);
                cached.threshold(true);
                plain.threshold(true);
                compare(cached, &plain);
            }
            assert_eq!(
                cached.sample_color_limited(image, [0., 1.], [79., 2.], 1.5, 512, 2),
                plain.sample_color_limited(image, [0., 1.], [79., 2.], 1.5, 512, 2)
            );
            cached.threshold(true);
            plain.threshold(true);
            compare(cached, &plain);
            cached.sample(image, [-3., 1.], [82., 2.]);
            plain.sample(image, [-3., 1.], [82., 2.]);
            cached.threshold(true);
            plain.threshold(true);
            compare(cached, &plain);
        });
    }
}

#[cfg(test)]
mod finite_extrema_tests {
    #[test]
    fn comparison_extrema_preserve_bounded_profile_values() {
        let mut values: Vec<f32> = (0_u8..=255).map(f32::from).collect();
        values.extend([0.001, 7.99, 8.001, 127.5, 254.99]);
        for &a in &values {
            for &b in &values {
                assert_eq!(super::minimum(a, b).to_bits(), a.min(b).to_bits());
                assert_eq!(super::maximum(a, b).to_bits(), a.max(b).to_bits());
            }
        }
    }
}

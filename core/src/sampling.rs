//! Validated borrowed images and reusable original-resolution path sampling.
//! No unsafe code, image ownership, decoder decisions, or expected labels here.
#![forbid(unsafe_code)]
use std::fmt;

/// Exact 5th/95th order statistics in reusable scratch storage. Full ordering
/// is unnecessary; retain `total_cmp` semantics (including ties and signed zero).
/// Every caller supplies at least64 finite image samples.
pub(crate) fn contrast_bounds(values: &mut [f32]) -> (f32, f32) {
    let n = values.len();
    assert!(n >= 64);
    let high = n * 95 / 100;
    let low = n * 5 / 100;
    let (lower, pivot, _) = values.select_nth_unstable_by(high, f32::total_cmp);
    let hi = *pivot;
    let lo = *lower.select_nth_unstable_by(low, f32::total_cmp).1;
    (lo, hi)
}

pub const MAX_IMAGE_BYTES: usize = 128 * 1024 * 1024;
pub const PROFILE_LEN: usize = 512;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Dimensions,
    Channels,
    Stride,
    BufferLength,
    Geometry,
    Path,
    Allocation,
    OutputShape,
    Parameters,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid sampler {self:?}")
    }
}
impl std::error::Error for Error {}

/// Pixel coordinates refer to the original image; stride is in bytes.
#[derive(Clone, Copy)]
pub struct ImageView<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) channels: usize,
    pub(crate) stride: usize,
}
const fn luminance_table(weight: f64) -> [f64; 256] {
    let mut a = [0.; 256];
    let mut i = 0;
    while i < 256 {
        a[i] = weight * crate::numeric::usize_f64(i);
        i += 1;
    }
    a
}
const RED: [f64; 256] = luminance_table(0.299);
const GREEN: [f64; 256] = luminance_table(0.587);
const BLUE: [f64; 256] = luminance_table(0.114);
#[inline]
fn rgb_luminance(r: u8, g: u8, b: u8) -> f64 {
    RED[r as usize] + GREEN[g as usize] + BLUE[b as usize]
}
impl<'a> ImageView<'a> {
    /// # Errors
    /// Rejects invalid dimensions, channels, stride, size overflow and undersized image buffers.
    pub fn new(
        data: &'a [u8],
        width: usize,
        height: usize,
        channels: usize,
        stride: usize,
    ) -> Result<Self, Error> {
        let required = image_len(width, height, channels, stride)?;
        if data.len() < required {
            return Err(Error::BufferLength);
        }
        Ok(Self {
            data,
            width,
            height,
            channels,
            stride,
        })
    }
    #[cfg(any(feature = "mode-low", feature = "mode-very-high"))]
    /// Share the clamped integer coordinates and row offsets of a bilinear
    /// footprint, retaining the legacy grayscale and interpolation order.
    #[expect(
        clippy::inline_always,
        reason = "The pinned sampling experiment forces these inner pixel operations inline; retain its code-generation policy and validate with paired end-to-end measurements."
    )]
    #[inline(always)]
    pub(crate) fn bilinear(self, x: f64, y: f64) -> f32 {
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let (left_offset, right_offset, top_offset, bottom_offset) = if x0 >= 0.
            && y0 >= 0.
            && x0 < crate::numeric::usize_f64(self.width - 1)
            && y0 < crate::numeric::usize_f64(self.height - 1)
        {
            let left_offset = crate::numeric::f64_usize(x0) * self.channels;
            let top_offset = crate::numeric::f64_usize(y0) * self.stride;
            (
                left_offset,
                left_offset + self.channels,
                top_offset,
                top_offset + self.stride,
            )
        } else {
            let left_offset =
                crate::numeric::f64_usize(x0.clamp(0., crate::numeric::usize_f64(self.width - 1)))
                    * self.channels;
            let right_offset = crate::numeric::f64_usize(
                (x0 + 1.).clamp(0., crate::numeric::usize_f64(self.width - 1)),
            ) * self.channels;
            let top_offset =
                crate::numeric::f64_usize(y0.clamp(0., crate::numeric::usize_f64(self.height - 1)))
                    * self.stride;
            let bottom_offset = crate::numeric::f64_usize(
                (y0 + 1.).clamp(0., crate::numeric::usize_f64(self.height - 1)),
            ) * self.stride;
            (left_offset, right_offset, top_offset, bottom_offset)
        };
        let gray = |i: usize| {
            if self.channels == 1 {
                f64::from(self.data[i])
            } else {
                rgb_luminance(self.data[i], self.data[i + 1], self.data[i + 2])
            }
        };
        crate::numeric::f64_f32(
            (gray(top_offset + left_offset) * (1. - fx) + gray(top_offset + right_offset) * fx)
                * (1. - fy)
                + (gray(bottom_offset + left_offset) * (1. - fx)
                    + gray(bottom_offset + right_offset) * fx)
                    * fy,
        )
    }
    #[cfg(feature = "mode-medium")]
    /// Share the clamped integer coordinates and row offsets of a bilinear
    /// footprint, retaining the legacy grayscale and interpolation order.
    #[expect(
        clippy::inline_always,
        reason = "The pinned sampling experiment forces these inner pixel operations inline; retain its code-generation policy and validate with paired end-to-end measurements."
    )]
    #[inline(always)]
    pub(crate) fn bilinear(self, x: f64, y: f64) -> f32 {
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let left_offset =
            crate::numeric::f64_usize(x0.clamp(0., crate::numeric::usize_f64(self.width - 1)))
                * self.channels;
        let right_offset = crate::numeric::f64_usize(
            (x0 + 1.).clamp(0., crate::numeric::usize_f64(self.width - 1)),
        ) * self.channels;
        let top_offset =
            crate::numeric::f64_usize(y0.clamp(0., crate::numeric::usize_f64(self.height - 1)))
                * self.stride;
        let bottom_offset = crate::numeric::f64_usize(
            (y0 + 1.).clamp(0., crate::numeric::usize_f64(self.height - 1)),
        ) * self.stride;
        let gray = |i: usize| {
            if self.channels == 1 {
                f64::from(self.data[i])
            } else {
                rgb_luminance(self.data[i], self.data[i + 1], self.data[i + 2])
            }
        };
        crate::numeric::f64_f32(
            (gray(top_offset + left_offset) * (1. - fx) + gray(top_offset + right_offset) * fx)
                * (1. - fy)
                + (gray(bottom_offset + left_offset) * (1. - fx)
                    + gray(bottom_offset + right_offset) * fx)
                    * fy,
        )
    }
    #[cfg(feature = "mode-high")]
    /// Share the clamped integer coordinates and row offsets of a bilinear
    /// footprint, retaining the legacy grayscale and interpolation order.
    pub(crate) fn bilinear(self, x: f64, y: f64) -> f32 {
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let left_offset =
            crate::numeric::f64_usize(x0.clamp(0., crate::numeric::usize_f64(self.width - 1)))
                * self.channels;
        let right_offset = crate::numeric::f64_usize(
            (x0 + 1.).clamp(0., crate::numeric::usize_f64(self.width - 1)),
        ) * self.channels;
        let top_offset =
            crate::numeric::f64_usize(y0.clamp(0., crate::numeric::usize_f64(self.height - 1)))
                * self.stride;
        let bottom_offset = crate::numeric::f64_usize(
            (y0 + 1.).clamp(0., crate::numeric::usize_f64(self.height - 1)),
        ) * self.stride;
        let gray = |i: usize| {
            if self.channels == 1 {
                f64::from(self.data[i])
            } else {
                rgb_luminance(self.data[i], self.data[i + 1], self.data[i + 2])
            }
        };
        crate::numeric::f64_f32(
            (gray(top_offset + left_offset) * (1. - fx) + gray(top_offset + right_offset) * fx)
                * (1. - fy)
                + (gray(bottom_offset + left_offset) * (1. - fx)
                    + gray(bottom_offset + right_offset) * fx)
                    * fy,
        )
    }

    #[cfg(any(
        feature = "mode-low",
        feature = "mode-medium",
        feature = "mode-very-high"
    ))]
    #[expect(
        clippy::inline_always,
        reason = "The pinned sampling experiment forces these inner pixel operations inline; retain its code-generation policy and validate with paired end-to-end measurements."
    )]
    #[inline(always)]
    pub(crate) fn gray(self, x: f64, y: f64) -> f64 {
        let x = crate::numeric::f64_usize(x.clamp(0., crate::numeric::usize_f64(self.width - 1)));
        let y = crate::numeric::f64_usize(y.clamp(0., crate::numeric::usize_f64(self.height - 1)));
        let i = y * self.stride + x * self.channels;
        if self.channels == 1 {
            f64::from(self.data[i])
        } else {
            rgb_luminance(self.data[i], self.data[i + 1], self.data[i + 2])
        }
    }
    #[cfg(feature = "mode-high")]
    pub(crate) fn gray(self, x: f64, y: f64) -> f64 {
        let x = crate::numeric::f64_usize(x.clamp(0., crate::numeric::usize_f64(self.width - 1)));
        let y = crate::numeric::f64_usize(y.clamp(0., crate::numeric::usize_f64(self.height - 1)));
        let i = y * self.stride + x * self.channels;
        if self.channels == 1 {
            f64::from(self.data[i])
        } else {
            rgb_luminance(self.data[i], self.data[i + 1], self.data[i + 2])
        }
    }
}
/// Exact source-luminance reuse for adjacent samples of one immutable image.
/// The interpolation order and precision remain identical to `ImageView::bilinear`.
pub(crate) struct BilinearCursor<'a> {
    image: ImageView<'a>,
    key: [usize; 4],
    values: [f64; 4],
}
impl<'a> BilinearCursor<'a> {
    pub(crate) fn new(image: ImageView<'a>) -> Self {
        Self {
            image,
            key: [usize::MAX; 4],
            values: [0.; 4],
        }
    }

    #[expect(
        clippy::inline_always,
        reason = "Keep the same inlining policy as the source sampler; paired benchmarks decide whether cross-sample reuse is worthwhile."
    )]
    #[inline(always)]
    pub(crate) fn sample(&mut self, x: f64, y: f64) -> f32 {
        let image = self.image;
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let (left, right, top, bottom) =
            if cfg!(any(feature = "mode-low", feature = "mode-very-high"))
                && x0 >= 0.
                && y0 >= 0.
                && x0 < crate::numeric::usize_f64(image.width - 1)
                && y0 < crate::numeric::usize_f64(image.height - 1)
            {
                let left = crate::numeric::f64_usize(x0) * image.channels;
                let top = crate::numeric::f64_usize(y0) * image.stride;
                (left, left + image.channels, top, top + image.stride)
            } else {
                (
                    crate::numeric::f64_usize(
                        x0.clamp(0., crate::numeric::usize_f64(image.width - 1)),
                    ) * image.channels,
                    crate::numeric::f64_usize(
                        (x0 + 1.).clamp(0., crate::numeric::usize_f64(image.width - 1)),
                    ) * image.channels,
                    crate::numeric::f64_usize(
                        y0.clamp(0., crate::numeric::usize_f64(image.height - 1)),
                    ) * image.stride,
                    crate::numeric::f64_usize(
                        (y0 + 1.).clamp(0., crate::numeric::usize_f64(image.height - 1)),
                    ) * image.stride,
                )
            };
        let key = [left, right, top, bottom];
        if key != self.key {
            let gray = |i: usize| {
                if image.channels == 1 {
                    f64::from(image.data[i])
                } else {
                    rgb_luminance(image.data[i], image.data[i + 1], image.data[i + 2])
                }
            };
            let [old_left, old_right, old_top, old_bottom] = self.key;
            let [tl, tr, bl, br] = self.values;
            self.values = if top == old_top && bottom == old_bottom && left == old_right {
                [tr, gray(top + right), br, gray(bottom + right)]
            } else if top == old_top && bottom == old_bottom && right == old_left {
                [gray(top + left), tl, gray(bottom + left), bl]
            } else if left == old_left && right == old_right && top == old_bottom {
                [bl, br, gray(bottom + left), gray(bottom + right)]
            } else if left == old_left && right == old_right && bottom == old_top {
                [gray(top + left), gray(top + right), tl, tr]
            } else {
                [
                    gray(top + left),
                    gray(top + right),
                    gray(bottom + left),
                    gray(bottom + right),
                ]
            };
            self.key = key;
        }
        let [top_left, top_right, bottom_left, bottom_right] = self.values;
        crate::numeric::f64_f32(
            (top_left * (1. - fx) + top_right * fx) * (1. - fy)
                + (bottom_left * (1. - fx) + bottom_right * fx) * fy,
        )
    }
}

/// # Errors
/// Rejects zero/oversized dimensions, unsupported channel counts, short strides and size overflow.
pub fn image_len(
    width: usize,
    height: usize,
    channels: usize,
    stride: usize,
) -> Result<usize, Error> {
    if width == 0 || height == 0 {
        return Err(Error::Dimensions);
    }
    if ![1, 3, 4].contains(&channels) {
        return Err(Error::Channels);
    }
    let row = width.checked_mul(channels).ok_or(Error::Dimensions)?;
    if stride < row {
        return Err(Error::Stride);
    }
    let n = (height - 1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(row))
        .ok_or(Error::Dimensions)?;
    if n > MAX_IMAGE_BYTES {
        return Err(Error::Dimensions);
    }
    Ok(n)
}

/// Homography from normalized candidate coordinates into original-image pixels.
/// Callers retain their original polygon. Construction rejects singular matrices.
#[derive(Clone, Copy, Debug)]
pub struct Transform(pub(crate) [f64; 9]);
impl Transform {
    /// # Errors
    /// Returns `Geometry` for a non-finite or degenerate transform.
    pub fn new(m: [f64; 9]) -> Result<Self, Error> {
        if m.iter().any(|v| !v.is_finite()) {
            return Err(Error::Geometry);
        }
        let det = m[0] * (m[4] * m[8] - m[5] * m[7]) - m[1] * (m[3] * m[8] - m[5] * m[6])
            + m[2] * (m[3] * m[7] - m[4] * m[6]);
        if !det.is_finite() || det.abs() < 1e-12 {
            return Err(Error::Geometry);
        }
        Ok(Self(m))
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Path {
    pub axis: usize,
    pub fraction: f64,
    pub curve: f64,
    pub margin: f64,
}
impl Default for Path {
    fn default() -> Self {
        Self {
            axis: 0,
            fraction: 0.5,
            curve: 0.,
            margin: 0.15,
        }
    }
}
/// Profile is borrowed until the next mutable operation; buffers are reused.
pub struct Sampler {
    profile: [f32; PROFILE_LEN],
    sorted: [f32; PROFILE_LEN],
}
impl Default for Sampler {
    fn default() -> Self {
        Self {
            profile: [0.; PROFILE_LEN],
            sorted: [0.; PROFILE_LEN],
        }
    }
}
impl Sampler {
    /// # Errors
    /// Rejects invalid paths and projective poles or non-finite projected coordinates.
    pub fn sample(
        &mut self,
        image: ImageView<'_>,
        transform: Transform,
        path: Path,
    ) -> Result<Option<&[f32; PROFILE_LEN]>, Error> {
        if path.axis > 1
            || !path.fraction.is_finite()
            || !(0.0..=1.0).contains(&path.fraction)
            || !path.curve.is_finite()
            || path.curve.abs() > 1.
            || !path.margin.is_finite()
            || !(0.0..=1.0).contains(&path.margin)
        {
            return Err(Error::Path);
        }
        let matrix = transform.0;
        // Reject projective poles throughout the continuous curved path, including
        // endpoints and the quadratic extremum, before touching the output.
        let (along_coefficient, across_coefficient, constant_coefficient) = if path.axis == 0 {
            (matrix[6], matrix[7], matrix[8])
        } else {
            (matrix[7], matrix[6], matrix[8])
        };
        let denominator = |u: f64| {
            along_coefficient * u
                + across_coefficient * (path.fraction + path.curve * (1. - (2. * u - 1.).powi(2)))
                + constant_coefficient
        };
        let lo = -path.margin;
        let hi = 1. + path.margin;
        let mut zs = [denominator(lo), denominator(hi), denominator(lo)];
        if across_coefficient * path.curve != 0. {
            let u = (along_coefficient + 4. * across_coefficient * path.curve)
                / (8. * across_coefficient * path.curve);
            if u > lo && u < hi {
                zs[2] = denominator(u);
            }
        }
        if zs.iter().any(|v| !v.is_finite() || v.abs() < 1e-9)
            || (zs.iter().any(|v| *v < 0.) && zs.iter().any(|v| *v > 0.))
        {
            return Err(Error::Geometry);
        }
        for i in 0..PROFILE_LEN {
            let u = -path.margin
                + (1. + 2. * path.margin) * (crate::numeric::usize_f64(i) + 0.5)
                    / crate::numeric::usize_f64(PROFILE_LEN);
            let v = path.fraction + path.curve * (1. - (2. * u - 1.).powi(2));
            let (x, y) = if path.axis == 0 { (u, v) } else { (v, u) };
            let denominator = matrix[6] * x + matrix[7] * y + matrix[8];
            let sx = (matrix[0] * x + matrix[1] * y + matrix[2]) / denominator - 0.5;
            let sy = (matrix[3] * x + matrix[4] * y + matrix[5]) / denominator - 0.5;
            if !sx.is_finite() || !sy.is_finite() {
                return Err(Error::Geometry);
            }
            self.profile[i] = image.bilinear(sx, sy);
        }
        self.sorted.copy_from_slice(&self.profile);
        let (lo, hi) = contrast_bounds(&mut self.sorted);
        let (lo, hi) = (f64::from(lo), f64::from(hi));
        if hi - lo < 8. {
            return Ok(None);
        }
        for v in &mut self.profile {
            *v = crate::numeric::f64_f32(((hi - f64::from(*v)) / (hi - lo)).clamp(0., 1.));
        }
        Ok(Some(&self.profile))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_matches_uncached_bits_for_color_stride_boundary_and_direction() {
        for channels in [1, 3, 4] {
            for (width, height) in [(1, 1), (1, 7), (9, 1), (9, 7)] {
                let stride = width * channels + 5;
                let mut data = vec![0; stride * height];
                for (i, byte) in data.iter_mut().enumerate() {
                    *byte = u8::try_from((i * 71 + i / 3) % 256).unwrap();
                }
                let image = ImageView::new(&data, width, height, channels, stride).unwrap();
                let mut cursor = BilinearCursor::new(image);
                for direction in [1., -1.] {
                    for i in 0..400 {
                        let x = direction * (f64::from(i) / 23. - 2.);
                        let y = (f64::from(i) / 31.).sin() * 8.;
                        assert_eq!(
                            cursor.sample(x, y).to_bits(),
                            image.bilinear(x, y).to_bits()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shared_bilinear_preserves_channels_stride_and_border_arithmetic() {
        for channels in [1usize, 3, 4] {
            for padding in [0, 7] {
                let (w, h) = (9, 8);
                let stride = w * channels + padding;
                let data: Vec<u8> = (0..stride * h)
                    .map(|i| ((i * 73 + 19) % 256).to_le_bytes()[0])
                    .collect();
                let im = ImageView::new(&data, w, h, channels, stride).unwrap();
                for x in [-9999.2, -2.25, -0.5, 0., 0.2, 1., 2.7, 8.8, 9999.2] {
                    for y in [-9999.2, -2.25, -0.5, 0., 0.2, 1., 2.7, 7.8, 9999.2] {
                        let (x0, y0) = (f64::floor(x), f64::floor(y));
                        let (fx, fy) = (x - x0, y - y0);
                        let old = crate::numeric::f64_f32(
                            (im.gray(x0, y0) * (1. - fx) + im.gray(x0 + 1., y0) * fx) * (1. - fy)
                                + (im.gray(x0, y0 + 1.) * (1. - fx)
                                    + im.gray(x0 + 1., y0 + 1.) * fx)
                                    * fy,
                        );
                        assert_eq!(old.to_bits(), im.bilinear(x, y).to_bits());
                    }
                }
            }
        }
    }
    #[test]
    fn selected_bounds_match_sorted_values_bitwise() {
        let mut seed = 17u32;
        for n in [64, 65, 127, 512, 513, 1024, 4096] {
            for kind in 0..5 {
                let mut p: Vec<f32> = (0..n)
                    .map(|i| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        match kind {
                            0 => crate::numeric::f64_f32(f64::from(seed >> 8)) / 65536.,
                            1 => crate::numeric::usize_f32(i % 3),
                            2 => crate::numeric::usize_f32(i),
                            3 => crate::numeric::usize_f32(n - i),
                            _ => {
                                if i % 2 == 0 {
                                    0.
                                } else {
                                    -0.
                                }
                            }
                        }
                    })
                    .collect();
                let mut sorted = p.clone();
                sorted.sort_unstable_by(f32::total_cmp);
                let (lo, hi) = contrast_bounds(&mut p);
                assert_eq!(lo.to_bits(), sorted[n * 5 / 100].to_bits());
                assert_eq!(hi.to_bits(), sorted[n * 95 / 100].to_bits());
            }
        }
    }
    #[test]
    fn validates_images() {
        for (w, h, c, s, e) in [
            (0, 1, 1, 1, Error::Dimensions),
            (usize::MAX, 2, 3, 3, Error::Dimensions),
            (1, 1, 2, 2, Error::Channels),
            (2, 1, 3, 5, Error::Stride),
            (2, 2, 1, 2, Error::BufferLength),
        ] {
            assert_eq!(ImageView::new(&[0; 3], w, h, c, s).err(), Some(e));
        }
        assert!(ImageView::new(&[0; 8], 2, 2, 3, 2).is_err());
        assert!(ImageView::new(&[0; 10], 2, 2, 3, 4).is_err());
        assert!(ImageView::new(&[0; 14], 2, 2, 3, 8).is_ok());
    }
    #[test]
    fn rejects_degenerate_and_poles() {
        assert!(Transform::new([0.; 9]).is_err());
        assert!(Transform::new([f64::NAN; 9]).is_err());
        let im = ImageView::new(&[0; 4], 2, 2, 1, 2).unwrap();
        let mut s = Sampler::default();
        let m = Transform::new([1., 0., 0., 0., 1., 0., -2., 0., 1.]).unwrap();
        assert_eq!(
            s.sample(im, m, Path::default()).err(),
            Some(Error::Geometry)
        );
        // Endpoints alone are insufficient: the curved path has a pole inside.
        let curved = Transform::new([1., 0., 0., 0., 1., 0., 0., 3., 1.]).unwrap();
        assert_eq!(
            s.sample(
                im,
                curved,
                Path {
                    curve: -1.,
                    margin: 0.,
                    ..Path::default()
                }
            )
            .err(),
            Some(Error::Geometry)
        );
        let m = Transform::new([2., 0., 0., 0., 2., 0., 0., 0., 1.]).unwrap();
        assert_eq!(
            s.sample(
                im,
                m,
                Path {
                    axis: 2,
                    ..Path::default()
                }
            )
            .err(),
            Some(Error::Path)
        );
    }
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "This regression checks exact deterministic samples, discrete tags or unchanged geometry; an epsilon would hide a behavior change."
    )]
    fn stride_alpha_borders_and_reuse() {
        let mut s = Sampler::default();
        let m = Transform::new([2., 0., 0., 0., 2., 0., 0., 0., 1.]).unwrap();
        let gray = [0, 255, 77, 77, 0, 255];
        let a = s
            .sample(
                ImageView::new(&gray, 2, 2, 1, 4).unwrap(),
                m,
                Path::default(),
            )
            .unwrap()
            .unwrap()
            .to_owned();
        let rgba = [0, 0, 0, 3, 255, 255, 255, 7, 0, 0, 0, 255, 255, 255, 255, 0];
        let b = s
            .sample(
                ImageView::new(&rgba, 2, 2, 4, 8).unwrap(),
                m,
                Path::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(&a, b);
        assert_eq!(b[0], 1.);
        assert_eq!(b[511], 0.);
        assert!(s
            .sample(
                ImageView::new(&[120; 4], 2, 2, 1, 2).unwrap(),
                m,
                Path::default()
            )
            .unwrap()
            .is_none());
    }
}

#[cfg(test)]
mod luminance_table_tests {
    use super::*;
    #[test]
    fn every_rgb_color_is_bit_identical() {
        for r in 0..=255u8 {
            for g in 0..=255u8 {
                for b in 0..=255u8 {
                    let old = 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b);
                    assert_eq!(rgb_luminance(r, g, b).to_bits(), old.to_bits());
                }
            }
        }
    }
}

//! Reusable scalar bilinear crop warping from original-image pixels.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView, Transform};
pub const MAX_CROP_PIXELS: usize = 8_000_000;
#[derive(Default)]
pub struct Warper {
    output: Vec<u8>,
    output_len: usize,
}
impl Warper {
    /// Transform maps destination pixel coordinates to original-image coordinates.
    /// Border replication, half-pixel centers and byte rounding match warp.mjs.
    /// # Errors
    /// Returns `OutputShape` for invalid or oversized output and `Geometry` for projective poles or non-finite coordinates.
    pub fn warp(
        &mut self,
        image: ImageView<'_>,
        transform: Transform,
        width: usize,
        height: usize,
    ) -> Result<&[u8], Error> {
        self.warp_inner(image, transform, width, height)
    }
    /// Exact rounded-RGB warp followed by in-place luminance compaction. The
    /// allocation remains color-sized and reusable; callers receive one byte/pixel.
    /// (306*R + 601*G + 117*B + 512) >> 10 matches `ZXing` `RGBToLum`; alpha ignored.
    /// # Errors
    /// Returns `OutputShape` for invalid or oversized output and `Geometry` for projective poles or non-finite coordinates.
    pub fn warp_luminance(
        &mut self,
        image: ImageView<'_>,
        transform: Transform,
        width: usize,
        height: usize,
    ) -> Result<&[u8], Error> {
        self.warp_inner(image, transform, width, height)?;
        let pixels = width * height;
        if image.channels != 1 {
            for i in 0..pixels {
                let j = i * image.channels;
                let value = (306 * u32::from(self.output[j])
                    + 601 * u32::from(self.output[j + 1])
                    + 117 * u32::from(self.output[j + 2])
                    + 512)
                    >> 10;
                // i <= j, and all components are loaded before writing. Future
                // source pixels cannot be overwritten by prefix compaction.
                self.output[i] = (value).to_le_bytes()[0];
            }
        }
        self.output_len = pixels;
        Ok(&self.output[..self.output_len])
    }
    fn warp_inner(
        &mut self,
        image: ImageView<'_>,
        transform: Transform,
        width: usize,
        height: usize,
    ) -> Result<&[u8], Error> {
        let channels = image.channels;
        let pixels = width.checked_mul(height).ok_or(Error::OutputShape)?;
        if width == 0 || height == 0 || pixels > MAX_CROP_PIXELS {
            return Err(Error::OutputShape);
        }
        let len = pixels.checked_mul(channels).ok_or(Error::OutputShape)?;
        let m = transform.0;
        // A linear projective denominator takes its extremes at the rectangle's
        // corners. Reject poles before allocating or touching output pixels.
        let mut negative = false;
        let mut positive = false;
        for (x, y) in [
            (0., 0.),
            (crate::numeric::usize_f64(width), 0.),
            (
                crate::numeric::usize_f64(width),
                crate::numeric::usize_f64(height),
            ),
            (0., crate::numeric::usize_f64(height)),
        ] {
            let z = m[6] * x + m[7] * y + m[8];
            let sx = (m[0] * x + m[1] * y + m[2]) / z;
            let sy = (m[3] * x + m[4] * y + m[5]) / z;
            if !z.is_finite() || z.abs() < 1e-9 || !sx.is_finite() || !sy.is_finite() {
                return Err(Error::Geometry);
            }
            negative |= z < 0.;
            positive |= z > 0.;
        }
        if negative && positive {
            return Err(Error::Geometry);
        }
        if len > self.output.len() {
            self.output
                .try_reserve(len - self.output.len())
                .map_err(|_| Error::Allocation)?;
        }
        self.output.resize(len, 0);
        let cx = |x: f64| {
            crate::numeric::f64_usize(x.clamp(0., crate::numeric::usize_f64(image.width - 1)))
        };
        let cy = |y: f64| {
            crate::numeric::f64_usize(y.clamp(0., crate::numeric::usize_f64(image.height - 1)))
        };
        for y in 0..height {
            for x in 0..width {
                let dx = crate::numeric::usize_f64(x) + 0.5;
                let dy = crate::numeric::usize_f64(y) + 0.5;
                let z = m[6] * dx + m[7] * dy + m[8];
                let sx = (m[0] * dx + m[1] * dy + m[2]) / z - 0.5;
                let sy = (m[3] * dx + m[4] * dy + m[5]) / z - 0.5;
                if !sx.is_finite() || !sy.is_finite() {
                    return Err(Error::Geometry);
                }
                let x0 = sx.floor();
                let y0 = sy.floor();
                let fx = sx - x0;
                let fy = sy - y0;
                let i00 = cy(y0) * image.stride + cx(x0) * image.channels;
                let i01 = cy(y0) * image.stride + cx(x0 + 1.) * image.channels;
                let i10 = cy(y0 + 1.) * image.stride + cx(x0) * image.channels;
                let i11 = cy(y0 + 1.) * image.stride + cx(x0 + 1.) * image.channels;
                let value = |c: usize| {
                    let v00 = f64::from(image.data[i00 + c]);
                    let v01 = f64::from(image.data[i01 + c]);
                    let v10 = f64::from(image.data[i10 + c]);
                    let v11 = f64::from(image.data[i11 + c]);
                    let top = v00 + (v01 - v00) * fx;
                    let bot = v10 + (v11 - v10) * fx;
                    crate::numeric::f64_u8((top + (bot - top) * fy).round())
                };
                for c in 0..channels {
                    self.output[(y * width + x) * channels + c] = value(c);
                }
            }
        }
        self.output_len = len;
        Ok(&self.output[..self.output_len])
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.output[..self.output_len]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> Transform {
        Transform::new([1., 0., 0., 0., 1., 0., 0., 0., 1.]).unwrap()
    }
    #[test]
    fn identity_stride_channels_and_reuse() {
        let mut w = Warper::default();
        for c in [1usize, 3, 4] {
            let stride = 3 * c + 5;
            let bytes: Vec<u8> = (0..(stride * 3))
                .map(|i| (i * 37).to_le_bytes()[0])
                .collect();
            let image = ImageView::new(&bytes, 3, 3, c, stride).unwrap();
            let expected: Vec<u8> = (0..3)
                .flat_map(|y| bytes[y * stride..y * stride + 3 * c].iter().copied())
                .collect();
            assert_eq!(w.warp(image, identity(), 3, 3).unwrap(), expected);
            assert_eq!(w.warp(image, identity(), 1, 1).unwrap(), &bytes[..c]);
        }
    }
    #[test]
    fn interpolates_and_replicates_borders() {
        let im = ImageView::new(&[0, 100, 200, 240], 2, 2, 1, 2).unwrap();
        let mut w = Warper::default();
        let center = Transform::new([1., 0., 0.5, 0., 1., 0.5, 0., 0., 1.]).unwrap();
        assert_eq!(w.warp(im, center, 1, 1).unwrap(), [135]);
        let outside = Transform::new([1., 0., -100., 0., 1., -100., 0., 0., 1.]).unwrap();
        assert_eq!(w.warp(im, outside, 3, 3).unwrap(), [0; 9]);
    }
    #[test]
    fn rejects_sizes_and_projective_poles() {
        let im = ImageView::new(&[0], 1, 1, 1, 1).unwrap();
        let mut w = Warper::default();
        for (width, height) in [(0, 1), (usize::MAX, 2), (MAX_CROP_PIXELS + 1, 1)] {
            assert_eq!(
                w.warp(im, identity(), width, height).err(),
                Some(Error::OutputShape)
            );
        }
        let pole = Transform::new([1., 0., 0., 0., 1., 0., -1., 0., 1.]).unwrap();
        assert_eq!(w.warp(im, pole, 2, 2).err(), Some(Error::Geometry));
    }
}

#[cfg(test)]
mod luminance_tests {
    use super::*;
    #[test]
    fn fused_matches_independent_post_warp_luminance() {
        let mut color = Warper::default();
        let mut gray = Warper::default();
        for channels in [1usize, 3, 4] {
            for seed in 0..30 {
                let (w, h) = (17, 13);
                let stride = w * channels + 5;
                let p: Vec<u8> = (0..stride * h)
                    .map(|i| ((i * 97 + seed * 13) ^ (i * 3 + seed * 41)).to_le_bytes()[0])
                    .collect();
                let image = ImageView::new(&p, w, h, channels, stride).unwrap();
                let m = Transform::new([
                    0.9,
                    0.08,
                    crate::numeric::usize_f64(seed) * 0.1 - 2.,
                    -0.04,
                    1.1,
                    0.3,
                    0.002,
                    0.001,
                    1.,
                ])
                .unwrap();
                let rgb = color.warp(image, m, 21, 15).unwrap();
                let expected: Vec<u8> = rgb
                    .chunks(channels)
                    .map(|v| {
                        if channels == 1 {
                            v[0]
                        } else {
                            ((306 * u32::from(v[0])
                                + 601 * u32::from(v[1])
                                + 117 * u32::from(v[2])
                                + 512)
                                >> 10)
                                .to_le_bytes()[0]
                        }
                    })
                    .collect();
                assert_eq!(gray.warp_luminance(image, m, 21, 15).unwrap(), expected);
            }
        }
    }
    #[test]
    fn output_modes_reuse_and_gray_validation() {
        let p = [10, 50, 200, 0, 200, 40, 5, 255];
        let im = ImageView::new(&p, 2, 1, 4, 8).unwrap();
        let m = Transform::new([1., 0., 0., 0., 1., 0., 0., 0., 1.]).unwrap();
        let mut w = Warper::default();
        let owned = w.warp_luminance(im, m, 2, 1).unwrap().to_vec();
        assert_eq!(owned.len(), 2);
        assert_eq!(w.warp(im, m, 2, 1).unwrap(), p);
        assert_eq!(w.warp_luminance(im, m, 2, 1).unwrap(), owned);
        assert_eq!(
            w.warp_luminance(im, m, usize::MAX, 2).err(),
            Some(Error::OutputShape)
        );
        let pole = Transform::new([1., 0., 0., 0., 1., 0., -1., 0., 1.]).unwrap();
        assert_eq!(
            w.warp_luminance(im, pole, 2, 2).err(),
            Some(Error::Geometry)
        );
    }
}

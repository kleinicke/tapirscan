//! Fixed 2x bilinear upscale followed by sigma-3, amount-1.6 unsharp masking.
//! Byte truncation and replicated borders match the workbench JS reference.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView};

pub const MAX_ENHANCED_PIXELS: usize = 2_000_000;

#[derive(Default)]
pub struct UpscaleSharpen {
    scaled: Vec<u8>,
    horizontal: Vec<f64>,
    output: Vec<u8>,
}
fn resize<T: Clone>(v: &mut Vec<T>, n: usize, value: T) -> Result<(), Error> {
    if n > v.len() {
        v.try_reserve_exact(n - v.len())
            .map_err(|_| Error::Allocation)?;
    }
    v.resize(n, value);
    Ok(())
}
impl UpscaleSharpen {
    /// The returned packed image is 2w x 2h with unchanged channels. Borrowed
    /// output remains valid until the next mutable workspace operation.
    /// At most 2M output pixels and 80MB logical scratch (RGBA) are retained.
    pub fn process(&mut self, image: ImageView<'_>) -> Result<&[u8], Error> {
        let w = image.width.checked_mul(2).ok_or(Error::OutputShape)?;
        let h = image.height.checked_mul(2).ok_or(Error::OutputShape)?;
        let pixels = w.checked_mul(h).ok_or(Error::OutputShape)?;
        if pixels > MAX_ENHANCED_PIXELS {
            return Err(Error::OutputShape);
        }
        let c = image.channels;
        let n = pixels.checked_mul(c).ok_or(Error::OutputShape)?;
        resize(&mut self.scaled, n, 0)?;
        resize(&mut self.horizontal, n, 0.)?;
        resize(&mut self.output, n, 0)?;
        for y in 0..h {
            let sy = ((y as f64 + 0.5) * 0.5 - 0.5).min((image.height - 1) as f64);
            let y0 = sy.floor().max(0.) as usize;
            let y1 = (y0 + 1).min(image.height - 1);
            let fy = (sy - y0 as f64).max(0.);
            for x in 0..w {
                let sx = ((x as f64 + 0.5) * 0.5 - 0.5).min((image.width - 1) as f64);
                let x0 = sx.floor().max(0.) as usize;
                let x1 = (x0 + 1).min(image.width - 1);
                let fx = (sx - x0 as f64).max(0.);
                for channel in 0..c {
                    let p = |xx, yy| f64::from(image.data[yy * image.stride + xx * c + channel]);
                    let top = p(x0, y0) + (p(x1, y0) - p(x0, y0)) * fx;
                    let bottom = p(x0, y1) + (p(x1, y1) - p(x0, y1)) * fx;
                    self.scaled[(y * w + x) * c + channel] = (top + (bottom - top) * fy) as u8;
                }
            }
        }
        let mut weights = [0.; 19];
        let mut sum = 0.;
        for (i, weight) in weights.iter_mut().enumerate() {
            let k = i as f64 - 9.;
            *weight = (-k * k / 18.).exp();
            sum += *weight;
        }
        for weight in &mut weights {
            *weight /= sum;
        }
        for y in 0..h {
            for x in 0..w {
                for channel in 0..c {
                    let mut sum = 0.;
                    for (i, weight) in weights.iter().enumerate() {
                        let sx = (x as isize + i as isize - 9).clamp(0, w as isize - 1) as usize;
                        sum += f64::from(self.scaled[(y * w + sx) * c + channel]) * weight;
                    }
                    self.horizontal[(y * w + x) * c + channel] = sum;
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                for channel in 0..c {
                    let mut sum = 0.;
                    for (i, weight) in weights.iter().enumerate() {
                        let sy = (y as isize + i as isize - 9).clamp(0, h as isize - 1) as usize;
                        sum += self.horizontal[(sy * w + x) * c + channel] * weight;
                    }
                    let i = (y * w + x) * c + channel;
                    self.output[i] =
                        (1.6 * f64::from(self.scaled[i]) + (1. - 1.6) * f64::from(sum as u8)) as u8;
                }
            }
        }
        Ok(&self.output)
    }
    pub fn bytes(&self) -> &[u8] {
        &self.output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borders_channels_stride_and_reuse() {
        let mut workspace = UpscaleSharpen::default();
        // Match JS byte truncation (flat white becomes 254); padding is ignored.
        for c in [1, 3, 4] {
            for value in [0, 255] {
                let data = vec![value; 5 * c + 11];
                let image = ImageView::new(&data, 1, 2, c, c + 3).unwrap();
                let out = workspace.process(image).unwrap();
                assert_eq!(out, vec![if value == 255 { 254 } else { 0 }; 8 * c]);
            }
        }
        let data = [10, 20, 250, 30, 40];
        let a = workspace
            .process(ImageView::new(&data, 2, 2, 1, 3).unwrap())
            .unwrap()
            .to_vec();
        let b = workspace
            .process(ImageView::new(&[10, 20, 30, 40], 2, 2, 1, 2).unwrap())
            .unwrap();
        assert_eq!(a, b);
    }
    #[test]
    fn rejects_excessive_output_before_allocating() {
        let data = vec![0; 501 * 1000];
        let mut workspace = UpscaleSharpen::default();
        assert_eq!(
            workspace.process(ImageView::new(&data, 501, 1000, 1, 501).unwrap()),
            Err(Error::OutputShape)
        );
        assert!(workspace.bytes().is_empty());
    }
}

//! Reusable grayscale Gaussian adaptive thresholding, matching enhance.mjs.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView};
use crate::warp::MAX_CROP_PIXELS;

#[derive(Default)]
pub struct AdaptiveThreshold {
    gray: Vec<u8>,
    horizontal: Vec<f64>,
    output: Vec<u8>,
    weights: Vec<f64>,
    block: usize,
}
fn resize<T: Clone>(v: &mut Vec<T>, n: usize, value: T) -> Result<(), Error> {
    if n > v.len() {
        v.try_reserve(n - v.len()).map_err(|_| Error::Allocation)?;
    }
    v.resize(n, value);
    Ok(())
}
impl AdaptiveThreshold {
    /// RGB(A) luma is truncated before blur; replicated borders and the blurred
    /// mean's byte truncation deliberately match the existing JS algorithm.
    pub fn process(
        &mut self,
        image: ImageView<'_>,
        block: usize,
        offset: f64,
    ) -> Result<&[u8], Error> {
        if !(3..=101).contains(&block) || block.is_multiple_of(2) || !offset.is_finite() {
            return Err(Error::Parameters);
        }
        let n = image
            .width
            .checked_mul(image.height)
            .ok_or(Error::OutputShape)?;
        if n > MAX_CROP_PIXELS {
            return Err(Error::OutputShape);
        }
        resize(&mut self.gray, n, 0)?;
        resize(&mut self.horizontal, n, 0.)?;
        resize(&mut self.output, n, 0)?;
        let sigma = 0.3 * ((block as f64 - 1.) * 0.5 - 1.) + 0.8;
        let radius = (3. * sigma).ceil().max(1.) as isize;
        if self.block != block {
            resize(&mut self.weights, (radius * 2 + 1) as usize, 0.)?;
            let mut sum = 0.;
            for (i, weight) in self.weights.iter_mut().enumerate() {
                let k = i as isize - radius;
                *weight = (-(k * k) as f64 / (2. * sigma * sigma)).exp();
                sum += *weight;
            }
            for weight in &mut self.weights {
                *weight /= sum;
            }
            self.block = block;
        }
        let (w, h, c) = (image.width, image.height, image.channels);
        for y in 0..h {
            for x in 0..w {
                let i = y * image.stride + x * c;
                self.gray[y * w + x] = if c == 1 {
                    image.data[i]
                } else {
                    (0.299 * f64::from(image.data[i])
                        + 0.587 * f64::from(image.data[i + 1])
                        + 0.114 * f64::from(image.data[i + 2])) as u8
                };
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.;
                for (i, weight) in self.weights.iter().enumerate() {
                    let sx = (x as isize + i as isize - radius).clamp(0, w as isize - 1) as usize;
                    sum += f64::from(self.gray[y * w + sx]) * weight;
                }
                self.horizontal[y * w + x] = sum;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.;
                for (i, weight) in self.weights.iter().enumerate() {
                    let sy = (y as isize + i as isize - radius).clamp(0, h as isize - 1) as usize;
                    sum += self.horizontal[sy * w + x] * weight;
                }
                self.output[y * w + x] =
                    if f64::from(self.gray[y * w + x]) > f64::from(sum as u8) - offset {
                        255
                    } else {
                        0
                    };
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
    fn grayscale_matches_existing_scalar_kernel_across_shapes() {
        let mut p = AdaptiveThreshold::default();
        for (w, h) in [(1, 1), (9, 7), (31, 17), (4, 3)] {
            for block in [3, 15, 31, 51, 101] {
                let mut old = crate::Kernel::new(w, h).unwrap();
                for (i, v) in old.input.iter_mut().enumerate() {
                    *v = ((i * 73 + 19) % 256) as u8;
                }
                old.process(block, 5.25);
                let im = ImageView::new(&old.input, w, h, 1, w).unwrap();
                assert_eq!(p.process(im, block, 5.25).unwrap(), old.output);
            }
        }
    }
    #[test]
    fn color_stride_and_alpha_match_explicit_gray() {
        let rgb = [
            20, 80, 130, 0, 200, 150, 10, 255, 99, 99, 99, 99, 70, 40, 190, 3, 255, 4, 30, 5,
        ];
        let gray = [67, 149, 66, 82];
        let mut a = AdaptiveThreshold::default();
        let mut b = AdaptiveThreshold::default();
        assert_eq!(
            a.process(ImageView::new(&rgb, 2, 2, 4, 12).unwrap(), 3, 0.)
                .unwrap(),
            b.process(ImageView::new(&gray, 2, 2, 1, 2).unwrap(), 3, 0.)
                .unwrap()
        );
    }
    #[test]
    fn parameters_and_reuse() {
        let data = [255; 16];
        let im = ImageView::new(&data, 4, 4, 1, 4).unwrap();
        let mut p = AdaptiveThreshold::default();
        assert_eq!(p.process(im, 2, 0.), Err(Error::Parameters));
        assert_eq!(p.process(im, 31, f64::NAN), Err(Error::Parameters));
        assert!(p.process(im, 31, 5.).unwrap().iter().all(|v| *v == 255));
        let capacity = p.horizontal.capacity();
        p.process(ImageView::new(&data, 1, 1, 1, 1).unwrap(), 31, 5.)
            .unwrap();
        assert_eq!(p.horizontal.capacity(), capacity);
    }
}

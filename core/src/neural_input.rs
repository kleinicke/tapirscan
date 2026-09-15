//! Safe reusable letterboxed BGR/CHW input preparation, matching the existing
//! detector's f64 bilinear arithmetic followed by one f32 store. No inference.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub size: usize,
    pub scale: f64,
    pub pad_x: usize,
    pub pad_y: usize,
    pub width: usize,
    pub height: usize,
}
#[derive(Default)]
pub struct Input {
    data: Vec<f32>,
    columns: Vec<(usize, usize, f64)>,
}
impl Input {
    pub fn prepare(&mut self, im: ImageView<'_>, size: usize) -> Result<Layout, Error> {
        self.data.clear();
        if !(1..=2048).contains(&size) || !matches!(im.channels, 3 | 4) {
            return Err(Error::Parameters);
        }
        let scale = size as f64 / im.width.max(im.height) as f64;
        let width = (im.width as f64 * scale).round() as usize;
        let height = (im.height as f64 * scale).round() as usize;
        // Zero-sized content has no valid sampling transform.
        if width == 0 || height == 0 || width > size || height > size {
            return Err(Error::Dimensions);
        }
        let layout = Layout {
            size,
            scale,
            pad_x: (size - width) / 2,
            pad_y: (size - height) / 2,
            width,
            height,
        };
        let plane = size * size;
        let n = 3 * plane;
        self.data.try_reserve(n).map_err(|_| Error::Allocation)?;
        self.data.resize(n, (114_f64 / 255.) as f32);
        self.columns.clear();
        self.columns
            .try_reserve(width)
            .map_err(|_| Error::Allocation)?;
        let xstep = im.width as f64 / width as f64;
        let ystep = im.height as f64 / height as f64;
        for x in 0..width {
            let sx = ((x as f64 + 0.5) * xstep - 0.5).clamp(0., (im.width - 1) as f64);
            let x0 = sx.floor() as usize;
            self.columns.push((
                x0 * im.channels,
                (x0 + 1).min(im.width - 1) * im.channels,
                sx - x0 as f64,
            ));
        }
        for y in 0..height {
            let sy = ((y as f64 + 0.5) * ystep - 0.5).clamp(0., (im.height - 1) as f64);
            let y0 = sy.floor() as usize;
            let fy = sy - y0 as f64;
            let a = y0 * im.stride;
            let b = (y0 + 1).min(im.height - 1) * im.stride;
            for (x, &(x0, x1, fx)) in self.columns.iter().enumerate() {
                let dst = (layout.pad_y + y) * size + layout.pad_x + x;
                for c in 0..3 {
                    let sc = 2 - c;
                    let v00 = f64::from(im.data[a + x0 + sc]);
                    let v01 = f64::from(im.data[a + x1 + sc]);
                    let v10 = f64::from(im.data[b + x0 + sc]);
                    let v11 = f64::from(im.data[b + x1 + sc]);
                    let top = v00 + (v01 - v00) * fx;
                    let bottom = v10 + (v11 - v10) * fx;
                    self.data[c * plane + dst] = ((top + (bottom - top) * fy) / 255.) as f32;
                }
            }
        }
        Ok(layout)
    }
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn color_padding_stride_and_reuse() {
        let mut p = Input::default();
        let rgb = [255, 0, 0, 91, 92, 0, 255, 0];
        let im = ImageView::new(&rgb, 1, 2, 3, 5).unwrap();
        let l = p.prepare(im, 2).unwrap();
        assert_eq!((l.width, l.height, l.pad_x, l.pad_y), (1, 2, 0, 0));
        let pad = (114_f64 / 255.) as f32;
        assert_eq!(
            p.data(),
            &[0., pad, 0., pad, 0., pad, 1., pad, 1., pad, 0., pad]
        );
        let cap = p.data.capacity();
        let rgba = [0, 0, 255, 0];
        p.prepare(ImageView::new(&rgba, 1, 1, 4, 4).unwrap(), 1)
            .unwrap();
        assert_eq!(p.data(), &[1., 0., 0.]);
        assert_eq!(p.data.capacity(), cap);
    }
    #[test]
    fn bilinear_and_invalid_clear() {
        let mut p = Input::default();
        let pixels = [0, 0, 0, 255, 255, 255];
        let im = ImageView::new(&pixels, 2, 1, 3, 6).unwrap();
        p.prepare(im, 3).unwrap();
        assert_eq!(p.data()[1], 0.5);
        assert!(p.prepare(im, 2049).is_err());
        assert!(p.data().is_empty());
        let gray = ImageView::new(&[0], 1, 1, 1, 1).unwrap();
        assert!(p.prepare(gray, 2).is_err());
        let tall = vec![0; 9000];
        assert!(p
            .prepare(ImageView::new(&tall, 1, 3000, 3, 3).unwrap(), 1)
            .is_err());
    }
}

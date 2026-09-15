//! Reusable, safe 2x box reduction. Odd right/bottom edges replicate the last
//! pixel: no source row or column is dropped. Output centers map to 2*p + .5
//! in the padded original coordinate system (padding is at most one pixel).
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView};
#[derive(Default)]
pub struct HalfImage {
    data: Vec<u8>,
    width: usize,
    height: usize,
}
impl HalfImage {
    pub fn reduce(&mut self, im: ImageView<'_>) -> Result<&[u8], Error> {
        let w = im.width.div_ceil(2);
        let h = im.height.div_ceil(2);
        let n = w.checked_mul(h).ok_or(Error::Dimensions)?;
        self.data
            .try_reserve(n.saturating_sub(self.data.len()))
            .map_err(|_| Error::Allocation)?;
        self.data.resize(n, 0);
        let gray = |x: usize, y: usize| -> u32 {
            let i = y * im.stride + x * im.channels;
            if im.channels == 1 {
                u32::from(im.data[i])
            } else {
                (306 * u32::from(im.data[i])
                    + 601 * u32::from(im.data[i + 1])
                    + 117 * u32::from(im.data[i + 2])
                    + 512)
                    >> 10
            }
        };
        for y in 0..h {
            for x in 0..w {
                let x0 = x * 2;
                let x1 = (x0 + 1).min(im.width - 1);
                let y0 = y * 2;
                let y1 = (y0 + 1).min(im.height - 1);
                self.data[y * w + x] =
                    ((gray(x0, y0) + gray(x1, y0) + gray(x0, y1) + gray(x1, y1) + 2) / 4) as u8;
            }
        }
        self.width = w;
        self.height = h;
        Ok(&self.data)
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn odd_edges_and_reuse() {
        let mut p = HalfImage::default();
        let a = [0, 10, 20, 30, 40, 50, 60, 70, 80];
        assert_eq!(
            p.reduce(ImageView::new(&a, 3, 3, 1, 3).unwrap()).unwrap(),
            &[20, 35, 65, 80]
        );
        assert_eq!(p.dimensions(), (2, 2));
        assert_eq!(
            p.reduce(ImageView::new(&[173], 1, 1, 1, 1).unwrap())
                .unwrap(),
            &[173]
        );
        assert_eq!(p.dimensions(), (1, 1));
    }
    #[test]
    fn color_stride_alpha_and_thin_images() {
        let mut p = HalfImage::default();
        let rgb = [255, 0, 0, 99, 99, 0, 255, 0];
        assert_eq!(
            p.reduce(ImageView::new(&rgb, 1, 2, 3, 5).unwrap()).unwrap(),
            &[113]
        );
        let rgba = [255, 0, 0, 0, 0, 255, 0, 255];
        assert_eq!(
            p.reduce(ImageView::new(&rgba, 2, 1, 4, 8).unwrap())
                .unwrap(),
            &[113]
        );
        assert!(ImageView::new(&rgba, 3, 1, 4, 12).is_err());
    }
}

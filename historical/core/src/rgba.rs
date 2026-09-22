//! Owned reusable RGBA preparation for image-data decoder adapters.
//! No resampling, rounding, grayscale transform, geometry or decoder decisions.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView, MAX_IMAGE_BYTES};

#[derive(Default)]
pub struct RgbaImage {
    data: Vec<u8>,
    width: usize,
    height: usize,
}
fn output_len(width: usize, height: usize) -> Result<usize, Error> {
    let n = width
        .checked_mul(height)
        .and_then(|n| n.checked_mul(4))
        .ok_or(Error::OutputShape)?;
    if width == 0 || height == 0 || n > MAX_IMAGE_BYTES {
        return Err(Error::OutputShape);
    }
    Ok(n)
}
impl RgbaImage {
    /// Returns owned workspace bytes borrowed until the next mutable call.
    /// RGB/gray use opaque alpha; RGBA preserves alpha exactly. Padding is skipped.
    /// Output has an independent128MiB cap. Failure invalidates previous output.
    /// # Errors
    /// Returns `OutputShape` for invalid/oversized output and `Allocation` if RGBA storage cannot be reserved.
    pub fn prepare(&mut self, im: ImageView<'_>) -> Result<&[u8], Error> {
        self.width = 0;
        self.height = 0;
        let n = match output_len(im.width, im.height) {
            Ok(n) => n,
            Err(error) => {
                self.data.clear();
                return Err(error);
            }
        };
        if self
            .data
            .try_reserve(n.saturating_sub(self.data.len()))
            .is_err()
        {
            self.data.clear();
            return Err(Error::Allocation);
        }
        // Existing bytes are overwritten below; only newly grown bytes need init.
        self.data.resize(n, 0);
        if im.channels == 4 && im.stride == im.width * 4 {
            self.data.copy_from_slice(&im.data[..n]);
        } else {
            for (y, out) in self.data.chunks_exact_mut(im.width * 4).enumerate() {
                let start = y * im.stride;
                let row = &im.data[start..start + im.width * im.channels];
                match im.channels {
                    4 => out.copy_from_slice(row),
                    3 => {
                        for (p, q) in row.chunks_exact(3).zip(out.chunks_exact_mut(4)) {
                            q.copy_from_slice(&[p[0], p[1], p[2], 255]);
                        }
                    }
                    1 => {
                        for (&p, q) in row.iter().zip(out.chunks_exact_mut(4)) {
                            q.copy_from_slice(&[p, p, p, 255]);
                        }
                    }
                    _ => unreachable!("ImageView validates channel count"),
                }
            }
        }
        self.width = im.width;
        self.height = im.height;
        Ok(&self.data)
    }
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    #[must_use]
    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_channels_strides_and_alpha() {
        let mut out = RgbaImage::default();
        for channels in [1usize, 3, 4] {
            for width in [1, 3, 17] {
                for height in [1, 2, 5] {
                    for pad in [0, 1, 7] {
                        let stride = width * channels + pad;
                        let n = (height - 1) * stride + width * channels;
                        let src: Vec<u8> = (0..n).map(|i| (i * 71 + 29).to_le_bytes()[0]).collect();
                        let mut expected = Vec::new();
                        for y in 0..height {
                            for x in 0..width {
                                let p = y * stride + x * channels;
                                expected.extend_from_slice(&[
                                    src[p],
                                    src[p + usize::from(channels != 1)],
                                    src[p + if channels == 1 { 0 } else { 2 }],
                                    if channels == 4 { src[p + 3] } else { 255 },
                                ]);
                            }
                        }
                        assert_eq!(
                            out.prepare(
                                ImageView::new(&src, width, height, channels, stride).unwrap()
                            )
                            .unwrap(),
                            expected
                        );
                        assert_eq!(out.dimensions(), (width, height));
                    }
                }
            }
        }
    }
    #[test]
    fn reuse_and_output_bounds() {
        let mut out = RgbaImage::default();
        let input = [17; 120];
        out.prepare(ImageView::new(&input, 10, 4, 3, 30).unwrap())
            .unwrap();
        let capacity = out.data.capacity();
        out.prepare(ImageView::new(&[2, 3, 4, 7], 1, 1, 4, 4).unwrap())
            .unwrap();
        assert_eq!(out.data(), [2, 3, 4, 7]);
        assert_eq!(out.data.capacity(), capacity);
        assert_eq!(output_len(usize::MAX, 2), Err(Error::OutputShape));
        assert_eq!(output_len(0, 1), Err(Error::OutputShape));
        assert_eq!(
            output_len(MAX_IMAGE_BYTES / 4 + 1, 1),
            Err(Error::OutputShape)
        );
        // The source can fit its own cap while its expanded output exceeds it.
        let src = vec![0; MAX_IMAGE_BYTES / 4 + 1];
        assert_eq!(
            out.prepare(ImageView::new(&src, src.len(), 1, 1, src.len()).unwrap()),
            Err(Error::OutputShape)
        );
        assert!(out.data().is_empty());
        assert_eq!(out.dimensions(), (0, 0));
        assert!(ImageView::new(&[1, 2], 1, 1, 3, 3).is_err());
    }
}

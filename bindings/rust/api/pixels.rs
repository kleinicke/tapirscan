use crate::Error;

/// Borrowed 8-bit pixels. RGB/RGBA are in RGB order; alpha is ignored.
/// Rows are packed unless [`Self::with_stride`] supplies a byte stride.
/// Validation happens when scanning, before pixels are read or copied.
#[derive(Clone, Copy, Debug)]
pub struct Image<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) channels: usize,
    pub(crate) stride: usize,
}
impl<'a> Image<'a> {
    /// Borrow a grayscale image with one byte per pixel.
    ///
    /// `data` contains row-major luminance bytes (0 black, 255 white). `width`
    /// and `height` are pixel counts. Rows initially have `width` bytes.
    /// Construction does not validate or copy pixels; scanning validates them.
    #[must_use]
    pub fn gray(data: &'a [u8], width: usize, height: usize) -> Self {
        Self {
            data,
            width,
            height,
            channels: 1,
            stride: width,
        }
    }
    /// Borrow an RGB image with three bytes per pixel, in red/green/blue order.
    ///
    /// `data` contains interleaved row-major pixels. `width` and `height` are
    /// pixel counts; rows initially have `width * 3` bytes. Construction does
    /// not validate or copy pixels; scanning validates them.
    #[must_use]
    pub fn rgb(data: &'a [u8], width: usize, height: usize) -> Self {
        Self {
            channels: 3,
            stride: width.saturating_mul(3),
            ..Self::gray(data, width, height)
        }
    }
    /// Borrow an RGBA image with four bytes per pixel, in red/green/blue/alpha order.
    ///
    /// `data` contains interleaved row-major pixels. `width` and `height` are
    /// pixel counts; rows initially have `width * 4` bytes. Alpha is ignored,
    /// including zero alpha; composite transparent images yourself if needed.
    /// Construction does not validate or copy pixels; scanning validates them.
    #[must_use]
    pub fn rgba(data: &'a [u8], width: usize, height: usize) -> Self {
        Self {
            channels: 4,
            stride: width.saturating_mul(4),
            ..Self::gray(data, width, height)
        }
    }
    /// Set `stride`, the number of bytes between consecutive row starts.
    ///
    /// It must be at least `width * channels`. The buffer must address
    /// `(height - 1) * stride + width * channels` bytes; last-row trailing padding
    /// is optional. Invalid values are rejected when scanning.
    #[must_use]
    pub fn with_stride(mut self, stride: usize) -> Self {
        self.stride = stride;
        self
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        if self.width < 3
            || self.height < 3
            || self
                .width
                .checked_mul(self.height)
                .is_none_or(|n| n > 32 * 1024 * 1024)
        {
            return Err(Error::InvalidImage(
                "dimensions must be at least 3×3 and at most 32 megapixels",
            ));
        }
        let row = self
            .width
            .checked_mul(self.channels)
            .ok_or(Error::InvalidImage("row size overflow"))?;
        if self.stride < row {
            return Err(Error::InvalidImage("stride is smaller than a pixel row"));
        }
        let size = (self.height - 1)
            .checked_mul(self.stride)
            .and_then(|n| n.checked_add(row))
            .ok_or(Error::InvalidImage("buffer size overflow"))?;
        if size > 128 * 1024 * 1024 || self.data.len() < size {
            return Err(Error::InvalidImage(
                "buffer is too short or exceeds the 128 MiB layout limit",
            ));
        }
        Ok(())
    }
}

#[cfg(feature = "image")]
macro_rules! image_buffer {
    ($name:ident, $constructor:ident) => {
        impl<'a> From<&'a image::$name> for Image<'a> {
            fn from(image: &'a image::$name) -> Self {
                Self::$constructor(
                    image.as_raw(),
                    image.width() as usize,
                    image.height() as usize,
                )
            }
        }
    };
}
#[cfg(feature = "image")]
image_buffer!(GrayImage, gray);
#[cfg(feature = "image")]
image_buffer!(RgbImage, rgb);
#[cfg(feature = "image")]
image_buffer!(RgbaImage, rgba);

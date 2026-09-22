//! Production scanner kernels. Select one effort mode at compile time.
//! Algorithm edits belong in this tree; historical recipes are never applied here.
#[cfg(not(any(
    feature = "mode-low",
    feature = "mode-medium",
    feature = "mode-high",
    feature = "mode-very-high"
)))]
compile_error!("Select one core mode; use mode-medium for the default policy.");
#[cfg(any(
    all(feature = "mode-low", feature = "mode-medium"),
    all(feature = "mode-low", feature = "mode-high"),
    all(feature = "mode-low", feature = "mode-very-high"),
    all(feature = "mode-medium", feature = "mode-high"),
    all(feature = "mode-medium", feature = "mode-very-high"),
    all(feature = "mode-high", feature = "mode-very-high")
))]
compile_error!(
    "Core modes are mutually exclusive; pass --no-default-features when selecting a mode."
);
#[doc(hidden)]
pub mod numeric;
pub struct Kernel {
    width: usize,
    height: usize,
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    tmp: Vec<f64>,
}
impl Kernel {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Option<Self> {
        let len = width.checked_mul(height)?;
        if len == 0 || len > 8_000_000 {
            return None;
        }
        Some(Self {
            width,
            height,
            input: vec![0; len],
            output: vec![0; len],
            tmp: vec![0.; len],
        })
    }
    pub fn process(&mut self, block: usize, offset: f64) -> bool {
        if !(3..=101).contains(&block) || block.is_multiple_of(2) || !offset.is_finite() {
            return false;
        }
        let sigma = 0.3 * ((crate::numeric::usize_f64(block) - 1.) * 0.5 - 1.) + 0.8;
        let radius = crate::numeric::f64_isize((3. * sigma).ceil().max(1.));
        let mut weights: Vec<f64> = (-radius..=radius)
            .map(|k| (crate::numeric::isize_f64(-(k * k)) / (2. * sigma * sigma)).exp())
            .collect();
        let sum: f64 = weights.iter().sum();
        for w in &mut weights {
            *w /= sum;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let mut acc = 0.;
                for (i, w) in weights.iter().enumerate() {
                    let sx = (((x).cast_signed() + (i).cast_signed() - radius)
                        .clamp(0, (self.width).cast_signed() - 1))
                    .cast_unsigned();
                    acc += f64::from(self.input[y * self.width + sx]) * w;
                }
                self.tmp[y * self.width + x] = acc;
            }
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let mut acc = 0.;
                for (i, w) in weights.iter().enumerate() {
                    let sy = (((y).cast_signed() + (i).cast_signed() - radius)
                        .clamp(0, (self.height).cast_signed() - 1))
                    .cast_unsigned();
                    acc += self.tmp[sy * self.width + x] * w;
                }
                // Match existing JS byte truncation before thresholding.
                self.output[y * self.width + x] = if f64::from(self.input[y * self.width + x])
                    > f64::from(crate::numeric::f64_u8(acc)) - offset
                {
                    255
                } else {
                    0
                };
            }
        }
        true
    }
}
#[no_mangle]
pub extern "C" fn kernel_new(width: usize, height: usize) -> *mut Kernel {
    Kernel::new(width, height).map_or(std::ptr::null_mut(), |k| Box::into_raw(Box::new(k)))
}
/// Return the writable image buffer.
///
/// # Safety
/// `k` must be a live handle returned by `kernel_new`, with exclusive access.
/// The returned buffer has the creation width times height bytes and is valid
/// only until destruction. Do not access it during another call on the handle.
#[no_mangle]
pub unsafe extern "C" fn kernel_input(k: *mut Kernel) -> *mut u8 {
    (*k).input.as_mut_ptr()
}
/// Return the thresholded image buffer.
///
/// # Safety
/// `k` must be a live handle returned by `kernel_new`. Read at most the creation
/// width times height bytes. Do not retain a borrowed view across processing
/// or destruction, or access the handle concurrently.
#[no_mangle]
pub unsafe extern "C" fn kernel_output(k: *mut Kernel) -> *const u8 {
    (*k).output.as_ptr()
}
/// Apply the Gaussian threshold to the input buffer.
///
/// # Safety
/// `k` must be a live handle returned by `kernel_new`, with exclusive access
/// and no outstanding references to its buffers during this call.
#[no_mangle]
pub unsafe extern "C" fn kernel_process(k: *mut Kernel, block: usize, offset: f64) -> bool {
    (*k).process(block, offset)
}
/// Release a kernel handle; a null handle is ignored.
///
/// # Safety
/// A non-null `k` must have been returned by `kernel_new` and not yet destroyed.
/// All access to the handle and its buffers must have ended before this call.
#[no_mangle]
pub unsafe extern "C" fn kernel_destroy(k: *mut Kernel) {
    if !k.is_null() {
        drop(Box::from_raw(k));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_and_parameters_are_bounded() {
        assert!(Kernel::new(0, 1).is_none());
        assert!(Kernel::new(usize::MAX, 2).is_none());
        let mut k = Kernel::new(1, 1).unwrap();
        assert!(!k.process(2, 5.));
        assert!(!k.process(31, f64::NAN));
    }
    #[test]
    fn uniform_white_and_black_have_no_edges() {
        for value in [0, 255] {
            let mut k = Kernel::new(9, 7).unwrap();
            k.input.fill(value);
            assert!(k.process(31, 5.));
            assert!(k.output.iter().all(|v| *v == 255));
        }
    }
}
pub mod ean;
impl Kernel {
    /// Linear-time local mean alternative; algorithmically different from Gaussian.
    pub fn box_threshold(&mut self, block: usize, offset: f64) -> bool {
        if !(3..=101).contains(&block) || block.is_multiple_of(2) || !offset.is_finite() {
            return false;
        }
        let r = (block / 2).cast_signed();
        let width = self.width;
        let height = self.height;
        for y in 0..height {
            let mut sum = 0.;
            for k in -r..=r {
                sum += f64::from(
                    self.input[y * width + (k.clamp(0, (width).cast_signed() - 1)).cast_unsigned()],
                );
            }
            for x in 0..width {
                self.tmp[y * width + x] = sum / crate::numeric::usize_f64(block);
                let left =
                    (((x).cast_signed() - r).clamp(0, (width).cast_signed() - 1)).cast_unsigned();
                let right = (((x).cast_signed() + r + 1).clamp(0, (width).cast_signed() - 1))
                    .cast_unsigned();
                sum += f64::from(self.input[y * width + right])
                    - f64::from(self.input[y * width + left]);
            }
        }
        for x in 0..width {
            let mut sum = 0.;
            for k in -r..=r {
                sum +=
                    self.tmp[(k.clamp(0, (height).cast_signed() - 1)).cast_unsigned() * width + x];
            }
            for y in 0..height {
                self.output[y * width + x] = if f64::from(self.input[y * width + x])
                    > sum / crate::numeric::usize_f64(block) - offset
                {
                    255
                } else {
                    0
                };
                let top =
                    (((y).cast_signed() - r).clamp(0, (height).cast_signed() - 1)).cast_unsigned();
                let bot = (((y).cast_signed() + r + 1).clamp(0, (height).cast_signed() - 1))
                    .cast_unsigned();
                sum += self.tmp[bot * width + x] - self.tmp[top * width + x];
            }
        }
        true
    }
}
/// Apply the local mean threshold to the input buffer.
///
/// # Safety
/// `k` must be a live handle returned by `kernel_new`, with exclusive access
/// and no outstanding references to its buffers during this call.
#[no_mangle]
pub unsafe extern "C" fn kernel_box(k: *mut Kernel, block: usize, offset: f64) -> bool {
    (*k).box_threshold(block, offset)
}
pub struct EanSession {
    pub probabilities: [f32; 95],
    pub digits: [u8; 13],
    pub cost: f32,
    pub gap: f32,
}
#[no_mangle]
pub extern "C" fn ean_new() -> *mut EanSession {
    Box::into_raw(Box::new(EanSession {
        probabilities: [0.; 95],
        digits: [0; 13],
        cost: 1.,
        gap: 0.,
    }))
}
/// Return the writable 95-element module probability buffer.
///
/// # Safety
/// `k` must be a live handle returned by `ean_new`, with exclusive access.
/// The returned buffer is valid until destruction; do not access it during
/// another call on the handle or read or write beyond its 95 elements.
#[no_mangle]
pub unsafe extern "C" fn ean_input(k: *mut EanSession) -> *mut f32 {
    (*k).probabilities.as_mut_ptr()
}
/// Return the 13-byte digit buffer.
///
/// # Safety
/// `k` must be a live handle returned by `ean_new`. Read at most 13 bytes,
/// and do not retain a borrowed view across decoding or destruction.
/// The handle must not be accessed concurrently.
#[no_mangle]
pub unsafe extern "C" fn ean_output(k: *mut EanSession) -> *const u8 {
    (*k).digits.as_ptr()
}
/// Decode the module probabilities into the digit buffer.
///
/// # Safety
/// `k` must be a live handle returned by `ean_new`, with exclusive access
/// and no outstanding references to its buffers during this call.
#[no_mangle]
pub unsafe extern "C" fn ean_decode(k: *mut EanSession, max_cost: f32, min_gap: f32) -> bool {
    if let Some(r) = ean::decode(&(*k).probabilities, max_cost, min_gap) {
        (*k).digits = r.digits;
        (*k).cost = r.cost;
        (*k).gap = r.gap;
        true
    } else {
        false
    }
}
/// Release an EAN session; a null handle is ignored.
///
/// # Safety
/// A non-null `k` must have been returned by `ean_new` and not yet destroyed.
/// All access to the handle and its buffers must have ended before this call.
#[no_mangle]
pub unsafe extern "C" fn ean_destroy(k: *mut EanSession) {
    if !k.is_null() {
        drop(Box::from_raw(k));
    }
}
#[cfg(test)]
mod box_tests {
    use super::*;
    #[test]
    fn running_mean_matches_naive_two_dimensional_reference() {
        for (w, h) in [(1, 1), (3, 7), (13, 5)] {
            for block in [3, 5, 11] {
                let mut k = Kernel::new(w, h).unwrap();
                for (i, v) in k.input.iter_mut().enumerate() {
                    *v = ((i * 73 + 19) % 256).to_le_bytes()[0];
                }
                let input = k.input.clone();
                assert!(k.box_threshold(block, 5.25));
                let r = (block / 2).cast_signed();
                for y in 0..h {
                    for x in 0..w {
                        let mut sum = 0usize;
                        for dy in -r..=r {
                            for dx in -r..=r {
                                let sx = (((x).cast_signed() + dx).clamp(0, (w).cast_signed() - 1))
                                    .cast_unsigned();
                                let sy = (((y).cast_signed() + dy).clamp(0, (h).cast_signed() - 1))
                                    .cast_unsigned();
                                sum += input[sy * w + sx] as usize;
                            }
                        }
                        let reference = if f64::from(input[y * w + x])
                            > crate::numeric::usize_f64(sum)
                                / crate::numeric::usize_f64(block * block)
                                - 5.25
                        {
                            255
                        } else {
                            0
                        };
                        assert_eq!(k.output[y * w + x], reference);
                    }
                }
            }
        }
    }
}
pub mod association;
pub mod band_association;
pub mod bands;
pub mod continuity;
pub mod contrast;
pub mod enhance;
pub mod localize;
pub mod neural_input;
pub mod oriented;
pub mod preprocess;
pub mod profile;
pub mod pyramid;
pub mod rgba;
pub mod row_group;
pub mod row_scan;
pub mod run_continuity;
pub mod run_ean;
pub mod run_profile;
pub mod sampling;
mod sampling_abi;
pub mod scan;
pub mod warp;

// Independent find-all scanner. Existing research ABI remains separate.
pub mod experiment;
pub mod frame;
pub mod identity;

pub(crate) mod invalid_visual;
pub mod local_signal;
pub mod multi_profile;
pub mod multi_scan;
pub mod orientation;
pub mod region_abi;
pub mod region_json;
pub mod region_scan;
#[cfg(any(feature = "mode-low", feature = "mode-medium"))]
pub(crate) mod retail_pipeline;
mod scanner_clock;
pub mod shear;

pub mod stripes;
pub mod transition;

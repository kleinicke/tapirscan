//! Exact f64 bilinear crop sampling shared with the JavaScript retail host.
//! A retained source lets successive crops reuse one uploaded grayscale frame.
#[derive(Default)]
pub(crate) struct Source {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
    pub transform: [f64; 8],
}
impl Source {
    pub fn capture(&mut self, pixels: &[u8], width: usize, height: usize) -> bool {
        if width == 0 || height == 0 || width.checked_mul(height) != Some(pixels.len()) {
            self.pixels.clear();
            self.width = 0;
            self.height = 0;
            return false;
        }
        self.pixels.clear();
        self.pixels.extend_from_slice(pixels);
        self.width = width;
        self.height = height;
        true
    }

    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Finite in-bounds coordinates are floored before indexing; rounded bilinear bytes use the original Uint8Array semantics, including NaN-to-zero."
    )]
    fn pixel_at(&self, source_x: f64, source_y: f64, max_x: f64, max_y: f64) -> u8 {
        if source_x < 0. || source_y < 0. || source_x > max_x || source_y > max_y {
            return 255;
        }
        if source_x.is_nan() || source_y.is_nan() {
            return 0;
        }
        let x0 = source_x.floor() as usize;
        let y0 = source_y.floor() as usize;
        let fx = source_x - crate::numeric::usize_f64(x0);
        let fy = source_y - crate::numeric::usize_f64(y0);
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let top_left = f64::from(self.pixels[y0 * self.width + x0]);
        let top_right = f64::from(self.pixels[y0 * self.width + x1]);
        let bottom_left = f64::from(self.pixels[y1 * self.width + x0]);
        let bottom_right = f64::from(self.pixels[y1 * self.width + x1]);
        ((top_left * (1. - fx) + top_right * fx) * (1. - fy)
            + (bottom_left * (1. - fx) + bottom_right * fx) * fy)
            .round() as u8
    }

    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "The two finite in-bounds coordinates are floored before indexing; final rounded bytes preserve scalar sampling semantics."
    )]
    fn pixel_pair(
        &self,
        source_x: core::arch::wasm32::v128,
        source_y: core::arch::wasm32::v128,
        max_x: f64,
        max_y: f64,
    ) -> [u8; 2] {
        use core::arch::wasm32::{
            f64x2, f64x2_add, f64x2_extract_lane, f64x2_floor, f64x2_ge, f64x2_mul, f64x2_splat,
            f64x2_sub, i32x4_extract_lane, i32x4_trunc_sat_f64x2_zero, v128_bitselect,
        };
        let xs = [
            f64x2_extract_lane::<0>(source_x),
            f64x2_extract_lane::<1>(source_x),
        ];
        let ys = [
            f64x2_extract_lane::<0>(source_y),
            f64x2_extract_lane::<1>(source_y),
        ];
        if !(0..2).all(|i| xs[i] >= 0. && xs[i] <= max_x && ys[i] >= 0. && ys[i] <= max_y) {
            return [
                self.pixel_at(xs[0], ys[0], max_x, max_y),
                self.pixel_at(xs[1], ys[1], max_x, max_y),
            ];
        }
        let floor_x = f64x2_floor(source_x);
        let floor_y = f64x2_floor(source_y);
        let left = [
            f64x2_extract_lane::<0>(floor_x) as usize,
            f64x2_extract_lane::<1>(floor_x) as usize,
        ];
        let top = [
            f64x2_extract_lane::<0>(floor_y) as usize,
            f64x2_extract_lane::<1>(floor_y) as usize,
        ];
        let right = left.map(|x| (x + 1).min(self.width - 1));
        let bottom = top.map(|y| (y + 1).min(self.height - 1));
        let top = top.map(|y| y * self.width);
        let bottom = bottom.map(|y| y * self.width);
        let read = |rows: [usize; 2], columns: [usize; 2]| {
            f64x2(
                f64::from(self.pixels[rows[0] + columns[0]]),
                f64::from(self.pixels[rows[1] + columns[1]]),
            )
        };
        let fx = f64x2_sub(source_x, floor_x);
        let fy = f64x2_sub(source_y, floor_y);
        let ix = f64x2_sub(f64x2_splat(1.), fx);
        let iy = f64x2_sub(f64x2_splat(1.), fy);
        let upper = f64x2_add(
            f64x2_mul(read(top, left), ix),
            f64x2_mul(read(top, right), fx),
        );
        let lower = f64x2_add(
            f64x2_mul(read(bottom, left), ix),
            f64x2_mul(read(bottom, right), fx),
        );
        let result = f64x2_add(f64x2_mul(upper, iy), f64x2_mul(lower, fy));
        // Interpolated bytes are nonnegative. Compare the exact fractional part
        // to one half: floor(value + 0.5) can round a just-below-half value early.
        let lower = f64x2_floor(result);
        let increment = v128_bitselect(
            f64x2_splat(1.),
            f64x2_splat(0.),
            f64x2_ge(f64x2_sub(result, lower), f64x2_splat(0.5)),
        );
        let rounded = i32x4_trunc_sat_f64x2_zero(f64x2_add(lower, increment));
        [
            i32x4_extract_lane::<0>(rounded).to_le_bytes()[0],
            i32x4_extract_lane::<1>(rounded).to_le_bytes()[0],
        ]
    }

    pub fn sample(&self, output: &mut Vec<u8>, width: usize, height: usize) -> bool {
        let Some(count) = width.checked_mul(height) else {
            return false;
        };
        if width == 0
            || height == 0
            || count > 32 * 1024 * 1024
            || self.pixels.is_empty()
            || self.transform.iter().any(|v| !v.is_finite())
        {
            return false;
        }
        output.resize(count, 0);
        let transform = self.transform;
        let max_x = crate::numeric::usize_f64(self.width - 1);
        let max_y = crate::numeric::usize_f64(self.height - 1);
        for (y, output_row) in output.chunks_exact_mut(width).enumerate() {
            let cy = crate::numeric::usize_f64(y) + 0.5;
            let yx = transform[1] * cy;
            let yy = transform[4] * cy;
            let yz = transform[7] * cy;
            #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
            {
                use core::arch::wasm32::{f64x2, f64x2_add, f64x2_div, f64x2_mul, f64x2_splat};
                let mut pairs = output_row.chunks_exact_mut(2);
                for (pair_index, pair) in pairs.by_ref().enumerate() {
                    let start = crate::numeric::usize_f64(pair_index * 2) + 0.5;
                    let positions = f64x2(start, start + 1.);
                    let denominator = f64x2_add(
                        f64x2_add(
                            f64x2_mul(f64x2_splat(transform[6]), positions),
                            f64x2_splat(yz),
                        ),
                        f64x2_splat(1.),
                    );
                    let source_x = f64x2_div(
                        f64x2_add(
                            f64x2_add(
                                f64x2_mul(f64x2_splat(transform[0]), positions),
                                f64x2_splat(yx),
                            ),
                            f64x2_splat(transform[2]),
                        ),
                        denominator,
                    );
                    let source_y = f64x2_div(
                        f64x2_add(
                            f64x2_add(
                                f64x2_mul(f64x2_splat(transform[3]), positions),
                                f64x2_splat(yy),
                            ),
                            f64x2_splat(transform[5]),
                        ),
                        denominator,
                    );
                    pair.copy_from_slice(&self.pixel_pair(source_x, source_y, max_x, max_y));
                }
                if let Some(target) = pairs.into_remainder().first_mut() {
                    let cx = crate::numeric::usize_f64(width - 1) + 0.5;
                    let denominator = transform[6] * cx + yz + 1.;
                    *target = self.pixel_at(
                        (transform[0] * cx + yx + transform[2]) / denominator,
                        (transform[3] * cx + yy + transform[5]) / denominator,
                        max_x,
                        max_y,
                    );
                }
            }
            #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
            for (x, target) in output_row.iter_mut().enumerate() {
                let cx = crate::numeric::usize_f64(x) + 0.5;
                let denominator = transform[6] * cx + yz + 1.;
                let source_x = (transform[0] * cx + yx + transform[2]) / denominator;
                let source_y = (transform[3] * cx + yy + transform[5]) / denominator;
                *target = self.pixel_at(source_x, source_y, max_x, max_y);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Source;

    #[test]
    fn captured_frame_is_owned_and_replaced_between_frames() {
        let mut source = Source::default();
        let mut frame = vec![0, 17, 255, 80, 90, 100];
        assert!(source.capture(&frame, 3, 2));
        source.transform = [1., 0., -0.5, 0., 1., -0.5, 0., 0.];
        frame.fill(33);
        let mut output = vec![];
        assert!(source.sample(&mut output, 3, 2));
        assert_eq!(output, [0, 17, 255, 80, 90, 100]);
        assert!(source.capture(&frame, 3, 2));
        assert!(source.sample(&mut output, 3, 2));
        assert_eq!(output, frame);
        assert!(!source.capture(&frame, 3, 3));
        assert!(!source.sample(&mut output, 3, 2));
    }

    #[test]
    fn invalid_crops_do_not_expose_a_previous_frame() {
        let mut source = Source::default();
        let mut output = vec![];
        assert!(!source.sample(&mut output, 1, 1));
        assert!(source.capture(&[3, 5, 7, 11], 2, 2));
        assert!(!source.sample(&mut output, 0, 1));
        assert!(!source.sample(&mut output, usize::MAX, 2));
        assert!(!source.sample(&mut output, 32 * 1024 * 1024 + 1, 1));
        source.transform[0] = f64::NAN;
        assert!(!source.sample(&mut output, 1, 1));
        source.transform = [1., 0., -10., 0., 1., 0., 0., 0.];
        assert!(source.sample(&mut output, 1, 1));
        assert_eq!(output, [255]);
    }
}

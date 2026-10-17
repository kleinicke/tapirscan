//! Grayscale grid refinement for Data Matrix candidates whose binary samples did not decode.
//!
//! Blooming, blur and uneven light shift a hull-based grid by a fraction of a module and
//! merge the dark L with neighboring modules in the binary image. The gray values of the
//! border modules still separate: the corners are tuned to maximize that separation and the
//! interior is classified against a threshold learned from the border.
use crate::{datamatrix, numeric::usize_f32, qr_detect::map};

type Point = [f32; 2];

/// Largest number of border evaluations spent on one refinement.
const EVALUATION_LIMIT: usize = 400;
/// Sample offsets (in modules) averaged for one interior module.
const OFFSETS: [[f32; 2]; 5] = [[0., 0.], [0.2, 0.], [-0.2, 0.], [0., 0.2], [0., -0.2]];

/// A gray frame, read as light on dark when `inverted`.
#[derive(Clone, Copy)]
struct Frame<'a> {
    gray: &'a [u8],
    width: usize,
    height: usize,
    inverted: bool,
}

impl Frame<'_> {
    /// Bilinear gray value at the grid position `at`, or `None` off the frame.
    fn sample(&self, transform: &[f32; 8], at: [f32; 2]) -> Option<f32> {
        let [px, py] = map(transform, at[0], at[1]);
        // Pixels are areas centered at half-integer source coordinates.
        let (sx, sy) = (px - 0.5, py - 0.5);
        if !(sx.is_finite() && sy.is_finite())
            || sx < 0.
            || sy < 0.
            || sx > usize_f32(self.width - 1)
            || sy > usize_f32(self.height - 1)
        {
            return None;
        }
        let left = crate::numeric::f32_usize(sx.floor());
        let top = crate::numeric::f32_usize(sy.floor());
        let right = (left + 1).min(self.width - 1);
        let bottom = (top + 1).min(self.height - 1);
        let (fx, fy) = (sx - usize_f32(left), sy - usize_f32(top));
        let value = |column: usize, row: usize| f32::from(self.gray[row * self.width + column]);
        let upper = value(left, top) * (1. - fx) + value(right, top) * fx;
        let lower = value(left, bottom) * (1. - fx) + value(right, bottom) * fx;
        let mixed = upper * (1. - fy) + lower * fy;
        Some(if self.inverted { 255. - mixed } else { mixed })
    }

    /// Dark and light means of the border modules, or `None` when one is off the frame.
    fn border_means(
        &self,
        transform: &[f32; 8],
        (cols, rows): (usize, usize),
    ) -> Option<(f32, f32)> {
        const ALONG: [f32; 3] = [-0.25, 0., 0.25];
        let (mut dark, mut light, mut dark_count, mut light_count) = (0., 0., 0_u32, 0_u32);
        for (edge, length) in [(0, cols), (1, rows), (2, cols), (3, rows)] {
            for module in 0..length {
                let (column, row, expected) = match edge {
                    0 => (module, 0, module % 2 == 0),
                    1 => (cols - 1, module, module % 2 == 1),
                    2 => (module, rows - 1, true),
                    _ => (0, module, true),
                };
                for shift in ALONG {
                    let along = if edge % 2 == 0 {
                        [shift, 0.]
                    } else {
                        [0., shift]
                    };
                    let value = self.sample(
                        transform,
                        [
                            usize_f32(column) + 0.5 + along[0],
                            usize_f32(row) + 0.5 + along[1],
                        ],
                    )?;
                    if expected {
                        dark += value;
                        dark_count += 1;
                    } else {
                        light += value;
                        light_count += 1;
                    }
                }
            }
        }
        Some((
            dark / f32::from(u16::try_from(dark_count).ok()?),
            light / f32::from(u16::try_from(light_count).ok()?),
        ))
    }

    /// Light minus dark border level of the grid `q` for a symbol of `size` modules.
    fn contrast(&self, q: &[Point; 4], size: (usize, usize)) -> Option<f32> {
        let transform = crate::dm_detect::rectangle_transform(size.0, size.1, *q)?;
        let (dark, light) = self.border_means(&transform, size)?;
        Some(light - dark)
    }

    /// Coordinate descent on the four corners, maximizing the border contrast.
    fn refine(&self, mut q: [Point; 4], size: (usize, usize)) -> Option<[Point; 4]> {
        let mut best = self.contrast(&q, size)?;
        let module = (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1]) / usize_f32(size.0);
        let mut step = (module * 0.5).max(0.5);
        let mut evaluations = 0;
        while step >= (module * 0.04).max(0.15) && evaluations < EVALUATION_LIMIT {
            let mut improved = false;
            for corner in 0..4 {
                for axis in 0..2 {
                    for sign in [1., -1.] {
                        let mut candidate = q;
                        candidate[corner][axis] += sign * step;
                        evaluations += 1;
                        if let Some(value) = self.contrast(&candidate, size)
                            && value > best
                        {
                            best = value;
                            q = candidate;
                            improved = true;
                        }
                    }
                }
            }
            if !improved {
                step *= 0.5;
            }
        }
        Some(q)
    }
}

/// Refine the grid of `q` for `size`, classify the modules from gray values and decode.
pub(crate) fn recover(
    gray: &[u8],
    dimensions: (usize, usize),
    q: [Point; 4],
    size: datamatrix::Size,
    inverted: bool,
) -> Option<(crate::qr::Payload, [Point; 4])> {
    let frame = Frame {
        gray,
        width: dimensions.0,
        height: dimensions.1,
        inverted,
    };
    let grid = (size.w, size.h);
    let q = frame.refine(q, grid)?;
    let transform = crate::dm_detect::rectangle_transform(size.w, size.h, q)?;
    let (dark, light) = frame.border_means(&transform, grid)?;
    if light - dark < 24. {
        return None;
    }
    let cut = f32::midpoint(light, dark);
    let mut matrix = Vec::with_capacity(size.w * size.h);
    for row in 0..size.h {
        for column in 0..size.w {
            let mut sum = 0.;
            for offset in OFFSETS {
                sum += frame.sample(
                    &transform,
                    [
                        usize_f32(column) + 0.5 + offset[0],
                        usize_f32(row) + 0.5 + offset[1],
                    ],
                )?;
            }
            matrix.push(sum / 5. < cut);
        }
    }
    datamatrix::decode_matrix(&matrix, size.w, size.h).map(|read| (read, q))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: [&str; 16] = [
        "1010101010101010",
        "1111010001001111",
        "1110010101000000",
        "1000010100110011",
        "1001010101000100",
        "1100101100010011",
        "1010010111010010",
        "1100110110100101",
        "1000000011001000",
        "1100110000010111",
        "1110100010011000",
        "1100011100110001",
        "1010110111001110",
        "1111100111000101",
        "1011000000100010",
        "1111111111111111",
    ];

    /// "TAPIRSCAN-DM-42" with a three-module quiet zone, `module` pixels per module,
    /// blurred by two box passes per axis.
    fn blurred_symbol(module: usize, radius: usize) -> (Vec<u8>, usize) {
        let size = 22 * module;
        let mut plane = vec![255_f32; size * size];
        for y in 0..16 * module {
            for x in 0..16 * module {
                let dark = ROWS[y / module].as_bytes()[x / module] == b'1';
                plane[(y + 3 * module) * size + x + 3 * module] = if dark { 50. } else { 200. };
            }
        }
        for vertical in [false, true, false, true] {
            let source = plane.clone();
            for line in 0..size {
                for at in 0..size {
                    let (lo, hi) = (at.saturating_sub(radius), (at + radius).min(size - 1));
                    let sum: f32 = (lo..=hi)
                        .map(|i| {
                            if vertical {
                                source[i * size + line]
                            } else {
                                source[line * size + i]
                            }
                        })
                        .sum();
                    let value = sum / usize_f32(hi - lo + 1);
                    if vertical {
                        plane[at * size + line] = value;
                    } else {
                        plane[line * size + at] = value;
                    }
                }
            }
        }
        (
            plane
                .iter()
                .map(|&v| crate::numeric::f32_u8(v.round()))
                .collect(),
            size,
        )
    }

    fn size_16() -> datamatrix::Size {
        *datamatrix::SIZES
            .iter()
            .find(|s| s.w == 16 && s.h == 16)
            .unwrap()
    }

    #[test]
    fn a_grid_off_by_a_third_of_a_module_is_pulled_onto_the_border() {
        let module = 24;
        let (gray, side) = blurred_symbol(module, 6);
        let (lo, hi) = (usize_f32(3 * module), usize_f32(19 * module));
        // Shifted and stretched by up to 0.4 module on every corner.
        let skew = usize_f32(module) * 0.4;
        let quad = [
            [lo + skew, lo - skew],
            [hi - skew, lo + skew],
            [hi + skew, hi - skew],
            [lo - skew, hi + skew],
        ];
        let (read, refined) = recover(&gray, (side, side), quad, size_16(), false).unwrap();
        assert_eq!(read.text, "TAPIRSCAN-DM-42");
        let tolerance = usize_f32(module) * 0.25;
        for (point, expected) in refined.iter().zip([[lo, lo], [hi, lo], [hi, hi], [lo, hi]]) {
            assert!(
                (point[0] - expected[0]).hypot(point[1] - expected[1]) < tolerance,
                "{point:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn light_modules_on_a_dark_ground_need_the_inverted_flag() {
        let module = 20;
        let (mut gray, side) = blurred_symbol(module, 4);
        for value in &mut gray {
            *value = 255 - *value;
        }
        let (lo, hi) = (usize_f32(3 * module), usize_f32(19 * module));
        let quad = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
        assert!(recover(&gray, (side, side), quad, size_16(), true).is_some());
        assert!(recover(&gray, (side, side), quad, size_16(), false).is_none());
    }

    #[test]
    fn a_flat_patch_has_no_border_contrast_and_reads_nothing() {
        let gray = vec![128_u8; 200 * 200];
        let quad = [[20., 20.], [180., 20.], [180., 180.], [20., 180.]];
        assert!(recover(&gray, (200, 200), quad, size_16(), false).is_none());
    }

    #[test]
    fn an_unread_candidate_of_a_large_blurred_symbol_reads_from_an_averaged_crop() {
        let module = 60;
        let (gray, side) = blurred_symbol(module, 20);
        let (lo, hi) = (usize_f32(3 * module), usize_f32(19 * module));
        let candidate = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
        let reads = crate::dm_detect::crop_retries_for(&gray, (side, side), &[candidate], &[], 1);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, "TAPIRSCAN-DM-42");
        let centre = reads[0]
            .polygon
            .iter()
            .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
        assert!((centre[0] - usize_f32(11 * module)).abs() < usize_f32(module));
        assert!((centre[1] - usize_f32(11 * module)).abs() < usize_f32(module));
        // A read symbol is not retried.
        assert!(
            crate::dm_detect::crop_retries_for(&gray, (side, side), &[candidate], &reads, 1)
                .is_empty()
        );
    }

    #[test]
    fn an_unread_two_pixel_symbol_reads_from_an_enlarged_crop() {
        let module = 2;
        let (gray, side) = blurred_symbol(module, 0);
        let (lo, hi) = (usize_f32(3 * module), usize_f32(19 * module));
        let candidate = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
        let reads = crate::dm_detect::crop_retries_for(&gray, (side, side), &[candidate], &[], 1);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, "TAPIRSCAN-DM-42");
        let centre = reads[0]
            .polygon
            .iter()
            .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
        assert!((centre[0] - usize_f32(11 * module)).abs() < usize_f32(module) * 2.);
    }

    #[test]
    fn an_unread_mid_size_symbol_reads_from_a_crop_at_source_scale_from_high_up() {
        let module = 10;
        let (gray, side) = blurred_symbol(module, 2);
        let (lo, hi) = (usize_f32(3 * module), usize_f32(19 * module));
        let candidate = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
        // Low and Medium retry one box and skip mid-size ones; High retries two.
        let one = crate::dm_detect::crop_retries_for(&gray, (side, side), &[candidate], &[], 1);
        assert!(one.is_empty());
        let reads = crate::dm_detect::crop_retries_for(&gray, (side, side), &[candidate], &[], 2);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, "TAPIRSCAN-DM-42");
    }
}

//! Grayscale grid refinement for Data Matrix candidates whose binary samples did not decode.
//!
//! Blooming, blur and uneven light shift a hull-based grid by a fraction of a module and
//! merge the dark L with neighboring modules in the binary image. The gray values of the
//! border modules still separate: the corners are tuned to maximize that separation and the
//! interior is classified against a threshold learned from the border.
use crate::{datamatrix, gray_sample::bilinear, numeric::usize_f32, qr_detect::map};

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
        let mixed = bilinear(self.gray, self.width, self.height, transform, at[0], at[1])?;
        Some(if self.inverted { 255. - mixed } else { mixed })
    }

    /// Whether the module at (`column`, `row`) is dark against the gray level `cut`.
    fn sample_module(
        &self,
        transform: &[f32; 8],
        (column, row): (usize, usize),
        cut: f32,
    ) -> Option<bool> {
        let mut sum = 0.;
        for offset in OFFSETS {
            sum += self.sample(
                transform,
                [
                    usize_f32(column) + 0.5 + offset[0],
                    usize_f32(row) + 0.5 + offset[1],
                ],
            )?;
        }
        Some(sum / 5. < cut)
    }

    /// Dark and light means of the border modules, or `None` when one is off the frame.
    fn border_means(&self, transform: &[f32; 8], size: (usize, usize)) -> Option<(f32, f32)> {
        self.border_means_along(transform, size, &[-0.25, 0., 0.25])
    }

    /// `border_means` sampling each border module at the given offsets along the border.
    fn border_means_along(
        &self,
        transform: &[f32; 8],
        (cols, rows): (usize, usize),
        along: &[f32],
    ) -> Option<(f32, f32)> {
        let (mut dark, mut light, mut dark_count, mut light_count) = (0., 0., 0_u32, 0_u32);
        for (edge, length) in [(0, cols), (1, rows), (2, cols), (3, rows)] {
            for module in 0..length {
                let (column, row, expected) = match edge {
                    0 => (module, 0, module % 2 == 0),
                    1 => (cols - 1, module, module % 2 == 1),
                    2 => (module, rows - 1, true),
                    _ => (0, module, true),
                };
                for &shift in along {
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
        let mut recent = crate::dm_detect::RecentQuads::new();
        let contrast = |q: &[Point; 4]| self.contrast(q, size);
        let mut best = recent.value(&q, contrast)?;
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
                        if let Some(value) = recent.value(&candidate, contrast)
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

/// Refine the grid of `q` for `size`, classify the modules from gray values and decode;
/// `regions` lets symbols of several data regions refine each region on its own.
pub(crate) fn recover(
    gray: &[u8],
    dimensions: (usize, usize),
    q: [Point; 4],
    size: datamatrix::Size,
    inverted: bool,
    regions: bool,
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
            matrix.push(frame.sample_module(&transform, (column, row), cut)?);
        }
    }
    if let Some(read) = datamatrix::decode_matrix(&matrix, size.w, size.h) {
        return Some((read, q));
    }
    // Large symbols tile data regions that each carry their own L and clock tracks;
    // curvature and lens distortion bend the whole grid but leave every region nearly
    // planar, so each region refines its own corners on its own border.
    let (region_w, region_h) = (size.rw + 2, size.rh + 2);
    if !regions || (size.w == region_w && size.h == region_h) {
        return None;
    }
    matrix.fill(false);
    for top in (0..size.h).step_by(region_h) {
        for left in (0..size.w).step_by(region_w) {
            let corner = |column: usize, row: usize| {
                map(&transform, usize_f32(left + column), usize_f32(top + row))
            };
            let region = [
                corner(0, 0),
                corner(region_w, 0),
                corner(region_w, region_h),
                corner(0, region_h),
            ];
            let region = frame.refine(region, (region_w, region_h))?;
            let transform = crate::dm_detect::rectangle_transform(region_w, region_h, region)?;
            let (dark, light) = frame.border_means(&transform, (region_w, region_h))?;
            if light - dark < 24. {
                return None;
            }
            let cut = f32::midpoint(light, dark);
            for row in 0..region_h {
                for column in 0..region_w {
                    matrix[(top + row) * size.w + left + column] =
                        frame.sample_module(&transform, (column, row), cut)?;
                }
            }
        }
    }
    datamatrix::decode_matrix(&matrix, size.w, size.h).map(|read| (read, q))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// "TAPIRSCAN-DM-42", a 16 x 16 symbol.
    pub(crate) const ROWS: [&str; 16] = [
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

    /// A 44x44 symbol (four 20x20 regions) of 126 bytes, `(i * 37 + 11) % 256 | 0x80`.
    const BIG: [&str; 44] = [
        "10101010101010101010101010101010101010101010",
        "11001110000010011100111111000001100100111001",
        "10000111110001100000001010110111011110011100",
        "10111011111101011000111100111001111000100111",
        "10000101011011100011001001101011011000000110",
        "11001001010100011010111000110111011001110001",
        "11101110010010010001101111010001111101100010",
        "10101010001110111000111001010000110100000101",
        "10011000100010111011101110011000001011011100",
        "10011100000000100111111000101001010110000101",
        "11110111101010000000001110100011000101011010",
        "11010000111001110100011011011000100101101101",
        "10110010110101000100001000011001111000100000",
        "10111000110101100100011011000000101100000011",
        "10100100101010010001101010111011111010010110",
        "11001001001001101111011001000100000110000111",
        "11010001111010111010101001000111000101101010",
        "11011000101100111111111011110110110011000101",
        "11010001001110000011001001000011001011010110",
        "11000000000111010100111101101100010100101111",
        "10001010110101010011101010010111100010111110",
        "11111111111111111111111111111111111111111111",
        "10101010101010101010101010101010101010101010",
        "11111011111011100110111000010111101011000001",
        "11111100101011000011101001111101111010010010",
        "11110100101001111011111110110010011010000011",
        "11010101000101110100101010100001111011000100",
        "10101010001100100000111100110110110011011011",
        "11010001011010101011001010010001011010101110",
        "11110110000101101001111001101110111111010101",
        "10011011101111101110001110100111000110010110",
        "10010011110111011011111011000100101001111101",
        "10111111100101000100101011111011010100111000",
        "11011100010011000101111001010000101011100101",
        "11000001100101100001001100100000001101001100",
        "11001000110001001111111101010000100111001111",
        "10001011110110111110101011100101010111111110",
        "11110011111010011100111010100110110100011111",
        "11010101100011000101001100001101110011001000",
        "10000101000100111000111000111001111000001101",
        "11100111111000111110101110111010000010011010",
        "11100001110110010100011110100111000110101111",
        "11100111111100100101101101101101100111011010",
        "11111111111111111111111111111111111111111111",
    ];

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
        let (read, refined) = recover(&gray, (side, side), quad, size_16(), false, false).unwrap();
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
        assert!(recover(&gray, (side, side), quad, size_16(), true, false).is_some());
        assert!(recover(&gray, (side, side), quad, size_16(), false, false).is_none());
    }

    #[test]
    fn a_flat_patch_has_no_border_contrast_and_reads_nothing() {
        let gray = vec![128_u8; 200 * 200];
        let quad = [[20., 20.], [180., 20.], [180., 180.], [20., 180.]];
        assert!(recover(&gray, (200, 200), quad, size_16(), false, false).is_none());
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

    /// `BIG` with a quiet zone, `module` pixels per module and the columns bowed by
    /// `bow` modules around the middle row, as on a curved surface.
    fn bowed_symbol(module: usize, bow: f32) -> (Vec<u8>, usize) {
        let size = (44 + 6) * module;
        let mut plane = vec![230_f32; size * size];
        let quiet = usize_f32(3 * module);
        for y in 0..size {
            for x in 0..size {
                let v = (usize_f32(y) - quiet) / usize_f32(44 * module);
                let shift = bow * (std::f32::consts::PI * v).sin() * usize_f32(module);
                let (mx, my) = (
                    (usize_f32(x) - quiet - shift) / usize_f32(module),
                    (usize_f32(y) - quiet) / usize_f32(module),
                );
                if (0. ..44.).contains(&mx) && (0. ..44.).contains(&my) {
                    let dark = BIG[crate::numeric::f32_usize(my)].as_bytes()
                        [crate::numeric::f32_usize(mx)]
                        == b'1';
                    plane[y * size + x] = if dark { 40. } else { 220. };
                }
            }
        }
        let source = plane.clone();
        for y in 1..size - 1 {
            for x in 1..size - 1 {
                plane[y * size + x] = (source[y * size + x] * 2.
                    + source[y * size + x - 1]
                    + source[y * size + x + 1]
                    + source[(y - 1) * size + x]
                    + source[(y + 1) * size + x])
                    / 6.;
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

    #[test]
    fn regions_of_a_curved_symbol_refine_on_their_own_borders() {
        let module = 8;
        let (gray, side) = bowed_symbol(module, 1.2);
        let (lo, hi) = (usize_f32(3 * module), usize_f32(47 * module));
        let quad = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]];
        let big = *datamatrix::SIZES
            .iter()
            .find(|s| s.w == 44 && s.h == 44)
            .unwrap();
        let (read, _) = recover(&gray, (side, side), quad, big, false, true).unwrap();
        let expected: Vec<u8> = (0..126_usize)
            .map(|i| u8::try_from((i * 37 + 11) % 256).unwrap() | 0x80)
            .collect();
        assert_eq!(read.bytes, expected);
    }
}

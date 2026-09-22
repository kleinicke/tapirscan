//! Working-raster preparation and Scharr tensors; storage belongs to the detector.
use super::{tensor_magnitude, Error, ImageView, Tile};

#[derive(Default)]
pub(super) struct Raster {
    pub width: usize,
    pub height: usize,
    pub scale: f64,
    pub sparse: bool,
    pub gray: Vec<f32>,
    pub gx: Vec<f32>,
    pub gy: Vec<f32>,
    pub tiles: Vec<Tile>,
}
impl Raster {
    pub fn prepare(
        &mut self,
        im: ImageView<'_>,
        working_dimension: f64,
        bilinear: bool,
    ) -> Result<(), Error> {
        if im.width < 3 || im.height < 3 {
            return Err(Error::Dimensions);
        }
        self.scale =
            (working_dimension / crate::numeric::usize_f64(im.width.max(im.height))).min(1.);
        self.width =
            crate::numeric::f64_usize((crate::numeric::usize_f64(im.width) * self.scale).round())
                .max(3);
        self.height =
            crate::numeric::f64_usize((crate::numeric::usize_f64(im.height) * self.scale).round())
                .max(3);
        let length = self.width * self.height;
        self.gray.resize(length, 0.);
        self.gx.resize(length, 0.);
        self.gy.resize(length, 0.);
        // Sparse sampling and image borders leave cells unwritten; clear old gradients.
        self.gx.fill(0.);
        self.gy.fill(0.);
        let conditioned = self.sample(im, bilinear);
        self.sparse =
            cfg!(feature = "mode-low") && self.width.max(self.height) > 512 && !conditioned;
        self.gradients(conditioned, bilinear);
        Ok(())
    }
    fn sample(&mut self, im: ImageView<'_>, bilinear: bool) -> bool {
        let mut histogram_lanes = [[0usize; 256]; 4];
        if bilinear {
            self.sample_bilinear(im, &mut histogram_lanes);
        } else {
            self.sample_nearest(im, &mut histogram_lanes);
        }
        condition(&mut self.gray, &histogram_lanes)
    }
    fn sample_bilinear(&mut self, im: ImageView<'_>, histogram_lanes: &mut [[usize; 256]; 4]) {
        let (working_width, working_height) = (self.width, self.height);
        let gray = &mut self.gray;

        // Diagnostic antialiased working-raster hypothesis; decoder source pixels
        // stay untouched. Same legacy RGB weights, bilinear center sampling.
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        let luminance = |x: usize, y: usize| {
            let i = y * im.stride + x * im.channels;
            if im.channels == 1 {
                f64::from(im.data[i])
            } else {
                (77. * f64::from(im.data[i])
                    + 150. * f64::from(im.data[i + 1])
                    + 29. * f64::from(im.data[i + 2]))
                    / 256.
            }
        };
        #[cfg(any(feature = "mode-low", feature = "mode-medium", feature = "mode-high"))]
        {
            for y in 0..working_height {
                let sy = ((crate::numeric::usize_f64(y) + 0.5)
                    * crate::numeric::usize_f64(im.height)
                    / crate::numeric::usize_f64(working_height)
                    - 0.5)
                    .clamp(0., crate::numeric::usize_f64(im.height - 1));
                let y0 = crate::numeric::f64_usize(sy.floor());
                let y1 = (y0 + 1).min(im.height - 1);
                let fy = sy - crate::numeric::usize_f64(y0);
                for x in 0..working_width {
                    let sx = ((crate::numeric::usize_f64(x) + 0.5)
                        * crate::numeric::usize_f64(im.width)
                        / crate::numeric::usize_f64(working_width)
                        - 0.5)
                        .clamp(0., crate::numeric::usize_f64(im.width - 1));
                    let x0 = crate::numeric::f64_usize(sx.floor());
                    let x1 = (x0 + 1).min(im.width - 1);
                    let fx = sx - crate::numeric::usize_f64(x0);
                    let top_left = luminance(x0, y0);
                    let top_right = luminance(x1, y0);
                    let bottom_left = luminance(x0, y1);
                    let bottom_right = luminance(x1, y1);
                    let u = top_left + (top_right - top_left) * fx;
                    let v = bottom_left + (bottom_right - bottom_left) * fx;
                    let value = crate::numeric::f64_f32(u + (v - u) * fy);
                    gray[y * working_width + x] = value;
                    histogram_lanes[x & 3][crate::numeric::f32_usize(value)] += 1;
                }
            }
        }
        #[cfg(feature = "mode-very-high")]
        {
            secondary_gray(im, working_width, working_height, gray, histogram_lanes);
        }
    }
    fn sample_nearest(&mut self, im: ImageView<'_>, histogram_lanes: &mut [[usize; 256]; 4]) {
        let (working_width, working_height) = (self.width, self.height);
        let gray = &mut self.gray;

        // Source-column mapping is invariant across all output rows. Integer RGB
        // weights are exactly the legacy binary-fraction luminance, not a new filter.
        let columns: Vec<_> = (0..working_width)
            .map(|x| {
                crate::numeric::f64_usize(
                    ((crate::numeric::usize_f64(x) + 0.5) * crate::numeric::usize_f64(im.width)
                        / crate::numeric::usize_f64(working_width))
                    .floor(),
                )
                .min(im.width - 1)
                    * im.channels
            })
            .collect();
        for y in 0..working_height {
            let sy = crate::numeric::f64_usize(
                ((crate::numeric::usize_f64(y) + 0.5) * crate::numeric::usize_f64(im.height)
                    / crate::numeric::usize_f64(working_height))
                .floor(),
            )
            .min(im.height - 1);
            let row = &im.data[sy * im.stride..];
            let dst = &mut gray[y * working_width..(y + 1) * working_width];
            if im.channels == 1 {
                for (x, &i) in columns.iter().enumerate() {
                    let value = row[i];
                    dst[x] = f32::from(value);
                    histogram_lanes[x & 3][usize::from(value)] += 1;
                }
            } else {
                for (x, &i) in columns.iter().enumerate() {
                    let weighted = 77 * u32::from(row[i])
                        + 150 * u32::from(row[i + 1])
                        + 29 * u32::from(row[i + 2]);
                    dst[x] = crate::numeric::f64_f32(f64::from(weighted)) / 256.;
                    // The bounded 16-bit weighted sum's high byte equals floor(gray).
                    histogram_lanes[x & 3][usize::from(weighted.to_le_bytes()[1])] += 1;
                }
            }
        }
    }

    fn gradients(&mut self, conditioned: bool, bilinear: bool) {
        let (working_width, working_height) = (self.width, self.height);
        let tw = working_width.div_ceil(8);
        self.tiles
            .resize(tw * working_height.div_ceil(8), Tile::default());
        self.tiles.fill(Tile::default());
        let gray = &self.gray;
        let (gx, gy, tiles) = (&mut self.gx, &mut self.gy, &mut self.tiles);

        let sparse = cfg!(feature = "mode-low") && self.sparse;

        let weight = if sparse { 4. } else { 1. };
        for y in 1..working_height - 1 {
            // Enumerate the exact quarter lattice in its original x order.

            let [a, bin_index] = [[0usize, 3], [1, 6], [4, 7], [2, 5]][(4 - (y + y / 2) % 4) % 4];

            let mut x = if sparse {
                if a == 0 {
                    bin_index
                } else {
                    a
                }
            } else {
                1
            };
            {
                {
                    while x < working_width - 1 {
                        let i = y * working_width + x;
                        // Unconditioned samples are exact multiples of 1/256. Every Scharr
                        // intermediate fits within 20 significant bits, so f32 is exact.
                        // Conditioned low-light samples retain the original f64 arithmetic.
                        let (dx, dy) = if !conditioned && !bilinear {
                            let g = |i: usize| gray[i];
                            (
                                f64::from(
                                    (3. * (g(i - working_width + 1) - g(i - working_width - 1))
                                        + 10. * (g(i + 1) - g(i - 1))
                                        + 3. * (g(i + working_width + 1)
                                            - g(i + working_width - 1)))
                                        / 16.,
                                ),
                                f64::from(
                                    (3. * (g(i + working_width - 1) - g(i - working_width - 1))
                                        + 10. * (g(i + working_width) - g(i - working_width))
                                        + 3. * (g(i + working_width + 1)
                                            - g(i - working_width + 1)))
                                        / 16.,
                                ),
                            )
                        } else {
                            let g = |i: usize| f64::from(gray[i]);
                            let dx = (3. * (g(i - working_width + 1) - g(i - working_width - 1))
                                + 10. * (g(i + 1) - g(i - 1))
                                + 3. * (g(i + working_width + 1) - g(i + working_width - 1)))
                                / 16.;
                            let dy = (3. * (g(i + working_width - 1) - g(i - working_width - 1))
                                + 10. * (g(i + working_width) - g(i - working_width))
                                + 3. * (g(i + working_width + 1) - g(i - working_width + 1)))
                                / 16.;
                            (dx, dy)
                        };
                        gx[i] = crate::numeric::f64_f32(dx);
                        gy[i] = crate::numeric::f64_f32(dy);
                        let tile = &mut tiles[y / 8 * tw + x / 8];
                        tile.xx += dx * dx * weight;
                        tile.xy += dx * dy * weight;
                        tile.yy += dy * dy * weight;
                        x += if !sparse {
                            1
                        } else if x % 8 == a {
                            bin_index - a
                        } else {
                            8 + a - bin_index
                        };
                    }
                }
            }
        }

        {
            for tile in tiles.iter_mut() {
                let energy = tile.xx + tile.yy;
                tile.angle = 0.5 * (2. * tile.xy).atan2(tile.xx - tile.yy);
                tile.active = energy > 64. * 80.
                    && tensor_magnitude(tile.xx, tile.xy, tile.yy) / (energy + 1.) > 0.60;
            }
        }
    }
}
// Column geometry is invariant across rows. The integer RGB dot product is
// exactly the former f64 sum: bounded integer inputs and binary denominator256.
#[cfg(feature = "mode-very-high")]
pub(super) fn secondary_gray(
    im: ImageView<'_>,
    width: usize,
    height: usize,
    gray: &mut [f32],
    histogram: &mut [[usize; 256]; 4],
) {
    let columns: Vec<_> = (0..width)
        .map(|x| {
            let sx = ((crate::numeric::usize_f64(x) + 0.5) * crate::numeric::usize_f64(im.width)
                / crate::numeric::usize_f64(width)
                - 0.5)
                .clamp(0., crate::numeric::usize_f64(im.width - 1));
            let x0 = crate::numeric::f64_usize(sx.floor());
            (
                x0 * im.channels,
                (x0 + 1).min(im.width - 1) * im.channels,
                sx - crate::numeric::usize_f64(x0),
            )
        })
        .collect();
    let lum = |row: &[u8], i: usize| {
        if im.channels == 1 {
            f64::from(row[i])
        } else {
            f64::from(
                77 * u32::from(row[i]) + 150 * u32::from(row[i + 1]) + 29 * u32::from(row[i + 2]),
            ) / 256.
        }
    };
    for y in 0..height {
        let sy = ((crate::numeric::usize_f64(y) + 0.5) * crate::numeric::usize_f64(im.height)
            / crate::numeric::usize_f64(height)
            - 0.5)
            .clamp(0., crate::numeric::usize_f64(im.height - 1));
        let y0 = crate::numeric::f64_usize(sy.floor());
        let y1 = (y0 + 1).min(im.height - 1);
        let fy = sy - crate::numeric::usize_f64(y0);
        let top = &im.data[y0 * im.stride..];
        let bottom = &im.data[y1 * im.stride..];
        for (x, &(x0, x1, fx)) in columns.iter().enumerate() {
            let top_left = lum(top, x0);
            let top_right = lum(top, x1);
            let bottom_left = lum(bottom, x0);
            let bottom_right = lum(bottom, x1);
            let u = top_left + (top_right - top_left) * fx;
            let v = bottom_left + (bottom_right - bottom_left) * fx;
            let value = crate::numeric::f64_f32(u + (v - u) * fy);
            gray[y * width + x] = value;
            histogram[x & 3][crate::numeric::f32_usize(value)] += 1;
        }
    }
}

fn condition(gray: &mut [f32], histogram_lanes: &[[usize; 256]; 4]) -> bool {
    // Only the <=768px working gray raster is conditioned. Source RGB stays
    // untouched; the decoder samples original pixels with its own contrast gates.
    let histogram: [usize; 256] =
        std::array::from_fn(|i| histogram_lanes.iter().map(|lane| lane[i]).sum());
    let quantile = |target: usize| {
        let mut sum = 0;
        {
            {
                for (i, &neighbor_index) in histogram.iter().enumerate() {
                    sum += neighbor_index;
                    if sum > target {
                        return crate::numeric::usize_f32(i);
                    }
                }
            }
        }

        255.
    };
    let lo = quantile(gray.len() * 2 / 100);
    let hi = quantile(gray.len() * 98 / 100);
    let span = hi - lo;
    let conditioned = (8.0..96.0).contains(&span);
    if conditioned {
        let gain = (192. / span).min(8.);
        for v in gray.iter_mut() {
            *v = ((*v - lo) * gain).clamp(0., 255.);
        }
    }
    conditioned
}

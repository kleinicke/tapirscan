//! Deconvolved crop rescans for blurred EAN-13 symbols in Medium.
//!
//! Motion during the exposure doubles the bars (ghosting) and defocus smears them. Both leave the
//! symbol well localized but defeat every decoder pass. When a frame reads nothing, the strongest
//! unread segment-voting boxes are resampled upright, deblurred along the scan axis under three
//! kernel hypotheses and rescanned for EAN-13/UPC-A only: short formats would decode the ghost
//! patterns that deblurring leaves inside the bars.
use super::{
    checked_image, scan_policy, source_contradiction, Error, Image, Proposal, Quad, ScanOptions,
    Scanner,
};
use barcode_research_core::numeric::{f64_u8, f64_usize, usize_f64};

/// Boxes tried per frame, and the minimum segment-voting score (about 20 voting bar edges).
const BOXES: usize = 1;
const MIN_SCORE: f64 = 0.9;
/// Supporting rows a deblurred read needs.
const MIN_SUPPORT: usize = 4;
/// Longest side of an upright crop, and the crop resolution in pixels per box module: sharp
/// symbols need no more, and smaller crops keep the rescans cheap.
const MAX_SIDE: f64 = 1600.;
const MODULE_PIXELS: f64 = 3.;
/// Ghost lag in box modules (boxes include quiet zone) and relative ghost strength.
const GHOST_MODULES: f64 = 0.8;
const GHOST_AMPLITUDE: f64 = 0.5;
/// Crops whose bar edges rise faster than this share of the row contrast per crop pixel are
/// sharp (often non-retail symbols such as Code 128) and are not deblurred. Corpus medians:
/// 0.33 for recovered blurred codes, 0.47 for the shaken video, 0.80 for non-target symbols.
const MAX_SHARPNESS: f64 = 0.6;
/// Gaussian defocus width in box modules, and Van Cittert iterations.
const DEFOCUS_MODULES: f64 = 0.7;
const DEFOCUS_ITERATIONS: usize = 3;

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// A box region resampled so that its scan axis runs along the rows.
struct Upright {
    rows: Vec<f64>,
    width: usize,
    height: usize,
    origin: [f64; 2],
    u: [f64; 2],
    n: [f64; 2],
    /// Box modules (an EAN-13 box holds 95) in crop pixels.
    module: f64,
    /// Source pixels per crop pixel.
    step: f64,
}

impl Upright {
    fn new(q: Quad) -> Option<Self> {
        let length = distance(q[0], q[1]);
        if length < 40. {
            return None;
        }
        // Source pixels per crop pixel along both crop axes.
        let step = (length / 95. / MODULE_PIXELS).max(1.);
        let u = [
            (q[1][0] - q[0][0]) / length * step,
            (q[1][1] - q[0][1]) / length * step,
        ];
        let n = [-u[1], u[0]];
        let bars = distance(q[0], q[3]).max(distance(q[1], q[2]));
        let width = f64_usize((1.6 * length / step).min(MAX_SIDE).round());
        let height = f64_usize(
            ((1.6 * bars).max(0.3 * length) / step)
                .min(MAX_SIDE)
                .round(),
        );
        let centre = [
            q.iter().map(|p| p[0]).sum::<f64>() / 4.,
            q.iter().map(|p| p[1]).sum::<f64>() / 4.,
        ];
        let origin = [
            centre[0] - 0.5 * usize_f64(width) * u[0] - 0.5 * usize_f64(height) * n[0],
            centre[1] - 0.5 * usize_f64(width) * u[1] - 0.5 * usize_f64(height) * n[1],
        ];
        if width < 64 || height < 8 {
            return None;
        }
        Some(Self {
            rows: vec![0.; width * height],
            width,
            height,
            origin,
            u,
            n,
            module: length / 95. / step,
            step,
        })
    }

    /// Central rows, which the sharpness test reads before the rest is sampled.
    fn band(&self) -> std::ops::Range<usize> {
        self.height * 21 / 50..self.height * 29 / 50
    }

    /// Resample `rows` of the crop from the source image.
    fn fill(&mut self, image: Image<'_>, rows: std::ops::Range<usize>) {
        // Reduced crops average a 2 x 2 grid per pixel against aliasing.
        let offsets: &[f64] = if self.step > 1.25 {
            &[0.25, 0.75]
        } else {
            &[0.5]
        };
        let weight = usize_f64(offsets.len() * offsets.len());
        for y in rows {
            for x in 0..self.width {
                let mut sum = 0.;
                for &dy in offsets {
                    for &dx in offsets {
                        sum += bilinear(image, self.source([usize_f64(x) + dx, usize_f64(y) + dy]));
                    }
                }
                self.rows[y * self.width + x] = sum / weight;
            }
        }
    }

    /// Crop position of a source-image position.
    fn local(&self, p: [f64; 2]) -> [f64; 2] {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1]];
        let step2 = self.u[0] * self.u[0] + self.u[1] * self.u[1];
        [
            (d[0] * self.u[0] + d[1] * self.u[1]) / step2,
            (d[0] * self.n[0] + d[1] * self.n[1]) / step2,
        ]
    }

    /// Source-image position of a crop position.
    fn source(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.origin[0] + p[0] * self.u[0] + p[1] * self.n[0],
            self.origin[1] + p[0] * self.u[1] + p[1] * self.n[1],
        ]
    }

    /// Median over the central rows of the steepest edges (95th percentile of the absolute
    /// sample difference) relative to the row contrast (5th to 95th percentile).
    fn sharpness(&self) -> Option<f64> {
        let percentile = |values: &mut Vec<f64>, f: f64| {
            values.sort_by(f64::total_cmp);
            values[f64_usize(f * usize_f64(values.len() - 1))]
        };
        let (x0, x1) = (self.width / 5, self.width * 4 / 5);
        let mut scores = Vec::new();
        for y in self.band() {
            let row = &self.rows[y * self.width + x0..y * self.width + x1];
            if row.len() < 8 {
                continue;
            }
            let mut values = row.to_vec();
            let range = percentile(&mut values, 0.95) - percentile(&mut values, 0.05);
            if range <= 20. {
                continue;
            }
            let mut steps: Vec<f64> = row.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
            scores.push(percentile(&mut steps, 0.95) / range);
        }
        (!scores.is_empty()).then(|| percentile(&mut scores, 0.5))
    }
}

/// Crop rows as 8-bit gray.
fn pixels(rows: &[f64]) -> Vec<u8> {
    rows.iter()
        .map(|v| f64_u8(v.clamp(0., 255.).round()))
        .collect()
}

/// Edge-clamped bilinear luminance.
fn bilinear(image: Image<'_>, p: [f64; 2]) -> f64 {
    let max_x = usize_f64(image.width - 1);
    let max_y = usize_f64(image.height - 1);
    let (x, y) = (p[0].clamp(0., max_x), p[1].clamp(0., max_y));
    let (x0, y0) = (f64_usize(x), f64_usize(y));
    let (x1, y1) = (
        (x0 + 1).min(image.width - 1),
        (y0 + 1).min(image.height - 1),
    );
    let (fx, fy) = (x - usize_f64(x0), y - usize_f64(y0));
    let at = |x: usize, y: usize| image.fixed_luminance(y * image.stride + x * image.channels);
    (at(x0, y0) * (1. - fx) + at(x1, y0) * fx) * (1. - fy)
        + (at(x0, y1) * (1. - fx) + at(x1, y1) * fx) * fy
}

/// Invert a ghost `lag` pixels behind each sample (ahead of it when `backward`), per row.
fn deghost(crop: &Upright, lag: f64, backward: bool) -> Vec<f64> {
    let mut out = crop.rows.clone();
    let whole = f64_usize(lag.floor()).max(1);
    let fraction = (lag - usize_f64(whole)).max(0.);
    for row in out.chunks_mut(crop.width) {
        if backward {
            row.reverse();
        }
        for i in whole..row.len() {
            // The first restored sample has no older neighbour to interpolate with.
            let older = row[i - whole - usize::from(i > whole)];
            let echo = (1. - fraction) * row[i - whole] + fraction * older;
            row[i] = (1. + GHOST_AMPLITUDE) * row[i] - GHOST_AMPLITUDE * echo;
        }
        if backward {
            row.reverse();
        }
    }
    out
}

/// Van Cittert deconvolution of a Gaussian of width `sigma` pixels along each row.
fn defocus(crop: &Upright, sigma: f64) -> Vec<f64> {
    let radius = f64_usize((3. * sigma).ceil()).max(1);
    let kernel: Vec<f64> = (0..=2 * radius)
        .map(|i| {
            let x = usize_f64(i) - usize_f64(radius);
            (-0.5 * (x / sigma) * (x / sigma)).exp()
        })
        .collect();
    let total: f64 = kernel.iter().sum();
    let blur = |rows: &[f64]| -> Vec<f64> {
        let mut out = vec![0.; rows.len()];
        for (row, target) in rows.chunks(crop.width).zip(out.chunks_mut(crop.width)) {
            for (i, value) in target.iter_mut().enumerate() {
                *value = kernel
                    .iter()
                    .enumerate()
                    .map(|(k, w)| w * row[(i + k).saturating_sub(radius).min(row.len() - 1)])
                    .sum::<f64>()
                    / total;
            }
        }
        out
    };
    let mut estimate = crop.rows.clone();
    for _ in 0..DEFOCUS_ITERATIONS {
        let blurred = blur(&estimate);
        for ((e, b), g) in estimate.iter_mut().zip(&blurred).zip(&crop.rows) {
            *e += g - b;
        }
    }
    estimate
}

/// Rescan the strongest unread boxes of a frame that read nothing after deblurring them.
pub(super) fn recover_deblurred_crops(
    scanner: &mut Scanner,
    image: Image<'_>,
    options: ScanOptions,
    proposals: &[Proposal],
    scan: &mut crate::ScanResult,
) -> std::result::Result<(), Error> {
    let mut boxes: Vec<_> = proposals.iter().filter(|p| p.score >= MIN_SCORE).collect();
    boxes.sort_by(|a, b| b.score.total_cmp(&a.score));
    for proposal in boxes.into_iter().take(BOXES) {
        if scan.frame.barcodes.iter().any(|b| {
            crate::geometry::overlap_quads(&b.detection.polygon, &proposal.polygon).0 > 0.3
        }) {
            continue;
        }
        let Some(mut crop) = Upright::new(proposal.polygon) else {
            continue;
        };
        let band = crop.band();
        crop.fill(image, band.clone());
        if crop.sharpness().is_none_or(|s| s > MAX_SHARPNESS) {
            continue;
        }
        let height = crop.height;
        crop.fill(image, 0..band.start);
        crop.fill(image, band.end..height);
        // The symbol sits where the box was: decode only that box, without localizing or
        // recovering again (the whole crop doubled the cost on frames without retail codes).
        let local = proposal.polygon.map(|p| crop.local(p));
        let lag = GHOST_MODULES * crop.module;
        let variants = [
            deghost(&crop, lag, false),
            deghost(&crop, lag, true),
            defocus(&crop, DEFOCUS_MODULES * crop.module),
        ];
        'variants: for rows in &variants {
            let pixels = pixels(rows);
            let variant = Image {
                data: &pixels,
                width: crop.width,
                height: crop.height,
                channels: 1,
                stride: crop.width,
            };
            let policy = scan_policy(
                variant,
                &[Proposal {
                    polygon: local,
                    score: proposal.score,
                }],
                &[],
                options,
            );
            scanner.regions.retail_configure(1)?;
            let retry = scanner
                .regions
                .scan(checked_image(variant)?, &[local], policy)?;
            for mut read in retry.frame.barcodes {
                if read.detection.support < MIN_SUPPORT {
                    continue;
                }
                read.detection.polygon = read.detection.polygon.map(|p| crop.source(p));
                if crate::geometry::overlap_quads(&read.detection.polygon, &proposal.polygon).0
                    < 0.3
                    || scan
                        .frame
                        .barcodes
                        .iter()
                        .any(|b| b.detection.digits == read.detection.digits)
                    || source_contradiction(
                        image,
                        read.detection.polygon,
                        read.detection.digits,
                        &mut scanner.fast_profiles,
                    )?
                {
                    continue;
                }
                read.candidate_indices.clear();
                scan.frame.barcodes.push(read);
                break 'variants;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crop(rows: Vec<f64>, width: usize) -> Upright {
        Upright {
            height: rows.len() / width,
            rows,
            width,
            origin: [0., 0.],
            u: [1., 0.],
            n: [0., 1.],
            module: 3.,
            step: 1.,
        }
    }

    /// Bars of `module` pixels with a 2:1 pattern, one row repeated `height` times.
    fn bars(width: usize, height: usize) -> Vec<f64> {
        (0..height)
            .flat_map(|_| (0..width).map(|x| if (x / 3) % 3 == 0 { 30. } else { 220. }))
            .collect()
    }

    #[test]
    fn crop_coordinates_round_trip_through_a_rotated_box() {
        let q = [[100., 50.], [400., 150.], [380., 210.], [80., 110.]];
        let c = Upright::new(q).unwrap();
        for p in q {
            let back = c.source(c.local(p));
            assert!((back[0] - p[0]).abs() < 1e-9 && (back[1] - p[1]).abs() < 1e-9);
        }
        // The box centre maps to the crop centre.
        let centre = c.local([240., 130.]);
        assert!((centre[0] - usize_f64(c.width) / 2.).abs() < 1.);
        assert!((centre[1] - usize_f64(c.height) / 2.).abs() < 1.);
    }

    #[test]
    fn deghost_removes_a_shifted_copy_along_the_rows() {
        let sharp = crop(bars(90, 2), 90);
        let mut ghosted = sharp.rows.clone();
        for row in ghosted.chunks_mut(90) {
            let source = row.to_vec();
            for i in 3..90 {
                row[i] = (source[i] + 0.5 * source[i - 3]) / 1.5;
            }
        }
        let restored = deghost(&crop(ghosted, 90), 3., false);
        for (r, s) in restored.iter().zip(&sharp.rows).skip(10) {
            assert!((r - s).abs() < 1e-6);
        }
    }

    #[test]
    fn defocus_restores_contrast_of_blurred_bars() {
        let sharp = crop(bars(90, 1), 90);
        let blurred = crop(
            (0..90_usize)
                .map(|i| {
                    let window = &sharp.rows[i.saturating_sub(1)..(i + 2).min(90)];
                    window.iter().sum::<f64>() / usize_f64(window.len())
                })
                .collect(),
            90,
        );
        let spread = |rows: &[f64]| {
            rows[10..80]
                .iter()
                .copied()
                .fold(f64::NEG_INFINITY, f64::max)
                - rows[10..80].iter().copied().fold(f64::INFINITY, f64::min)
        };
        let restored = defocus(&blurred, 0.7);
        assert!(spread(&restored) > spread(&blurred.rows) + 20.);
    }

    #[test]
    fn sharpness_separates_crisp_from_smeared_edges() {
        let height = 50;
        let crisp = crop(bars(120, height), 120);
        let smeared = crop(
            crisp
                .rows
                .chunks(120)
                .flat_map(|row| {
                    (0..120_usize).map(move |i| {
                        let window = &row[i.saturating_sub(3)..(i + 4).min(120)];
                        window.iter().sum::<f64>() / usize_f64(window.len())
                    })
                })
                .collect(),
            120,
        );
        assert!(crisp.sharpness().unwrap() > MAX_SHARPNESS);
        assert!(smeared.sharpness().unwrap() < MAX_SHARPNESS);
    }
}

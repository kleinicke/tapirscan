//! Bounded axis-aligned proposal localizer for small, high-contrast linear codes.
//! Directional transition density, not a trained detector or symbology decision.
#![forbid(unsafe_code)]
use crate::sampling::{Error, ImageView};
pub const MAX_PROPOSALS: usize = 12;
const TILE: usize = 8;
const MAX_SIDE: usize = 1536;
#[derive(Clone, Copy, Debug)]
pub struct Proposal {
    pub bounds: [f64; 4],
    pub score: f64,
}
#[derive(Default)]
pub struct Localizer {
    gray: Vec<u8>,
    mask: Vec<u8>,
    axis_masks: Vec<[u8; 2]>,
    closed: Vec<u8>,

    queue: Vec<usize>,
    proposals: Vec<Proposal>,

    pub omitted: usize,
    pub work_limited: bool,
}
fn resize<T: Clone>(v: &mut Vec<T>, n: usize, value: T) -> Result<(), Error> {
    if n > v.len() {
        v.try_reserve_exact(n - v.len())
            .map_err(|_| Error::Allocation)?;
    }
    v.resize(n, value);
    Ok(())
}
impl Localizer {
    /// Integer point subsampling caps the working image at 1536 per side.
    /// Returned boxes are in original-image coordinates. At most twelve boxes
    /// survive a deterministic density/area ranking; undecodability is unknown.
    /// # Errors
    /// Returns `Allocation` if the bounded localization scratch buffers cannot be reserved.
    #[expect(
        clippy::too_many_lines,
        reason = "Tile thresholding, morphology and component ranking share bounded scratch buffers and one deterministic proposal order."
    )]
    pub fn detect(&mut self, image: ImageView<'_>) -> Result<&[Proposal], Error> {
        self.proposals.clear();
        self.omitted = 0;
        self.work_limited = false;

        let step = image.width.max(image.height).div_ceil(MAX_SIDE);
        let working_width = image.width.div_ceil(step);
        let working_height = image.height.div_ceil(step);
        let tw = working_width.div_ceil(TILE);
        let th = working_height.div_ceil(TILE);
        let tiles = tw * th;
        resize(&mut self.gray, working_width * working_height, 0)?;
        resize(&mut self.mask, tiles, 0)?;
        resize(&mut self.axis_masks, tiles, [0; 2])?;
        resize(&mut self.closed, tiles, 0)?;
        self.queue.clear();
        self.queue
            .try_reserve_exact(tiles.saturating_sub(self.queue.len()))
            .map_err(|_| Error::Allocation)?;
        // A component cannot have more than one proposal per tile.
        self.proposals
            .try_reserve_exact(tiles.saturating_sub(self.proposals.len()))
            .map_err(|_| Error::Allocation)?;
        for y in 0..working_height {
            for x in 0..working_width {
                let i = y * step * image.stride + x * step * image.channels;
                self.gray[y * working_width + x] = if image.channels == 1 {
                    image.data[i]
                } else {
                    ((77 * u32::from(image.data[i])
                        + 150 * u32::from(image.data[i + 1])
                        + 29 * u32::from(image.data[i + 2]))
                        >> 8)
                        .to_le_bytes()[0]
                };
            }
        }
        for ty in 0..th {
            for tx in 0..tw {
                let (mut gx, mut gy, mut ex, mut ey, mut samples) = (0u32, 0u32, 0u32, 0u32, 0u32);
                for y in (ty * TILE).max(1)..((ty + 1) * TILE).min(working_height) {
                    for x in (tx * TILE).max(1)..((tx + 1) * TILE).min(working_width) {
                        let p = self.gray[y * working_width + x];
                        let dx = u32::from(p.abs_diff(self.gray[y * working_width + x - 1]));
                        let dy = u32::from(p.abs_diff(self.gray[(y - 1) * working_width + x]));
                        // Retain the one-pixel edge for narrow modules; the
                        // wider difference measures blurred/wide transitions.

                        gx += dx;
                        gy += dy;
                        ex += u32::from(dx >= 24);
                        ey += u32::from(dy >= 24);
                        samples += 1;
                    }
                }
                for axis in 0..2 {
                    let (along, across, edges) = if axis == 0 {
                        (gx, gy, ex)
                    } else {
                        (gy, gx, ey)
                    };
                    self.axis_masks[ty * tw + tx][axis] = u8::from(
                        samples > 0
                            && along > 12 * samples
                            && along > 2 * across
                            && edges * 6 >= samples,
                    );
                }
            }
        }
        for axis in 0..2 {
            for (mask, pair) in self.mask.iter_mut().zip(&self.axis_masks) {
                *mask = pair[axis];
            }
            // Bridge one missing tile along the module axis. No dilation into
            // arbitrary neighboring text regions; use 4-connected components.
            self.closed.copy_from_slice(&self.mask);
            for y in 0..th {
                for x in 0..tw {
                    if axis == 0
                        && x > 0
                        && x + 1 < tw
                        && self.mask[y * tw + x - 1] > 0
                        && self.mask[y * tw + x + 1] > 0
                    {
                        self.closed[y * tw + x] = 1;
                    }
                    if axis == 1
                        && y > 0
                        && y + 1 < th
                        && self.mask[(y - 1) * tw + x] > 0
                        && self.mask[(y + 1) * tw + x] > 0
                    {
                        self.closed[y * tw + x] = 1;
                    }
                }
            }

            for start in 0..tiles {
                if self.closed[start] == 0 {
                    continue;
                }
                self.queue.clear();
                self.queue.push(start);
                self.closed[start] = 0;
                let (mut x0, mut x1, mut y0, mut y1) =
                    (start % tw, start % tw, start / tw, start / tw);
                let mut head = 0;
                while head < self.queue.len() {
                    let p = self.queue[head];
                    head += 1;
                    let (x, y) = (p % tw, p / tw);
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                    for next in [
                        if x > 0 { Some(p - 1) } else { None },
                        if x + 1 < tw { Some(p + 1) } else { None },
                        if y > 0 { Some(p - tw) } else { None },
                        if y + 1 < th { Some(p + tw) } else { None },
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if self.closed[next] > 0 {
                            self.closed[next] = 0;
                            self.queue.push(next);
                        }
                    }
                }
                let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);

                let count = self.queue.len();

                let (span, depth) = if axis == 0 { (bw, bh) } else { (bh, bw) };
                if span < 6
                    || depth < 2
                    || span * 5 < depth * 6
                    || span > depth * 25
                    || count < 6
                    || count * 3 < bw * bh
                {
                    continue;
                }
                let unit = TILE * step;
                // One-tile surrounding context supplies quiet zones; the scanner
                // also applies its declared crop margin from original pixels.
                let bounds = [
                    x0.saturating_sub(1) * unit,
                    y0.saturating_sub(1) * unit,
                    ((x1 + 2) * unit).min(image.width),
                    ((y1 + 2) * unit).min(image.height),
                ]
                .map(crate::numeric::usize_f64);

                self.proposals.push(Proposal {
                    bounds,
                    score: crate::numeric::usize_f64(count) / crate::numeric::usize_f64(bw * bh),
                });
            }
        }
        self.proposals.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| {
                    let area =
                        |p: &Proposal| (p.bounds[2] - p.bounds[0]) * (p.bounds[3] - p.bounds[1]);
                    area(b).total_cmp(&area(a))
                })
                .then_with(|| a.bounds[1].total_cmp(&b.bounds[1]))
                .then_with(|| a.bounds[0].total_cmp(&b.bounds[0]))
        });

        self.omitted = self.proposals.len().saturating_sub(MAX_PROPOSALS);
        self.proposals.truncate(MAX_PROPOSALS);
        Ok(&self.proposals)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn blank_and_edges_are_not_proposals() {
        let mut l = Localizer::default();
        for v in [0, 255] {
            let data = vec![v; 256 * 128];
            assert!(l
                .detect(ImageView::new(&data, 256, 128, 1, 256).unwrap())
                .unwrap()
                .is_empty());
        }
        let data: Vec<_> = (0..256 * 128)
            .map(|i| if i % 256 < 128 { 0 } else { 255 })
            .collect();
        assert!(l
            .detect(ImageView::new(&data, 256, 128, 1, 256).unwrap())
            .unwrap()
            .is_empty());
    }
    #[test]
    fn separated_horizontal_vertical_regions_and_stride() {
        let (w, h) = (320, 256);
        let mut data = vec![255; h * (w + 7)];
        for y in 24..56 {
            for x in 24..152 {
                data[y * (w + 7) + x] = if (x / 2) % 2 == 0 { 0 } else { 255 };
            }
        }
        for y in 80..224 {
            for x in 240..272 {
                data[y * (w + 7) + x] = if (y / 2) % 2 == 0 { 0 } else { 255 };
            }
        }
        let mut l = Localizer::default();
        let p = l
            .detect(ImageView::new(&data, w, h, 1, w + 7).unwrap())
            .unwrap();
        assert_eq!(p.len(), 2);
        assert!(p.iter().any(|p| p.bounds[0] <= 24.
            && p.bounds[2] >= 152.
            && p.bounds[1] <= 24.
            && p.bounds[3] >= 56.));
        assert!(p.iter().any(|p| p.bounds[0] <= 240.
            && p.bounds[2] >= 272.
            && p.bounds[1] <= 80.
            && p.bounds[3] >= 224.));
    }
}

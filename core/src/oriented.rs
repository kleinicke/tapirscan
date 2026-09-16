//! Experimental gradient-tensor proposals, not confirmed barcode instances.
#![forbid(unsafe_code)]
use crate::{
    sampling::{Error, ImageView},
    scan::Quad,
};
const TILE: usize = 8;
#[derive(Clone, Copy, Debug)]
pub struct Proposal {
    pub polygon: Quad,
    pub score: f64,
}
#[derive(Default)]
pub struct Localizer {
    gray: Vec<u8>,
    tensor: Vec<[f64; 3]>,
    active: Vec<u8>,
    closed: Vec<u8>,
    queue: Vec<usize>,
    proposals: Vec<Proposal>,
}
fn resize<T: Clone>(v: &mut Vec<T>, n: usize, x: T) -> Result<(), Error> {
    if n > v.len() {
        v.try_reserve_exact(n - v.len())
            .map_err(|_| Error::Allocation)?;
    }
    v.resize(n, x);
    Ok(())
}
fn direction(t: [f64; 3]) -> (f64, f64) {
    let a = 0.5 * (2. * t[2]).atan2(t[0] - t[1]);
    (a.cos(), a.sin())
}
impl Localizer {
    /// Up to12 ranked proposals in original pixels. No successful-read or label gate.
    /// # Errors
    /// Returns `Allocation` if localization scratch buffers cannot be reserved.
    #[expect(
        clippy::too_many_lines,
        reason = "Tensor construction, directional closing and connected components share one bounded tile grid."
    )]
    pub fn detect(&mut self, image: ImageView<'_>) -> Result<&[Proposal], Error> {
        self.proposals.clear();
        let step = image.width.max(image.height).div_ceil(1536);
        let working_width = image.width.div_ceil(step);
        let working_height = image.height.div_ceil(step);
        let tw = working_width.div_ceil(TILE);
        let th = working_height.div_ceil(TILE);
        let tile_count = tw * th;
        resize(&mut self.gray, working_width * working_height, 0)?;
        resize(&mut self.tensor, tile_count, [0.; 3])?;
        resize(&mut self.active, tile_count, 0)?;
        self.queue.clear();
        self.queue
            .try_reserve(tile_count)
            .map_err(|_| Error::Allocation)?;
        self.proposals
            .try_reserve(tile_count)
            .map_err(|_| Error::Allocation)?;
        for y in 0..working_height {
            for x in 0..working_width {
                let tile_index = y * step * image.stride + x * step * image.channels;
                self.gray[y * working_width + x] = if image.channels == 1 {
                    image.data[tile_index]
                } else {
                    ((77 * u32::from(image.data[tile_index])
                        + 150 * u32::from(image.data[tile_index + 1])
                        + 29 * u32::from(image.data[tile_index + 2]))
                        >> 8)
                        .to_le_bytes()[0]
                };
            }
        }
        for ty in 0..th {
            for tx in 0..tw {
                let mut t = [0.; 3];
                let (mut count, mut edges) = (0, 0);
                for y in (ty * TILE).max(1)..((ty + 1) * TILE).min(working_height.saturating_sub(1))
                {
                    for x in
                        (tx * TILE).max(1)..((tx + 1) * TILE).min(working_width.saturating_sub(1))
                    {
                        let dx = (f64::from(self.gray[y * working_width + x + 1])
                            - f64::from(self.gray[y * working_width + x - 1]))
                            * 0.5;
                        let dy = (f64::from(self.gray[(y + 1) * working_width + x])
                            - f64::from(self.gray[(y - 1) * working_width + x]))
                            * 0.5;
                        t[0] += dx * dx;
                        t[1] += dy * dy;
                        t[2] += dx * dy;
                        count += 1;
                        if dx * dx + dy * dy >= 24. * 24. {
                            edges += 1;
                        }
                    }
                }
                let trace = t[0] + t[1];
                let coherence = ((t[0] - t[1]).powi(2) + 4. * t[2] * t[2]).sqrt() / trace.max(1.);
                let tile_index = ty * tw + tx;
                self.tensor[tile_index] = t;
                self.active[tile_index] = u8::from(
                    count > 0
                        && trace > 144. * f64::from(count)
                        && coherence >= 0.65
                        && edges * 6 >= count,
                );
            }
        }
        // Bridge one missing tile only along a supported module direction.
        // Four-connected components otherwise fragment at wide EAN runs.
        resize(&mut self.closed, tile_count, 0)?;
        self.closed.copy_from_slice(&self.active);
        for y in 1..th.saturating_sub(1) {
            for x in 1..tw.saturating_sub(1) {
                let tile_index = y * tw + x;
                if self.active[tile_index] != 0 {
                    continue;
                }
                for (dx, dy) in [(1isize, 0isize), (0, 1), (1, 1), (1, -1)] {
                    let first_direction =
                        ((tile_index).cast_signed() - dy * (tw).cast_signed() - dx).cast_unsigned();
                    let second_direction =
                        ((tile_index).cast_signed() + dy * (tw).cast_signed() + dx).cast_unsigned();
                    if self.active[first_direction] == 0 || self.active[second_direction] == 0 {
                        continue;
                    }
                    let (cos_direction, sin_direction) = direction(self.tensor[first_direction]);
                    let (e, f) = direction(self.tensor[second_direction]);
                    let norm = crate::numeric::isize_f64(dx * dx + dy * dy).sqrt();
                    if (cos_direction * e + sin_direction * f).abs() >= 0.9
                        && ((cos_direction * crate::numeric::isize_f64(dx)
                            + sin_direction * crate::numeric::isize_f64(dy))
                            / norm)
                            .abs()
                            >= 0.85
                    {
                        self.closed[tile_index] = 1;
                        for k in 0..3 {
                            self.tensor[tile_index][k] = (self.tensor[first_direction][k]
                                + self.tensor[second_direction][k])
                                * 0.5;
                        }
                        break;
                    }
                }
            }
        }
        self.active.copy_from_slice(&self.closed);
        for start in 0..tile_count {
            if self.active[start] == 0 {
                continue;
            }
            self.queue.clear();
            self.queue.push(start);
            self.active[start] = 0;
            let mut head = 0;
            let mut sum = [0.; 3];
            while head < self.queue.len() {
                let tile_index = self.queue[head];
                head += 1;
                for (k, sum_entry) in sum.iter_mut().enumerate() {
                    (*sum_entry) += self.tensor[tile_index][k];
                }
                let (x, y) = (tile_index % tw, tile_index / tw);
                let (first_direction, second_direction) = direction(self.tensor[tile_index]);
                for next in [
                    if x > 0 { Some(tile_index - 1) } else { None },
                    if x + 1 < tw {
                        Some(tile_index + 1)
                    } else {
                        None
                    },
                    if y > 0 { Some(tile_index - tw) } else { None },
                    if y + 1 < th {
                        Some(tile_index + tw)
                    } else {
                        None
                    },
                ]
                .into_iter()
                .flatten()
                {
                    if self.active[next] == 0 {
                        continue;
                    }
                    let (cos_direction, sin_direction) = direction(self.tensor[next]);
                    if (first_direction * cos_direction + second_direction * sin_direction).abs()
                        < 0.9
                    {
                        continue;
                    }
                    self.active[next] = 0;
                    self.queue.push(next);
                }
            }
            if self.queue.len() < 6 {
                continue;
            }
            let (cos_direction, sine) = direction(sum);
            let (mut u0, mut u1, mut v0, mut v1) = (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            );
            for &tile_index in &self.queue {
                let x = crate::numeric::usize_f64(tile_index % tw * TILE);
                let y = crate::numeric::usize_f64(tile_index / tw * TILE);
                for (x, y) in [(x, y), (x + 8., y), (x + 8., y + 8.), (x, y + 8.)] {
                    let u = cos_direction * x + sine * y;
                    let v = -sine * x + cos_direction * y;
                    u0 = u0.min(u);
                    u1 = u1.max(u);
                    v0 = v0.min(v);
                    v1 = v1.max(v);
                }
            }
            let span = u1 - u0;
            let depth = v1 - v0;
            let density = crate::numeric::usize_f64(self.queue.len()) * 64. / (span * depth);
            if span < 48.
                || depth < 16.
                || span < depth * 0.4
                || span > depth * 25.
                || density < 0.3
            {
                continue;
            }
            let map = |u: f64, v: f64| {
                [
                    (cos_direction * u - sine * v) * crate::numeric::usize_f64(step),
                    (sine * u + cos_direction * v) * crate::numeric::usize_f64(step),
                ]
            };
            let polygon = [
                map(u0 - 8., v0 - 8.),
                map(u1 + 8., v0 - 8.),
                map(u1 + 8., v1 + 8.),
                map(u0 - 8., v1 + 8.),
            ];
            self.proposals.push(Proposal {
                polygon,
                score: density,
            });
        }
        self.proposals.sort_by(|first_direction, second_direction| {
            second_direction
                .score
                .total_cmp(&first_direction.score)
                .then_with(|| {
                    first_direction.polygon[0][1].total_cmp(&second_direction.polygon[0][1])
                })
                .then_with(|| {
                    first_direction.polygon[0][0].total_cmp(&second_direction.polygon[0][0])
                })
        });
        self.proposals.truncate(12);
        Ok(&self.proposals)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagonal_region_and_blank() {
        let (image_width, image_height) = (320, 320);
        let mut p = vec![255; image_width * image_height];
        for y in 0..image_height {
            for x in 0..image_width {
                let u = (crate::numeric::usize_f64(x) + crate::numeric::usize_f64(y) - 320.)
                    / 2f64.sqrt();
                let v = (crate::numeric::usize_f64(y) - crate::numeric::usize_f64(x)) / 2f64.sqrt();
                if u.abs() < 100.
                    && v.abs() < 24.
                    && crate::numeric::f64_i32(((u + 100.) / 4.).floor()) % 2 == 0
                {
                    p[y * image_width + x] = 0;
                }
            }
        }
        let mut l = Localizer::default();
        let r = l
            .detect(ImageView::new(&p, image_width, image_height, 1, image_width).unwrap())
            .unwrap();
        assert!(!r.is_empty());
        assert!(r.iter().any(|p| {
            let d = [
                p.polygon[1][0] - p.polygon[0][0],
                p.polygon[1][1] - p.polygon[0][1],
            ];
            (d[0] - d[1]).abs() < d[0].abs() * 0.2
        }));
        p.fill(255);
        assert!(l
            .detect(ImageView::new(&p, image_width, image_height, 1, image_width).unwrap())
            .unwrap()
            .is_empty());
    }
    #[test]
    fn tiny_images_and_stride() {
        let mut l = Localizer::default();
        for (w, h) in [(1, 1), (2, 20), (20, 2)] {
            let p = vec![127; (w + 7) * h];
            assert!(l
                .detect(ImageView::new(&p, w, h, 1, w + 7).unwrap())
                .unwrap()
                .is_empty());
        }
    }
}

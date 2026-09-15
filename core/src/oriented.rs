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
    pub fn detect(&mut self, image: ImageView<'_>) -> Result<&[Proposal], Error> {
        self.proposals.clear();
        let step = image.width.max(image.height).div_ceil(1536);
        let w = image.width.div_ceil(step);
        let h = image.height.div_ceil(step);
        let tw = w.div_ceil(TILE);
        let th = h.div_ceil(TILE);
        let n = tw * th;
        resize(&mut self.gray, w * h, 0)?;
        resize(&mut self.tensor, n, [0.; 3])?;
        resize(&mut self.active, n, 0)?;
        self.queue.clear();
        self.queue.try_reserve(n).map_err(|_| Error::Allocation)?;
        self.proposals
            .try_reserve(n)
            .map_err(|_| Error::Allocation)?;
        for y in 0..h {
            for x in 0..w {
                let i = y * step * image.stride + x * step * image.channels;
                self.gray[y * w + x] = if image.channels == 1 {
                    image.data[i]
                } else {
                    ((77 * u32::from(image.data[i])
                        + 150 * u32::from(image.data[i + 1])
                        + 29 * u32::from(image.data[i + 2]))
                        >> 8) as u8
                };
            }
        }
        for ty in 0..th {
            for tx in 0..tw {
                let mut t = [0.; 3];
                let (mut count, mut edges) = (0, 0);
                for y in (ty * TILE).max(1)..((ty + 1) * TILE).min(h.saturating_sub(1)) {
                    for x in (tx * TILE).max(1)..((tx + 1) * TILE).min(w.saturating_sub(1)) {
                        let dx = (f64::from(self.gray[y * w + x + 1])
                            - f64::from(self.gray[y * w + x - 1]))
                            * 0.5;
                        let dy = (f64::from(self.gray[(y + 1) * w + x])
                            - f64::from(self.gray[(y - 1) * w + x]))
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
                let i = ty * tw + tx;
                self.tensor[i] = t;
                self.active[i] = u8::from(
                    count > 0
                        && trace > 144. * f64::from(count)
                        && coherence >= 0.65
                        && edges * 6 >= count,
                );
            }
        }
        // Bridge one missing tile only along a supported module direction.
        // Four-connected components otherwise fragment at wide EAN runs.
        resize(&mut self.closed, n, 0)?;
        self.closed.copy_from_slice(&self.active);
        for y in 1..th.saturating_sub(1) {
            for x in 1..tw.saturating_sub(1) {
                let i = y * tw + x;
                if self.active[i] != 0 {
                    continue;
                }
                for (dx, dy) in [(1isize, 0isize), (0, 1), (1, 1), (1, -1)] {
                    let a = (i as isize - dy * tw as isize - dx) as usize;
                    let b = (i as isize + dy * tw as isize + dx) as usize;
                    if self.active[a] == 0 || self.active[b] == 0 {
                        continue;
                    }
                    let (c, d) = direction(self.tensor[a]);
                    let (e, f) = direction(self.tensor[b]);
                    let norm = ((dx * dx + dy * dy) as f64).sqrt();
                    if (c * e + d * f).abs() >= 0.9
                        && ((c * dx as f64 + d * dy as f64) / norm).abs() >= 0.85
                    {
                        self.closed[i] = 1;
                        for k in 0..3 {
                            self.tensor[i][k] = (self.tensor[a][k] + self.tensor[b][k]) * 0.5;
                        }
                        break;
                    }
                }
            }
        }
        self.active.copy_from_slice(&self.closed);
        for start in 0..n {
            if self.active[start] == 0 {
                continue;
            }
            self.queue.clear();
            self.queue.push(start);
            self.active[start] = 0;
            let mut head = 0;
            let mut sum = [0.; 3];
            while head < self.queue.len() {
                let i = self.queue[head];
                head += 1;
                for k in 0..3 {
                    sum[k] += self.tensor[i][k];
                }
                let (x, y) = (i % tw, i / tw);
                let (a, b) = direction(self.tensor[i]);
                for next in [
                    if x > 0 { Some(i - 1) } else { None },
                    if x + 1 < tw { Some(i + 1) } else { None },
                    if y > 0 { Some(i - tw) } else { None },
                    if y + 1 < th { Some(i + tw) } else { None },
                ]
                .into_iter()
                .flatten()
                {
                    if self.active[next] == 0 {
                        continue;
                    }
                    let (c, d) = direction(self.tensor[next]);
                    if (a * c + b * d).abs() < 0.9 {
                        continue;
                    }
                    self.active[next] = 0;
                    self.queue.push(next);
                }
            }
            if self.queue.len() < 6 {
                continue;
            }
            let (c, s) = direction(sum);
            let (mut u0, mut u1, mut v0, mut v1) = (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            );
            for &i in &self.queue {
                let x = (i % tw * TILE) as f64;
                let y = (i / tw * TILE) as f64;
                for (x, y) in [(x, y), (x + 8., y), (x + 8., y + 8.), (x, y + 8.)] {
                    let u = c * x + s * y;
                    let v = -s * x + c * y;
                    u0 = u0.min(u);
                    u1 = u1.max(u);
                    v0 = v0.min(v);
                    v1 = v1.max(v);
                }
            }
            let span = u1 - u0;
            let depth = v1 - v0;
            let density = self.queue.len() as f64 * 64. / (span * depth);
            if span < 48.
                || depth < 16.
                || span < depth * 0.4
                || span > depth * 25.
                || density < 0.3
            {
                continue;
            }
            let map =
                |u: f64, v: f64| [(c * u - s * v) * step as f64, (s * u + c * v) * step as f64];
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
        self.proposals.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.polygon[0][1].total_cmp(&b.polygon[0][1]))
                .then_with(|| a.polygon[0][0].total_cmp(&b.polygon[0][0]))
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
        let (w, h) = (320, 320);
        let mut p = vec![255; w * h];
        for y in 0..h {
            for x in 0..w {
                let u = (x as f64 + y as f64 - 320.) / 2f64.sqrt();
                let v = (y as f64 - x as f64) / 2f64.sqrt();
                if u.abs() < 100. && v.abs() < 24. && ((u + 100.) / 4.).floor() as i32 % 2 == 0 {
                    p[y * w + x] = 0;
                }
            }
        }
        let mut l = Localizer::default();
        let r = l.detect(ImageView::new(&p, w, h, 1, w).unwrap()).unwrap();
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
            .detect(ImageView::new(&p, w, h, 1, w).unwrap())
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

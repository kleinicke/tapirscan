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
    #[cfg(feature = "experimental-classical-wide-gaps")]
    basis: Vec<u8>,
    queue: Vec<usize>,
    proposals: Vec<Proposal>,
    #[cfg(feature = "experimental-classical-orientation")]
    refined: Vec<crate::oriented::Proposal>,
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
    /// Optional refinement of admitted proposals; uses the already prepared gray.
    #[cfg(feature = "experimental-classical-orientation")]
    pub fn oriented_bands(
        &mut self,
        image: ImageView<'_>,
    ) -> Result<&[crate::oriented::Proposal], Error> {
        self.detect(image)?;
        self.refined.clear();
        self.refined
            .try_reserve(MAX_PROPOSALS.saturating_sub(self.refined.capacity()))
            .map_err(|_| Error::Allocation)?;
        let step = image.width.max(image.height).div_ceil(MAX_SIDE);
        let w = image.width.div_ceil(step);
        let mut remaining = 4_000_000usize;
        for p in &self.proposals {
            let polygon = refine_orientation(
                &self.gray,
                w,
                image.width,
                image.height,
                step,
                p.bounds,
                &mut remaining,
                &mut self.work_limited,
            );
            self.refined.push(crate::oriented::Proposal {
                polygon,
                score: p.score,
            });
        }
        Ok(&self.refined)
    }
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
        #[cfg(feature = "experimental-classical-bands")]
        let mut band_budget = 4_000_000usize;
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
                        #[cfg(feature = "experimental-classical-span2")]
                        let dx = dx.max(
                            p.abs_diff(self.gray[y * working_width + x.saturating_sub(2)]) as u32,
                        );
                        #[cfg(feature = "experimental-classical-span2")]
                        let dy = dy.max(
                            p.abs_diff(self.gray[y.saturating_sub(2) * working_width + x]) as u32,
                        );
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
            #[cfg(feature = "experimental-classical-wide-gaps")]
            {
                resize(&mut self.basis, tiles, 0)?;
                self.basis.copy_from_slice(&self.closed);
                bridge_wide_gaps(
                    &self.mask,
                    &mut self.closed,
                    tw,
                    th,
                    axis,
                    Some((&self.gray, working_width, working_height)),
                );
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
                #[cfg(not(feature = "experimental-classical-wide-gaps"))]
                let count = self.queue.len();
                #[cfg(feature = "experimental-classical-wide-gaps")]
                let count = self.queue.iter().filter(|&&p| self.basis[p] > 0).count();
                let (span, depth) = if axis == 0 { (bw, bh) } else { (bh, bw) };
                if span < 6
                    || depth < 2
                    || span * 5 < depth * 6
                    || span > depth * 25
                    || count < 6
                    || count * 3 < bw * bh
                {
                    #[cfg(feature = "experimental-classical-trim")]
                    if span >= 6 && depth >= 2 && count >= 6 {
                        let trimmed: Vec<_> = component_bands(
                            {
                                #[cfg(feature = "experimental-classical-wide-gaps")]
                                {
                                    self.queue.retain(|&p| self.basis[p] > 0);
                                }
                                &self.queue
                            },
                            tw,
                            axis,
                            [x0, y0, x1 + 1, y1 + 1],
                        )
                        .into_iter()
                        .filter(|p| {
                            let b = [
                                (p.bounds[0] as usize + 1) * TILE,
                                (p.bounds[1] as usize + 1) * TILE,
                                ((p.bounds[2] as usize - 1) * TILE).min(working_width),
                                ((p.bounds[3] as usize - 1) * TILE).min(working_height),
                            ];
                            stationary_direction(
                                &self.gray,
                                working_width,
                                working_height,
                                axis,
                                b,
                                &mut band_budget,
                                &mut self.work_limited,
                            )
                        })
                        .collect();
                        if !trimmed.is_empty() {
                            self.proposals.extend(trimmed.into_iter().map(|mut p| {
                                for v in &mut p.bounds {
                                    *v *= (TILE * step) as f64;
                                }
                                p.bounds[2] = p.bounds[2].min(image.width as f64);
                                p.bounds[3] = p.bounds[3].min(image.height as f64);
                                p
                            }));
                            continue;
                        }
                    }
                    #[cfg(feature = "experimental-classical-bands")]
                    if span >= 6 && depth >= 2 && count >= 6 {
                        let bands = coherent_bands(
                            &self.gray,
                            working_width,
                            working_height,
                            axis,
                            [
                                x0 * TILE,
                                y0 * TILE,
                                ((x1 + 1) * TILE).min(working_width),
                                ((y1 + 1) * TILE).min(working_height),
                            ],
                            &mut band_budget,
                            &mut self.work_limited,
                        );
                        #[cfg(feature = "experimental-classical-edge-groups")]
                        let bands = if bands.is_empty() && span * 2 < depth {
                            directional_groups(
                                &self.gray,
                                working_width,
                                working_height,
                                0,
                                [
                                    x0 * TILE,
                                    y0 * TILE,
                                    ((x1 + 1) * TILE).min(working_width),
                                    ((y1 + 1) * TILE).min(working_height),
                                ],
                                &mut band_budget,
                                &mut self.work_limited,
                            )
                        } else {
                            bands
                        };
                        self.proposals.extend(bands.into_iter().map(|mut p| {
                            for v in &mut p.bounds {
                                *v *= step as f64;
                            }
                            p.bounds[2] = p.bounds[2].min(image.width as f64);
                            p.bounds[3] = p.bounds[3].min(image.height as f64);
                            p
                        }));
                    }
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
                #[cfg(feature = "experimental-classical-accepted-band")]
                let bounds = {
                    let working = bounds.map(|v| v as usize / step);
                    let refined = refine_accepted_band(
                        &self.gray,
                        working_width,
                        working_height,
                        axis,
                        working,
                        &mut band_budget,
                        &mut self.work_limited,
                    );
                    if refined == working {
                        bounds
                    } else {
                        refined.map(|v| v as f64 * step as f64)
                    }
                };
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
        #[cfg(feature = "experimental-classical-rank-direction")]
        if self.proposals.len() > MAX_PROPOSALS {
            // Only when admission is actually capped: prefer source-supported
            // directional persistence over density alone. Preserve confidence
            // scores and density order within each class; do not add candidates.
            let mut ranked = Vec::with_capacity(self.proposals.len());
            for p in self.proposals.drain(..) {
                let b = [
                    p.bounds[0] as usize / step + 8,
                    p.bounds[1] as usize / step + 8,
                    (p.bounds[2] as usize / step).saturating_sub(8),
                    (p.bounds[3] as usize / step).saturating_sub(8),
                ];
                #[cfg(feature = "experimental-classical-rank-core")]
                let b = {
                    // Evaluate the interior signal, excluding attached captions
                    // where geometry is broad. Keep at least 48 working pixels
                    // per dimension; decode geometry itself never changes.
                    let dx =
                        ((b[2].saturating_sub(b[0])) / 4).min(b[2].saturating_sub(b[0] + 48) / 2);
                    let dy =
                        ((b[3].saturating_sub(b[1])) / 4).min(b[3].saturating_sub(b[1] + 48) / 2);
                    [b[0] + dx, b[1] + dy, b[2] - dx, b[3] - dy]
                };
                let means = directional_persistence(
                    &self.gray,
                    working_width,
                    working_height,
                    b,
                    &mut band_budget,
                    &mut self.work_limited,
                );
                let supported = means.is_some_and(|m| m[0] > 2. * m[1] || m[1] > 2. * m[0]);
                ranked.push((!supported, p));
            }
            ranked.sort_by_key(|p| p.0);
            self.proposals.extend(ranked.into_iter().map(|p| p.1));
        }
        #[cfg(feature = "experimental-classical-global-groups")]
        {
            // A separate bounded pass can recover stripes absent from the axis mask.
            // Preserve every existing candidate's admission priority.
            let mut remaining = 4_000_000;
            let groups = global_cached_groups(
                &self.gray,
                working_width,
                working_height,
                &mut remaining,
                &mut self.work_limited,
            );
            #[cfg(feature = "experimental-classical-crossband")]
            let mut alternatives = Vec::new();
            for group in groups {
                for mut p in group {
                    for v in &mut p.bounds {
                        *v *= step as f64;
                    }
                    p.bounds[2] = p.bounds[2].min(image.width as f64);
                    p.bounds[3] = p.bounds[3].min(image.height as f64);
                    let mut redundant = false;
                    #[cfg(feature = "experimental-classical-crossband")]
                    let mut alternative = true;
                    for quad in &self.proposals {
                        let a = p.bounds;
                        let b = quad.bounds;
                        let intersection = (a[2].min(b[2]) - a[0].max(b[0])).max(0.)
                            * (a[3].min(b[3]) - a[1].max(b[1])).max(0.);
                        let small =
                            ((a[2] - a[0]) * (a[3] - a[1])).min((b[2] - b[0]) * (b[3] - b[1]));
                        if intersection >= 0.8 * small {
                            redundant = true;
                            #[cfg(feature = "experimental-classical-crossband")]
                            {
                                alternative &= crossband_alternative(a, b);
                            }
                        }
                    }
                    if !redundant {
                        self.proposals.push(p);
                    }
                    #[cfg(feature = "experimental-classical-crossband")]
                    if redundant && alternative {
                        alternatives.push(p);
                    }
                }
            }
            // Preserve all original global-group admission priority before any
            // new cross-band alternatives; cap omissions remain explicit.
            #[cfg(feature = "experimental-classical-crossband")]
            self.proposals.extend(alternatives);
        }
        self.omitted = self.proposals.len().saturating_sub(MAX_PROPOSALS);
        self.proposals.truncate(MAX_PROPOSALS);
        Ok(&self.proposals)
    }
}
/// Join wide-module gaps only in repeated, cross-row-supported edge trains.
/// The raw mask is immutable: newly joined tiles cannot seed another bridge.
/// These tiles connect geometry only; original one-gap mask determines scores.
#[cfg(feature = "experimental-classical-wide-gaps")]
fn bridge_wide_gaps(
    mask: &[u8],
    closed: &mut [u8],
    tw: usize,
    th: usize,
    axis: usize,
    evidence: Option<(&[u8], usize, usize)>,
) {
    let _ = evidence;
    let (span, depth) = if axis == 0 { (tw, th) } else { (th, tw) };
    if depth < 3 {
        return;
    }
    #[cfg(feature = "experimental-classical-stable-components")]
    let protected = stable_components(closed, tw, th, axis, evidence);
    let index = |u: usize, v: usize| if axis == 0 { v * tw + u } else { u * tw + v };
    for v in 1..depth - 1 {
        let mut islands = vec![];
        let mut u = 0;
        while u < span {
            if mask[index(u, v)] == 0 {
                u += 1;
                continue;
            }
            let start = u;
            u += 1;
            while u < span && mask[index(u, v)] > 0 {
                u += 1;
            }
            islands.push((start, u));
        }
        if islands.len() < 8 {
            continue;
        }
        for i in 0..islands.len() - 1 {
            let (left, right) = (islands[i].1, islands[i + 1].0);
            let gap = right - left;
            if !(2..=5).contains(&gap) {
                continue;
            }
            #[cfg(feature = "experimental-classical-stable-components")]
            if protected[index(left - 1, v)] || protected[index(right, v)] {
                continue;
            }
            let lo = i.saturating_sub(4).min(islands.len().saturating_sub(10));
            let hi = (lo + 10).min(islands.len());
            if hi - lo < 8 || islands[hi - 1].1 - islands[lo].0 > 96 {
                continue;
            }
            let mut gaps: Vec<_> = islands[lo..hi]
                .windows(2)
                .map(|p| p[1].0 - p[0].1)
                .filter(|&g| g <= 5)
                .collect();
            if gaps.len() < 6 {
                continue;
            }
            gaps.sort_unstable();
            if gap > 4 * gaps[gaps.len() / 2] {
                continue;
            }
            let supported = |vv: usize, edge: usize| {
                ((edge.saturating_sub(1))..=(edge + 1).min(span - 1))
                    .any(|x| mask[index(x, vv)] > 0)
            };
            if [v - 1, v + 1]
                .iter()
                .all(|&vv| supported(vv, left - 1) && supported(vv, right))
            {
                for x in left..right {
                    closed[index(x, v)] = 1;
                }
            }
        }
    }
}
/// Two independently localized extents can place cheap rows on different source
/// bands even when their rectangles overlap. Keep only comparable reading spans
/// with a material cross-band shift; smaller internal fragments remain suppressed.
#[cfg(feature = "experimental-classical-crossband")]
fn crossband_alternative(a: [f64; 4], b: [f64; 4]) -> bool {
    let axis = usize::from(a[3] - a[1] > a[2] - a[0]);
    let other = 1 - axis;
    let reading = a[axis + 2] - a[axis];
    let parent = b[axis + 2] - b[axis];
    let cross = a[other + 2] - a[other];
    let parent_cross = b[other + 2] - b[other];
    if reading < 0.8 * parent
        || reading > 1.25 * parent
        || cross < 0.5 * parent_cross
        || cross > 1.5 * parent_cross
    {
        return false;
    }
    let overlap = a[axis + 2].min(b[axis + 2]) - a[axis].max(b[axis]);
    let change = (a[other] - b[other])
        .abs()
        .max((a[other + 2] - b[other + 2]).abs());
    overlap >= 0.8 * reading.max(parent) && change >= 0.1 * cross.max(parent_cross)
}

/// Identical central-difference tensor and geometric gates to024 TS prototype.
/// Four million working sample positions are shared across admitted proposals.
#[cfg(feature = "experimental-classical-orientation")]
fn refine_orientation(
    gray: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    step: usize,
    b: [f64; 4],
    remaining: &mut usize,
    limited: &mut bool,
) -> crate::scan::Quad {
    let [x0, y0, x1, y1] = b;
    let (w, h) = (x1 - x0, y1 - y0);
    let q = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let sf = step as f64;
    let lx = ((x0 + 0.15 * w) / sf).ceil().max(1.) as usize;
    let rx = ((width.saturating_sub(step) as f64).min(x1 - 0.15 * w) / sf)
        .ceil()
        .max(0.) as usize;
    let ly = ((y0 + 0.15 * h) / sf).ceil().max(1.) as usize;
    let ry = ((height.saturating_sub(step) as f64).min(y1 - 0.15 * h) / sf)
        .ceil()
        .max(0.) as usize;
    let n = rx.saturating_sub(lx) * ry.saturating_sub(ly);
    if n > *remaining {
        *limited = true;
        return q;
    }
    *remaining -= n;
    if n < 64 {
        return q;
    }
    let (mut xx, mut yy, mut xy) = (0f64, 0f64, 0f64);
    #[cfg(feature = "experimental-classical-consistent-angle")]
    let mut cells = [[0f64; 4]; 6];
    for y in ly..ry {
        for x in lx..rx {
            let dx = (gray[y * stride + x + 1] as f64 - gray[y * stride + x - 1] as f64) * 0.5;
            let dy = (gray[(y + 1) * stride + x] as f64 - gray[(y - 1) * stride + x] as f64) * 0.5;
            xx += dx * dx;
            yy += dy * dy;
            xy += dx * dy;
            #[cfg(feature = "experimental-classical-consistent-angle")]
            {
                let k = ((y - ly) * 2 / (ry - ly)) * 3 + (x - lx) * 3 / (rx - lx);
                cells[k][0] += dx * dx;
                cells[k][1] += dy * dy;
                cells[k][2] += dx * dy;
                cells[k][3] += 1.;
            }
        }
    }
    let coherence = (xx - yy).hypot(2. * xy) / (xx + yy).max(1.);
    let angle = 0.5 * (2. * xy).atan2(xx - yy);
    let pi = std::f64::consts::PI;
    let delta = (angle + pi / 4. + pi) % (pi / 2.) - pi / 4.;
    let (reading, cross) = if angle.abs() <= pi / 4. {
        (w, h)
    } else {
        (h, w)
    };
    let drift = delta.abs().tan() * reading / cross.max(1.);
    let supported_angle = delta.abs() <= 20. * pi / 180.;
    #[cfg(feature = "experimental-classical-consistent-angle")]
    let supported_angle = {
        let mut supported_angle = supported_angle;
        if !supported_angle && delta.abs() <= 35. * pi / 180. {
            // Six spatial cells must each exhibit the same strong direction. Reusing
            // sampled gradients preserves the existing source-position work budget.
            supported_angle = cells.iter().all(|&[a, b, c, n]| {
                let local = 0.5 * (2. * c).atan2(a - b);
                let difference = (local - angle).sin().abs().asin();
                n >= 64.
                    && (a + b) / n >= 144.
                    && (a - b).hypot(2. * c) / (a + b).max(1.) >= 0.8
                    && difference <= 5. * pi / 180.
            });
        }
        supported_angle
    };
    if drift <= 0.25
        || (xx + yy) / (n as f64) < 144.
        || coherence < 0.5
        || delta.abs() < 3. * pi / 180.
        || !supported_angle
    {
        return q;
    }
    let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let (c, s) = (delta.cos(), delta.sin());
    let out = q.map(|[x, y]| {
        [
            cx + c * (x - cx) - s * (y - cy),
            cy + s * (x - cx) + c * (y - cy),
        ]
    });
    if out
        .iter()
        .any(|p| p[0] < 0. || p[1] < 0. || p[0] > width as f64 || p[1] > height as f64)
    {
        q
    } else {
        out
    }
}

/// Mark accepted one-gap regions as ineligible wider-bridge endpoints.
/// This uses the original geometry gates; no union of old/new proposals.
#[cfg(feature = "experimental-classical-stable-components")]
fn stable_components(
    closed: &[u8],
    tw: usize,
    th: usize,
    axis: usize,
    evidence: Option<(&[u8], usize, usize)>,
) -> Vec<bool> {
    let _ = evidence;
    let mut seen = closed.to_vec();
    let mut protected = vec![false; seen.len()];
    let mut queue = vec![];
    for start in 0..seen.len() {
        if seen[start] == 0 {
            continue;
        }
        queue.clear();
        queue.push(start);
        seen[start] = 0;
        let (mut x0, mut x1, mut y0, mut y1) = (start % tw, start % tw, start / tw, start / tw);
        let mut head = 0;
        while head < queue.len() {
            let p = queue[head];
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
                if seen[next] > 0 {
                    seen[next] = 0;
                    queue.push(next);
                }
            }
        }
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let (span, depth) = if axis == 0 { (bw, bh) } else { (bh, bw) };
        let count = queue.len();
        if span >= 6
            && depth >= 2
            && span * 5 >= depth * 6
            && span <= depth * 25
            && count >= 6
            && count * 3 >= bw * bh
        {
            #[cfg(feature = "experimental-classical-complete-components")]
            if let Some((gray, w, h)) = evidence {
                if !complete_edge_support(
                    gray,
                    w,
                    h,
                    axis,
                    [
                        x0.saturating_sub(1) * TILE,
                        y0.saturating_sub(1) * TILE,
                        ((x1 + 2) * TILE).min(w),
                        ((y1 + 2) * TILE).min(h),
                    ],
                ) {
                    continue;
                }
            }
            for &p in &queue {
                protected[p] = true;
            }
        }
    }
    protected
}
/// Structural completeness for protection only; never a decoder acceptance.
/// Inspect five existing source rows, requiring at least three with enough
/// observed transitions to plausibly span a full EAN train. No digit tables.
#[cfg(feature = "experimental-classical-complete-components")]
fn complete_edge_support(gray: &[u8], w: usize, h: usize, axis: usize, b: [usize; 4]) -> bool {
    complete_edge_support_limit(gray, w, h, axis, b, 110)
}
#[cfg(feature = "experimental-classical-complete-components")]
fn complete_edge_support_limit(
    gray: &[u8],
    w: usize,
    h: usize,
    axis: usize,
    b: [usize; 4],
    maximum: usize,
) -> bool {
    let (u0, v0, u1, v1) = if axis == 0 {
        (b[0], b[1], b[2], b[3])
    } else {
        (b[1], b[0], b[3], b[2])
    };
    let _ = h;
    if u1 <= u0 + 1 || v1 <= v0 {
        return false;
    }
    let mut supported = 0;
    for numerator in [2, 3, 4, 5, 6] {
        let v = v0 + (v1 - v0) * numerator / 8;
        let pixel = |u: usize| {
            if axis == 0 {
                gray[v * w + u]
            } else {
                gray[u * w + v]
            }
        };
        let (mut lo, mut hi) = (255u8, 0u8);
        for u in u0..u1 {
            let p = pixel(u);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if hi - lo < 40 {
            continue;
        }
        let mid = (lo as u16 + hi as u16) / 2;
        let count = (u0 + 1..u1)
            .filter(|&u| (pixel(u - 1) as u16 >= mid) != (pixel(u) as u16 >= mid))
            .count();
        supported += usize::from((56..=maximum).contains(&count));
    }
    supported >= 3
}
/// A barcode's signed edge pattern persists along bars, but shifts across
/// modules. Require directional persistence rather than repetition in both
/// directions (common in text). Used only for newly trimmed components.
#[cfg(feature = "experimental-classical-trim")]
fn stationary_direction(
    gray: &[u8],
    w: usize,
    h: usize,
    axis: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
) -> bool {
    directional_persistence(gray, w, h, b, budget, limited)
        .is_some_and(|m| m[axis] > 2. * m[1 - axis])
}
#[cfg(feature = "experimental-classical-trim")]
fn directional_persistence(
    gray: &[u8],
    w: usize,
    h: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
) -> Option<[f64; 2]> {
    let _ = h;
    let mut means = [0.; 2];
    for direction in 0..2 {
        let (u0, v0, u1, v1) = if direction == 0 {
            (b[0], b[1], b[2], b[3])
        } else {
            (b[1], b[0], b[3], b[2])
        };
        let span = u1.saturating_sub(u0);
        if span < 6 || v1.saturating_sub(v0) < 12 {
            return None;
        }
        let mut previous = vec![0.; span - 1];
        let mut row = vec![0.; span - 1];
        let (mut rows, mut sum) = (0, 0.);
        for v in (v0 + 2..v1 - 2).step_by(4) {
            if *budget < span {
                *limited = true;
                return None;
            }
            *budget -= span;
            let pixel = |u: usize| {
                if direction == 0 {
                    gray[v * w + u]
                } else {
                    gray[u * w + v]
                }
            };
            for u in u0 + 1..u1 {
                row[u - u0 - 1] = pixel(u) as f64 - pixel(u - 1) as f64;
            }
            if rows > 0 {
                let mut corr = 0f64;
                for shift in -2isize..=2 {
                    let (mut dot, mut aa, mut bb) = (0., 0., 0.);
                    for i in 2..span - 3 {
                        let (a, b) = (previous[i], row[(i as isize + shift) as usize]);
                        dot += a * b;
                        aa += a * a;
                        bb += b * b;
                    }
                    corr = corr.max(dot / (aa * bb).sqrt().max(1.));
                }
                sum += corr;
            }
            rows += 1;
            std::mem::swap(&mut previous, &mut row);
        }
        if rows < 3 {
            return None;
        }
        means[direction] = sum / (rows - 1) as f64;
    }
    Some(means)
}
/// Trim sparse attachment rows in a rejected connected component. A band
/// needs two adjacent rows with at least half the component's peak support;
/// accepted bounds still satisfy the original aspect/density gates.
#[cfg(feature = "experimental-classical-trim")]
fn component_bands(points: &[usize], tw: usize, axis: usize, b: [usize; 4]) -> Vec<Proposal> {
    let (v0, v1) = if axis == 0 {
        (b[1], b[3])
    } else {
        (b[0], b[2])
    };
    let mut rows = vec![0usize; v1 - v0];
    for &p in points {
        let v = if axis == 0 { p / tw } else { p % tw };
        rows[v - v0] += 1;
    }
    let peak = rows.iter().copied().max().unwrap_or(0);
    if peak < 6 {
        return vec![];
    }
    let mut out = vec![];
    let mut i = 0;
    while i < rows.len() {
        if rows[i] * 2 < peak || rows[i] < 6 {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < rows.len() && rows[i] * 2 >= peak && rows[i] >= 6 {
            i += 1;
        }
        if i - start < 2 {
            continue;
        }
        let (mut u0, mut u1, mut count) = (usize::MAX, 0, 0);
        for &p in points {
            let (u, v) = if axis == 0 {
                (p % tw, p / tw)
            } else {
                (p / tw, p % tw)
            };
            if v >= v0 + start && v < v0 + i {
                u0 = u0.min(u);
                u1 = u1.max(u + 1);
                count += 1;
            }
        }
        let (span, depth) = (u1 - u0, i - start);
        if span < 6 || span * 5 < depth * 6 || span > depth * 25 || count * 3 < span * depth {
            continue;
        }
        let c = [
            u0.saturating_sub(1),
            (v0 + start).saturating_sub(1),
            u1 + 1,
            v0 + i + 1,
        ];
        let bounds = if axis == 0 {
            c
        } else {
            [c[1], c[0], c[3], c[2]]
        }
        .map(|v| v as f64);
        out.push(Proposal {
            bounds,
            score: count as f64 / (span * depth) as f64,
        });
    }
    out
}
/// Refine a caption-attached accepted component only when one coherent band
/// spans most of its reading width. Multiple bands retain the whole proposal.
/// This changes localization geometry only; no digit/checksum evidence.
#[cfg(feature = "experimental-classical-accepted-band")]
fn refine_accepted_band(
    gray: &[u8],
    w: usize,
    h: usize,
    axis: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
) -> [usize; 4] {
    if *limited {
        return b;
    }
    let bands = coherent_bands_impl(gray, w, h, axis, b, budget, limited, true);
    if *limited || bands.len() != 1 {
        return b;
    }
    let mut p = bands[0].bounds.map(|v| v as usize);
    for i in 0..2 {
        p[i] = p[i].max(b[i]);
        p[i + 2] = p[i + 2].min(b[i + 2]);
    }
    let (u, v) = if axis == 0 { (0, 1) } else { (1, 0) };
    // Persistent gradient columns separate the bar train from adjacent numbers.
    let Some((left, right)) = persistent_extent(gray, w, axis, p, budget, limited) else {
        return b;
    };
    p[u] = left.saturating_sub(8).max(b[u]);
    p[u + 2] = (right + 9).min(b[u + 2]);
    let span = b[u + 2] - b[u];
    let depth = b[v + 2] - b[v];
    if p[u] < b[u]
        || p[u + 2] > b[u + 2]
        || p[v] < b[v]
        || p[v + 2] > b[v + 2]
        || (p[u + 2] - p[u]) * 2 < span
        || (p[v + 2] - p[v]) * 4 > depth * 3
    {
        return b;
    }
    let cost = 5 * (p[u + 2] - p[u]);
    if *budget < cost {
        *limited = true;
        return b;
    }
    *budget -= cost;
    // Do not shorten a code into a coherent partial train. EAN-13 has 60
    // observed black/white boundaries; allow limited noise, no synthesized edges.
    if !complete_edge_support_limit(gray, w, h, axis, p, 66) {
        return b;
    }
    p
}
#[cfg(feature = "experimental-classical-accepted-band")]
fn persistent_extent(
    gray: &[u8],
    w: usize,
    axis: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
) -> Option<(usize, usize)> {
    let (u, v) = if axis == 0 { (0, 1) } else { (1, 0) };
    let (u0, u1, v0, v1) = (b[u], b[u + 2], b[v] + 4, b[v + 2].saturating_sub(8));
    if u1 <= u0 + 4 || v1 <= v0 + 7 {
        return None;
    }
    let n = v1 - v0;
    let mut support = vec![0usize; u1 - u0];
    let mut widths = vec![];
    for y in v0..v1 {
        if *budget < u1 - u0 {
            *limited = true;
            return None;
        }
        *budget -= u1 - u0;
        let pixel = |x: usize| {
            if axis == 0 {
                gray[y * w + x]
            } else {
                gray[x * w + y]
            }
        };
        let (mut lo, mut hi) = (255u8, 0u8);
        for x in u0..u1 {
            lo = lo.min(pixel(x));
            hi = hi.max(pixel(x));
        }
        let mid = (lo as u16 + hi as u16) / 2;
        let mut last = None;
        for x in u0 + 1..u1 {
            if (pixel(x) as u16 >= mid) != (pixel(x - 1) as u16 >= mid) {
                if let Some(prev) = last {
                    widths.push(x - prev);
                }
                last = Some(x);
            }
        }
        for x in u0 + 2..u1 - 1 {
            if (x - 1..=x + 1).any(|xx| pixel(xx).abs_diff(pixel(xx - 1)) >= 24) {
                support[x - u0] += 1;
            }
        }
    }
    if widths.len() < 56 {
        return None;
    }
    widths.sort_unstable();
    let pitch = widths[widths.len() / 4];
    let gap = (4 * pitch).saturating_sub(2).max(4);
    let points: Vec<_> = support
        .iter()
        .enumerate()
        .filter(|(_, c)| **c * 4 >= n * 3)
        .map(|(i, _)| u0 + i)
        .collect();
    let mut groups = vec![];
    let mut start = 0;
    for end in 1..=points.len() {
        if end == points.len() || points[end] - points[end - 1] > gap {
            if end - start >= 24 && points[end - 1] - points[start] >= 48 {
                groups.push((points[start], points[end - 1]));
            }
            start = end;
        }
    }
    if groups.len() == 1 {
        Some(groups[0])
    } else {
        None
    }
}
/// Recover only repeated edge bands inside geometrically rejected components.
/// Adjacent signed gradients must agree after at most two working-pixel shift;
/// blank gaps and text rows break the band. No digits/checksum enter this model.
#[cfg(feature = "experimental-classical-bands")]
fn coherent_bands(
    gray: &[u8],
    w: usize,
    h: usize,
    axis: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
) -> Vec<Proposal> {
    coherent_bands_impl(gray, w, h, axis, b, budget, limited, false)
}
#[cfg(feature = "experimental-classical-bands")]
fn coherent_bands_impl(
    gray: &[u8],
    w: usize,
    h: usize,
    axis: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
    strict: bool,
) -> Vec<Proposal> {
    let stride = if strict { 1 } else { 4 };
    let minimum_rows = if strict { 8 } else { 4 };
    let (u0, v0, u1, v1) = if axis == 0 {
        (b[0], b[1], b[2], b[3])
    } else {
        (b[1], b[0], b[3], b[2])
    };
    let span = u1 - u0;
    if span < 48 || v1 - v0 < 16 {
        return vec![];
    }
    let mut out = vec![];
    let mut previous = vec![0f64; span];
    let mut row = vec![0f64; span];
    let (mut start, mut last, mut rows, mut xmin, mut xmax, mut quality) =
        (v0, v0, 0usize, u1, u0, 0f64);
    let finish = |out: &mut Vec<Proposal>,
                  start: usize,
                  last: usize,
                  rows: usize,
                  xmin: usize,
                  xmax: usize,
                  quality: f64| {
        if rows < minimum_rows || xmax.saturating_sub(xmin) < 48 {
            return;
        }
        let depth = last - start + stride;
        if depth < (if strict { 8 } else { 16 }) || depth > span * 3 {
            return;
        }
        let coords = [
            xmin.saturating_sub(8),
            start.saturating_sub(4),
            (xmax + 9).min(if axis == 0 { w } else { h }),
            (last + 8).min(if axis == 0 { h } else { w }),
        ];
        let bounds = if axis == 0 {
            coords
        } else {
            [coords[1], coords[0], coords[3], coords[2]]
        }
        .map(|x| x as f64);
        out.push(Proposal {
            bounds,
            score: (quality / (rows - 1) as f64).min(1.) * 0.8,
        });
    };
    for v in (v0 + 2..v1.saturating_sub(2)).step_by(stride) {
        if *budget < span {
            *limited = true;
            break;
        }
        *budget -= span;
        let pixel = |u: usize| {
            if axis == 0 {
                gray[v * w + u]
            } else {
                gray[u * w + v]
            }
        };
        let (mut lo, mut hi) = (255u8, 0u8);
        for u in u0..u1 {
            let p = pixel(u);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let mid = (lo as u16 + hi as u16) / 2;
        let (mut crossings, mut first, mut end) = (0usize, u1, u0);
        row.fill(0.);
        for u in u0 + 1..u1 {
            let a = pixel(u - 1);
            let b = pixel(u);
            row[u - u0] = b as f64 - a as f64;
            if (a as u16 >= mid) != (b as u16 >= mid) {
                crossings += 1;
                first = first.min(u);
                end = end.max(u);
            }
        }
        let valid = hi.saturating_sub(lo) >= 40
            && (if strict { 56..=110 } else { 24..=150 }).contains(&crossings)
            && end.saturating_sub(first) >= 48;
        let mut corr = 0f64;
        if valid && rows > 0 {
            for shift in -2isize..=2 {
                let (mut dot, mut aa, mut bb) = (0., 0., 0.);
                for i in 2..span - 2 {
                    let (a, b) = (previous[i], row[(i as isize + shift) as usize]);
                    dot += a * b;
                    aa += a * a;
                    bb += b * b;
                }
                corr = corr.max(dot / (aa * bb).sqrt().max(1.));
            }
        }
        if !valid || rows > 0 && corr < (if strict { 0.90 } else { 0.80 }) {
            finish(&mut out, start, last, rows, xmin, xmax, quality);
            rows = 0;
            xmin = u1;
            xmax = u0;
            quality = 0.;
        }
        if valid {
            if rows == 0 {
                start = v;
            } else {
                quality += corr;
            }
            last = v;
            rows += 1;
            xmin = xmin.min(first);
            xmax = xmax.max(end);
            std::mem::swap(&mut previous, &mut row);
        }
    }
    finish(&mut out, start, last, rows, xmin, xmax, quality);
    out
}
/// Split rejected broad components by locally consistent gradient direction.
#[cfg(feature = "experimental-classical-edge-groups")]
fn directional_groups(
    g: &[u8],
    w: usize,
    h: usize,
    axis: usize,
    b: [usize; 4],
    budget: &mut usize,
    limited: &mut bool,
) -> Vec<Proposal> {
    let pixel = |u: usize, v: usize| {
        if axis == 0 {
            g[v * w + u]
        } else {
            g[u * w + v]
        }
    };
    let (b, width, height) = if axis == 0 {
        (b, w, h)
    } else {
        ([b[1], b[0], b[3], b[2]], h, w)
    };
    let tw = (b[2] - b[0]) / 8;
    let th = (b[3] - b[1]) / 8;
    let mut angles = vec![None; tw * th];
    for ty in 0..th {
        for tx in 0..tw {
            if *budget < 64 {
                *limited = true;
                return vec![];
            }
            *budget -= 64;
            let x = b[0] + tx * 8;
            let y = b[1] + ty * 8;
            let (mut xx, mut yy, mut xy, mut edges) = (0f64, 0f64, 0f64, 0usize);
            for v in y + 1..y + 7 {
                for u in x + 1..x + 7 {
                    let dx = (pixel(u + 1, v) as f64 - pixel(u - 1, v) as f64) * 0.5;
                    let dy = (pixel(u, v + 1) as f64 - pixel(u, v - 1) as f64) * 0.5;
                    xx += dx * dx;
                    yy += dy * dy;
                    xy += dx * dy;
                    if dx.abs() >= 24. {
                        edges += 1;
                    }
                }
            }
            if (xx - yy).hypot(2. * xy) / (xx + yy).max(1.) >= 0.65
                && (xx + yy) / 36. >= 144.
                && edges >= 6
            {
                angles[ty * tw + tx] = Some(0.5 * (2. * xy).atan2(xx - yy));
            }
        }
    }
    grouped_tiles(&angles, tw, th, axis, b, width, height)
}
#[cfg(feature = "experimental-classical-edge-groups")]
fn grouped_tiles(
    angles: &[Option<f64>],
    tw: usize,
    th: usize,
    axis: usize,
    b: [usize; 4],
    width: usize,
    height: usize,
) -> Vec<Proposal> {
    let mut seen = vec![false; tw * th];
    let mut out = vec![];
    for start in 0..tw * th {
        if seen[start] || angles[start].is_none() {
            continue;
        }
        let mut queue = vec![start];
        seen[start] = true;
        let (mut x0, mut y0, mut x1, mut y1) = (tw, th, 0, 0);
        let mut i = 0;
        while i < queue.len() {
            let p = queue[i];
            i += 1;
            let (x, y) = (p % tw, p / tw);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            for n in [
                if x > 0 { Some(p - 1) } else { None },
                if x + 1 < tw { Some(p + 1) } else { None },
                if y > 0 { Some(p - tw) } else { None },
                if y + 1 < th { Some(p + tw) } else { None },
            ]
            .into_iter()
            .flatten()
            {
                if !seen[n]
                    && angles[n]
                        .is_some_and(|a| (a - angles[p].unwrap()).sin().abs() < 0.2588190451)
                {
                    seen[n] = true;
                    queue.push(n);
                }
            }
        }
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        if queue.len() >= 6 && bw >= 6 && bh >= 2 && bw >= bh * 2 && queue.len() * 3 >= bw * bh {
            let bounds = [
                (b[0] + x0 * 8).saturating_sub(8) as f64,
                (b[1] + y0 * 8).saturating_sub(8) as f64,
                (b[0] + (x1 + 1) * 8 + 8).min(width) as f64,
                (b[1] + (y1 + 1) * 8 + 8).min(height) as f64,
            ];
            out.push(Proposal {
                bounds: if axis == 0 {
                    bounds
                } else {
                    [bounds[1], bounds[0], bounds[3], bounds[2]]
                },
                score: 0.8 * queue.len() as f64 / (bw * bh) as f64,
            });
        }
    }
    out
}
/// Compute the same two directional masks from one traversal of source gradients.
#[cfg(feature = "experimental-classical-global-groups")]
fn global_cached_groups(
    g: &[u8],
    w: usize,
    h: usize,
    budget: &mut usize,
    limited: &mut bool,
) -> [Vec<Proposal>; 2] {
    let (tw, th) = (w / 8, h / 8);
    let cost = tw * th * 64;
    if *budget < cost {
        *limited = true;
        return [vec![], vec![]];
    }
    *budget -= cost;
    let second = *budget >= cost;
    if second {
        *budget -= cost
    } else {
        *limited = true;
        *budget = 0;
    }
    let mut a = vec![None; tw * th];
    let mut b = if second { vec![None; tw * th] } else { vec![] };
    for ty in 0..th {
        for tx in 0..tw {
            let (x, y) = (tx * 8, ty * 8);
            let (mut xx, mut yy, mut xy, mut ex, mut ey) = (0f64, 0f64, 0f64, 0usize, 0usize);
            for v in y + 1..y + 7 {
                for u in x + 1..x + 7 {
                    let dx = (g[v * w + u + 1] as f64 - g[v * w + u - 1] as f64) * 0.5;
                    let dy = (g[(v + 1) * w + u] as f64 - g[(v - 1) * w + u] as f64) * 0.5;
                    xx += dx * dx;
                    yy += dy * dy;
                    xy += dx * dy;
                    ex += usize::from(dx.abs() >= 24.);
                    ey += usize::from(dy.abs() >= 24.);
                }
            }
            if (xx - yy).hypot(2. * xy) / (xx + yy).max(1.) >= 0.65 && (xx + yy) / 36. >= 144. {
                if ex >= 6 {
                    a[ty * tw + tx] = Some(0.5 * (2. * xy).atan2(xx - yy));
                }
                if second && ey >= 6 {
                    b[tx * th + ty] = Some(0.5 * (2. * xy).atan2(yy - xx));
                }
            }
        }
    }
    [
        grouped_tiles(&a, tw, th, 0, [0, 0, w, h], w, h),
        if second {
            grouped_tiles(&b, th, tw, 1, [0, 0, h, w], h, w)
        } else {
            vec![]
        },
    ]
}
#[cfg(test)]
mod tests {
    #[cfg(feature = "experimental-classical-global-groups")]
    #[test]
    fn cached_masks_match_serial_axes_with_partial_budgets() {
        let (w, h) = (640, 160);
        let mut p = vec![255u8; w * h];
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        for y in 16..136 {
            for x in 0..380 {
                p[y * w + 40 + x + y] = if bits[x / 4] > 0.5 { 0 } else { 255 };
            }
        }
        for budget in [0, 64, 102399, 102400, 204799, 204800, 4000000] {
            let (mut ba, mut bb) = (budget, budget);
            let (mut la, mut lb) = (false, false);
            let a = global_cached_groups(&p, w, h, &mut ba, &mut la);
            let b = [
                directional_groups(&p, w, h, 0, [0, 0, w, h], &mut bb, &mut lb),
                directional_groups(&p, w, h, 1, [0, 0, w, h], &mut bb, &mut lb),
            ];
            assert_eq!(la, lb);
            for (a, b) in a.iter().zip(b) {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    assert_eq!(a.bounds, b.bounds);
                    assert_eq!(a.score, b.score);
                }
            }
        }
    }
    #[cfg(feature = "experimental-classical-edge-groups")]
    #[test]
    fn directional_groups_separate_sheared_symbols_and_budget() {
        let (w, h) = (640, 160);
        let mut p = vec![255u8; w * h];
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        for y in (16..56).chain(96..136) {
            for x in 0..380 {
                p[y * w + 40 + x + y] = if bits[x / 4] > 0.5 { 0 } else { 255 };
            }
        }
        let b = [0, 0, w, h];
        let a = directional_groups(&p, w, h, 0, b, &mut 1000000, &mut false);
        assert!(a.iter().any(|p| p.bounds[3] <= 64.));
        assert!(a.iter().any(|p| p.bounds[1] >= 88.));
        assert!(
            a.iter().all(|p| p.bounds[3] <= 64. || p.bounds[1] >= 88.),
            "no group may bridge the blank gap: {a:?}"
        );
        let mut t = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                t[x * h + y] = p[y * w + x];
            }
        }
        let rotated = directional_groups(&t, h, w, 1, [0, 0, h, w], &mut 1000000, &mut false);
        assert_eq!(a.len(), rotated.len());
        for (u, v) in a.iter().zip(rotated) {
            assert_eq!(
                u.bounds,
                [v.bounds[1], v.bounds[0], v.bounds[3], v.bounds[2]]
            );
            assert_eq!(u.score, v.score);
        }
        let mut limited = false;
        assert!(directional_groups(&p, w, h, 0, b, &mut 63, &mut limited).is_empty());
        assert!(limited);
        p.fill(128);
        assert!(directional_groups(&p, w, h, 0, b, &mut 1000000, &mut false).is_empty());
        let mut state = 123u32;
        for v in &mut p {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (state >> 24) as u8;
        }
        assert!(directional_groups(&p, w, h, 0, b, &mut 1000000, &mut false).is_empty());
    }
    use super::*;
    #[cfg(feature = "experimental-classical-accepted-band")]
    #[test]
    fn persistent_columns_preserve_separate_groups_and_reject_single_edge() {
        let (w, h) = (1000, 64);
        let mut p = vec![255u8; w * h];
        for y in 0..h {
            for (a, b) in [(30, 410), (590, 970)] {
                for x in a..b {
                    if (x / 4) % 2 == 0 {
                        p[y * w + x] = 0;
                    }
                }
            }
        }
        assert!(persistent_extent(&p, w, 0, [0, 0, w, h], &mut 1000000, &mut false).is_none());
        for y in 0..h {
            p[y * w + 590..y * w + 970].fill(255);
        }
        assert!(persistent_extent(&p, w, 0, [0, 0, w, h], &mut 1000000, &mut false).is_some());
        p.fill(255);
        for y in 0..h {
            p[y * w + 200] = 0;
        }
        assert!(persistent_extent(&p, w, 0, [0, 0, w, h], &mut 1000000, &mut false).is_none());
        p.fill(255);
        assert!(persistent_extent(&p, w, 0, [0, 0, w, h], &mut 1000000, &mut false).is_none());
    }
    #[cfg(feature = "experimental-classical-crossband")]
    #[test]
    fn crossband_geometry_distinguishes_rows_from_internal_fragments() {
        let a = [40., 100., 440., 220.];
        assert!(!crossband_alternative(a, a));
        assert!(!crossband_alternative([41., 101., 439., 219.], a));
        assert!(crossband_alternative([40., 116., 440., 220.], a));
        assert!(crossband_alternative(
            [100., 40., 220., 440.],
            [116., 40., 220., 440.]
        ));
        assert!(!crossband_alternative([40., 100., 180., 220.], a));
        assert!(!crossband_alternative([40., 140., 440., 170.], a));
        assert!(!crossband_alternative([400., 116., 800., 220.], a));
    }
    #[cfg(feature = "experimental-classical-consistent-angle")]
    #[test]
    fn distributed_oblique_direction_rejects_mixed_cells_and_noise() {
        let (w, h) = (480usize, 420usize);
        let b = [100., 150., 380., 270.];
        let q = [[100., 150.], [380., 150.], [380., 270.], [100., 270.]];
        let mut gray = vec![128u8; w * h];
        for angle in [-30f64, 30., 60., -60.] {
            let a = angle.to_radians();
            for y in 0..h {
                for x in 0..w {
                    let u = x as f64 * a.cos() + y as f64 * a.sin();
                    gray[y * w + x] =
                        (128. + 110. * (u * 2. * std::f64::consts::PI / 12.).sin()) as u8;
                }
            }
            let p = refine_orientation(&gray, w, w, h, 1, b, &mut 4_000_000, &mut false);
            assert_ne!(p, q);
            let mut limited = false;
            assert_eq!(
                refine_orientation(&gray, w, w, h, 1, b, &mut 1, &mut limited),
                q
            );
            assert!(limited);
            // A localized conflicting stripe field must block the large correction.
            for y in 168..210 {
                for x in 142..207 {
                    let u = x as f64;
                    gray[y * w + x] =
                        (128. + 110. * (u * 2. * std::f64::consts::PI / 12.).sin()) as u8;
                }
            }
            assert_eq!(
                refine_orientation(&gray, w, w, h, 1, b, &mut 4_000_000, &mut false),
                q
            );
        }
        let mut state = 987u32;
        for v in &mut gray {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (state >> 24) as u8;
        }
        assert_eq!(
            refine_orientation(&gray, w, w, h, 1, b, &mut 4_000_000, &mut false),
            q
        );
    }
    #[cfg(feature = "experimental-classical-orientation")]
    #[test]
    fn oriented_refinement_preserves_blank_boundaries_and_budget() {
        let (w, h) = (360usize, 280usize);
        let b = [70., 100., 290., 180.];
        let mut gray = vec![128; w * h];
        for angle in [-10f64, 10.] {
            let a = angle.to_radians();
            for y in 0..h {
                for x in 0..w {
                    let u = x as f64 * a.cos() + y as f64 * a.sin();
                    gray[y * w + x] =
                        (128. + 110. * (u * 2. * std::f64::consts::PI / 12.).sin()) as u8;
                }
            }
            let p = refine_orientation(&gray, w, w, h, 1, b, &mut 4_000_000, &mut false);
            let got = (p[1][1] - p[0][1]).atan2(p[1][0] - p[0][0]).to_degrees();
            assert!((got - angle).abs() < 2.);
            let mut limited = false;
            assert_eq!(
                refine_orientation(&gray, w, w, h, 1, b, &mut 10, &mut limited),
                [[70., 100.], [290., 100.], [290., 180.], [70., 180.]]
            );
            assert!(limited);
            assert_eq!(
                refine_orientation(
                    &gray,
                    w,
                    w,
                    h,
                    1,
                    [0., 0., 220., 80.],
                    &mut 4_000_000,
                    &mut false
                ),
                [[0., 0.], [220., 0.], [220., 80.], [0., 80.]]
            );
        }
        gray.fill(128);
        assert_eq!(
            refine_orientation(&gray, w, w, h, 1, b, &mut 4_000_000, &mut false),
            [[70., 100.], [290., 100.], [290., 180.], [70., 180.]]
        );
    }
    #[cfg(feature = "experimental-classical-accepted-band")]
    #[test]
    fn accepted_band_trims_blank_context_preserves_two_bands_and_budget() {
        let (w, h) = (440, 200);
        let mut pixels = vec![255u8; w * h];
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        for y in 70..130 {
            for x in 30..410 {
                if bits[(x - 30) / 4] > 0.5 {
                    pixels[y * w + x] = 0;
                }
            }
        }
        let b = [10, 0, 430, 200];
        let mut limited = false;
        let p = refine_accepted_band(&pixels, w, h, 0, b, &mut 1000000, &mut limited);
        assert!(p[1] > 0 && p[3] < 200);
        assert!(p[0] <= 30 && p[2] >= 410);
        assert!(!limited);
        let mut rotated = vec![255u8; w * h];
        for y in 0..h {
            for x in 0..w {
                rotated[x * h + y] = pixels[y * w + x];
            }
        }
        assert_eq!(
            refine_accepted_band(
                &rotated,
                h,
                w,
                1,
                [0, 10, 200, 430],
                &mut 1000000,
                &mut false
            ),
            [p[1], p[0], p[3], p[2]]
        );
        for y in 92..108 {
            pixels[y * w..(y + 1) * w].fill(255);
        }
        assert_eq!(
            refine_accepted_band(&pixels, w, h, 0, b, &mut 1000000, &mut false),
            b
        );
        let mut limited = false;
        assert_eq!(
            refine_accepted_band(&pixels, w, h, 0, b, &mut 1, &mut limited),
            b
        );
        assert!(limited);
        pixels.fill(255);
        assert_eq!(
            refine_accepted_band(&pixels, w, h, 0, b, &mut 1000000, &mut false),
            b
        );
    }
    #[cfg(feature = "experimental-classical-complete-components")]
    #[test]
    fn full_transition_support_protects_complete_not_partial_regions() {
        let (w, h) = (512, 100);
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        let mut p = vec![255u8; w * h];
        for y in 0..h {
            for x in 0..380 {
                if bits[x / 4] > 0.5 {
                    p[y * w + 60 + x] = 0;
                }
            }
        }
        assert!(complete_edge_support(&p, w, h, 0, [40, 0, 460, h]));
        assert!(!complete_edge_support(&p, w, h, 0, [40, 0, 330, h]));
        let mut t = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                t[x * h + y] = p[y * w + x];
            }
        }
        assert!(complete_edge_support(&t, h, w, 1, [0, 40, h, 460]));
        for y in 38..h {
            p[y * w..(y + 1) * w].fill(255);
        }
        assert!(!complete_edge_support(&p, w, h, 0, [40, 0, 460, h]));
        p.fill(255);
        assert!(!complete_edge_support(&p, w, h, 0, [40, 0, 460, h]));
    }

    #[cfg(feature = "experimental-classical-wide-gaps")]
    #[test]
    fn wide_gaps_require_repeated_cross_row_evidence_and_keep_large_gaps() {
        let (w, h) = (64, 5);
        let mut m = vec![0; w * h];
        for y in 1..4 {
            for x in (2..42).step_by(4) {
                m[y * w + x] = 1;
            }
            m[y * w + 52] = 1;
        }
        let mut c = m.clone();
        bridge_wide_gaps(&m, &mut c, w, h, 0, None);
        assert!((2..39).all(|x| c[2 * w + x] > 0));
        assert!((42..52).all(|x| c[2 * w + x] == 0));
        let mut transpose = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                transpose[x * h + y] = m[y * w + x];
            }
        }
        let mut tc = transpose.clone();
        bridge_wide_gaps(&transpose, &mut tc, h, w, 1, None);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(c[y * w + x], tc[x * h + y]);
            }
        }
        for y in [1, 3] {
            m[y * w..(y + 1) * w].fill(0);
        }
        let mut c = m.clone();
        bridge_wide_gaps(&m, &mut c, w, h, 0, None);
        assert_eq!(c, m);
        m.fill(0);
        for y in 1..4 {
            m[y * w + 2] = 1;
            m[y * w + 6] = 1;
        }
        let mut c = m.clone();
        bridge_wide_gaps(&m, &mut c, w, h, 0, None);
        assert_eq!(c, m);
    }
    #[cfg(feature = "experimental-classical-stable-components")]
    #[test]
    fn accepted_components_do_not_attach_to_neighboring_fragments() {
        let (w, h) = (80, 12);
        let mut m = vec![0; w * h];
        for y in 2..10 {
            for x in (2..42).step_by(4) {
                m[y * w + x] = 1;
            }
            for x in 42..62 {
                m[y * w + x] = 1;
            }
        }
        let protected = stable_components(&m, w, h, 0, None);
        assert!(protected[5 * w + 50]);
        assert!(!protected[5 * w + 38]);
        let mut c = m.clone();
        bridge_wide_gaps(&m, &mut c, w, h, 0, None);
        assert!((39..42).all(|x| c[5 * w + x] == 0));
        assert!((2..35).all(|x| c[5 * w + x] > 0));
    }
    #[cfg(feature = "experimental-classical-wide-gaps")]
    #[test]
    fn wide_modules_form_two_separate_supported_regions() {
        let (w, h) = (1024, 384);
        let mut p = vec![255u8; w * h];
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        for top in [32, 224] {
            for y in top..top + 112 {
                for x in 0..855 {
                    if bits[x / 9] > 0.5 {
                        p[y * w + 72 + x] = 0;
                    }
                }
            }
        }
        let mut l = Localizer::default();
        let ps = l.detect(ImageView::new(&p, w, h, 1, w).unwrap()).unwrap();
        assert!(
            ps.iter().any(|p| p.bounds[0] <= 80.
                && p.bounds[2] >= 920.
                && p.bounds[1] < 48.
                && p.bounds[3] < 180.),
            "first wide symbol"
        );
        assert!(
            ps.iter().any(|p| p.bounds[0] <= 80.
                && p.bounds[2] >= 920.
                && p.bounds[1] > 180.
                && p.bounds[3] > 320.),
            "second wide symbol"
        );
        p.fill(255);
        assert!(l
            .detect(ImageView::new(&p, w, h, 1, w).unwrap())
            .unwrap()
            .is_empty());
    }

    #[cfg(feature = "experimental-classical-bands")]
    #[test]
    fn repeated_band_geometry_and_explicit_work_limit() {
        let (w, h) = (256, 320);
        let mut p = vec![255u8; w * h];
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        for y in 32..288 {
            for x in 32..222 {
                p[y * w + x] = if bits[(x - 32) / 2] > 0.5 { 0 } else { 255 };
            }
        }
        let mut l = Localizer::default();
        let found = l.detect(ImageView::new(&p, w, h, 1, w).unwrap()).unwrap();
        assert!(
            found.iter().any(|p| p.bounds[0] <= 32.
                && p.bounds[2] >= 222.
                && p.bounds[1] <= 40.
                && p.bounds[3] >= 280.),
            "tall barcode recovered using repeated edge support"
        );
        for y in 140..164 {
            p[y * w..(y + 1) * w].fill(255);
        }
        let mut budget = 1_000_000;
        let mut limited = false;
        let bands = coherent_bands(&p, w, h, 0, [24, 24, 232, 296], &mut budget, &mut limited);
        assert_eq!(bands.len(), 2);
        assert!(bands[0].bounds[3] < bands[1].bounds[1]);
        assert!(!limited);
        let mut budget = 0;
        assert!(
            coherent_bands(&p, w, h, 0, [24, 24, 232, 296], &mut budget, &mut limited).is_empty()
        );
        assert!(limited);
        p.fill(255);
        let mut budget = 1_000_000;
        let mut limited = false;
        assert!(
            coherent_bands(&p, w, h, 0, [24, 24, 232, 296], &mut budget, &mut limited).is_empty()
        );
        for y in 0..h {
            for x in 32..128 {
                p[y * w + x] = 0;
            }
        }
        assert!(
            coherent_bands(&p, w, h, 0, [24, 24, 232, 296], &mut budget, &mut limited).is_empty()
        );
    }
    #[cfg(feature = "experimental-classical-trim")]
    #[test]
    fn trimmed_direction_rejects_repeated_two_axis_texture() {
        let (w, h) = (128, 64);
        let mut g = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                g[y * w + x] = if (x / 3) % 2 == 0 { 20 } else { 220 };
            }
        }
        let mut budget = 100000;
        let mut limited = false;
        assert!(stationary_direction(
            &g,
            w,
            h,
            0,
            [0, 0, w, h],
            &mut budget,
            &mut limited
        ));
        assert!(!stationary_direction(
            &g,
            w,
            h,
            1,
            [0, 0, w, h],
            &mut budget,
            &mut limited
        ));
        for y in 0..h {
            for x in 0..w {
                g[y * w + x] = if ((x / 3) + (y / 3)) % 2 == 0 {
                    20
                } else {
                    220
                };
            }
        }
        assert!(!stationary_direction(
            &g,
            w,
            h,
            0,
            [0, 0, w, h],
            &mut budget,
            &mut limited
        ));
        let mut budget = 0;
        assert!(!stationary_direction(
            &g,
            w,
            h,
            0,
            [0, 0, w, h],
            &mut budget,
            &mut limited
        ));
        assert!(limited);
    }
    #[cfg(feature = "experimental-classical-trim")]
    #[test]
    fn sparse_attachments_trim_and_spatial_gaps_remain() {
        let tw = 40;
        let mut points = vec![];
        for y in 1..25 {
            points.push(y * tw + 2);
        }
        for y in (8..12).chain(16..20) {
            for x in 3..23 {
                points.push(y * tw + x);
            }
        }
        let p = component_bands(&points, tw, 0, [2, 1, 23, 25]);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].bounds, [1., 7., 24., 13.]);
        assert_eq!(p[1].bounds, [1., 15., 24., 21.]);
        let transposed: Vec<_> = points.iter().map(|p| (p % tw) * tw + p / tw).collect();
        let q = component_bands(&transposed, tw, 1, [1, 2, 25, 23]);
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].bounds, [7., 1., 13., 24.]);
        let sparse: Vec<_> = (1..25).map(|y| y * tw + 2).collect();
        assert!(component_bands(&sparse, tw, 0, [2, 1, 3, 25]).is_empty());
        let isolated: Vec<_> = (2..23).map(|x| 8 * tw + x).collect();
        assert!(component_bands(&isolated, tw, 0, [2, 8, 23, 9]).is_empty());
    }
    #[cfg(feature = "experimental-classical-span2")]
    #[test]
    fn blurred_transition_support_without_lowering_edge_threshold() {
        let (w, h) = (288, 160);
        let mut source = vec![180u8; w * h];
        for y in 40..120 {
            for x in 40..248 {
                source[y * w + x] = if (x - 40) / 6 % 2 == 0 { 100 } else { 180 };
            }
        }
        let mut blurred = source.clone();
        for y in 0..h {
            for x in 2..w - 2 {
                blurred[y * w + x] = (source[y * w + x - 2..y * w + x + 3]
                    .iter()
                    .map(|&v| v as u32)
                    .sum::<u32>()
                    / 5) as u8;
            }
        }
        assert!((45..240).any(|x| blurred[80 * w + x].abs_diff(blurred[80 * w + x - 2]) >= 24));
        assert!((45..240).all(|x| blurred[80 * w + x].abs_diff(blurred[80 * w + x - 1]) < 24));
        let mut l = Localizer::default();
        let r = l
            .detect(ImageView::new(&blurred, w, h, 1, w).unwrap())
            .unwrap();
        assert!(r.iter().any(|p| p.bounds[0] < 80. && p.bounds[2] > 220.));
        // A single soft step does not become a repeated barcode region.
        for y in 0..h {
            for x in 0..w {
                blurred[y * w + x] = if x < 140 {
                    100
                } else if x < 145 {
                    100 + ((x - 140) * 16) as u8
                } else {
                    180
                };
            }
        }
        assert!(l
            .detect(ImageView::new(&blurred, w, h, 1, w).unwrap())
            .unwrap()
            .is_empty());
    }
    #[cfg(feature = "experimental-classical-rank-direction")]
    #[test]
    fn capped_equal_symbols_remain_spatial_and_report_omissions() {
        let (w, h) = (1024, 512);
        let mut data = vec![255u8; w * h];
        let bits = crate::ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        for row in 0..4 {
            for col in 0..4 {
                for y in row * 128 + 24..row * 128 + 72 {
                    for x in 0..190 {
                        data[y * w + col * 256 + 24 + x] = if bits[x / 2] > 0.5 { 0 } else { 255 };
                    }
                }
            }
        }
        let mut l = Localizer::default();
        let found = l
            .detect(ImageView::new(&data, w, h, 1, w).unwrap())
            .unwrap();
        assert_eq!(found.len(), 12);
        assert!(found.iter().all(|p| (0.0..=1.0).contains(&p.score)));
        for (i, a) in found.iter().enumerate() {
            assert!(found[i + 1..].iter().all(|b| a.bounds != b.bounds));
        }
        assert_eq!(l.omitted, 4);
        assert!(!l.work_limited);
    }
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

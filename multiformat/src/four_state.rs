//! Rows of height-modulated bars, as used by four-state postal codes.
//!
//! The bars of such a symbol share one width and one pitch; each bar carries its
//! data in whether it extends above and below the central tracker band. Linear
//! readers measure widths along a scanline and cannot see this, so this reader
//! works on the connected components of a threshold image. It chains evenly
//! spaced bar-like components into rows at any orientation, fits the tracker
//! line through their centroids and classifies each bar by its extent on both
//! sides. Format decoders (currently RM4SCC) interpret the bar states.
use crate::numeric::{f64_f32, usize_f64};

/// One bar's state: which sides of the tracker band it extends to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    /// Tracker only.
    Tracker,
    /// Tracker and ascender.
    Ascender,
    /// Tracker and descender.
    Descender,
    /// Ascender, tracker and descender.
    Full,
}

impl Bar {
    #[must_use]
    pub const fn new(up: bool, down: bool) -> Self {
        match (up, down) {
            (false, false) => Self::Tracker,
            (true, false) => Self::Ascender,
            (false, true) => Self::Descender,
            (true, true) => Self::Full,
        }
    }
    #[must_use]
    pub const fn up(self) -> bool {
        matches!(self, Self::Ascender | Self::Full)
    }
    #[must_use]
    pub const fn down(self) -> bool {
        matches!(self, Self::Descender | Self::Full)
    }
    /// The state seen when the symbol is turned upside down.
    #[must_use]
    pub const fn flipped(self) -> Self {
        Self::new(self.down(), self.up())
    }
}

/// A row of bars in one reading direction, with "up" to the left of it.
#[derive(Clone, Debug)]
pub struct Row {
    pub bars: Vec<Bar>,
    /// Start-up, end-up, end-down and start-down corners in image coordinates.
    pub polygon: [[f32; 2]; 4],
    /// Ink or the image border lies where the next bar beyond an end would be:
    /// the row may be a part of a longer one.
    pub open: bool,
}

impl Row {
    /// The same row read upside down: reversed, with ascenders and descenders swapped.
    #[must_use]
    pub fn rotated(&self) -> Self {
        let [a, b, c, d] = self.polygon;
        Self {
            bars: self.bars.iter().rev().map(|bar| bar.flipped()).collect(),
            polygon: [c, d, a, b],
            open: self.open,
        }
    }
}

/// Fewest bars a row may have; RM4SCC's shortest symbol (one character and the
/// check character) has ten.
pub const MIN_BARS: usize = 10;
/// Components a frame may yield before the reader stops early.
const MAX_BLOBS: usize = 1 << 16;
/// Rows a frame may yield before the reader stops early.
const MAX_ROWS: usize = 256;

#[derive(Clone, Copy, Default)]
struct Moments {
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    syy: f64,
    sxy: f64,
}

impl Moments {
    /// Add pixels `x0..x1` of row `y`.
    fn add_run(&mut self, y: usize, x0: usize, x1: usize) {
        let (a, b, y) = (usize_f64(x0), usize_f64(x1 - 1), usize_f64(y));
        let n = b - a + 1.;
        let sx = (a + b) * n / 2.;
        let squares = |k: f64| k * (k + 1.) * (2. * k + 1.) / 6.;
        self.n += n;
        self.sx += sx;
        self.sy += y * n;
        self.sxx += squares(b) - squares(a - 1.);
        self.syy += y * y * n;
        self.sxy += y * sx;
    }
    fn merge(&mut self, other: &Self) {
        self.n += other.n;
        self.sx += other.sx;
        self.sy += other.sy;
        self.sxx += other.sxx;
        self.syy += other.syy;
        self.sxy += other.sxy;
    }
}

/// A bar-shaped component: pixel-centre centroid and covariance.
#[derive(Clone, Copy, Debug)]
struct Blob {
    c: [f64; 2],
    /// Covariance `xx`, `yy`, `xy`.
    cov: [f64; 3],
    /// Unit long axis.
    axis: [f64; 2],
    length: f64,
    width: f64,
}

impl Blob {
    fn from_moments(m: &Moments) -> Option<Self> {
        let (mx, my) = (m.sx / m.n, m.sy / m.n);
        // A solid one-pixel column has variance 1/12 across it.
        let xx = m.sxx / m.n - mx * mx + 1. / 12.;
        let yy = m.syy / m.n - my * my + 1. / 12.;
        let xy = m.sxy / m.n - mx * my;
        let mean = f64::midpoint(xx, yy);
        let spread = ((xx - yy) / 2.).hypot(xy);
        let (major, minor) = (mean + spread, (mean - spread).max(1. / 12.));
        let angle = 0.5 * (2. * xy).atan2(xx - yy);
        let (length, width) = ((12. * major).sqrt(), (12. * minor).sqrt());
        // Bars are solid rectangles: their area matches their moment extents.
        let fill = m.n / (length * width);
        (0.6..=1.5).contains(&fill).then_some(Self {
            c: [mx + 0.5, my + 0.5],
            cov: [xx, yy, xy],
            axis: [angle.cos(), angle.sin()],
            length,
            width,
        })
    }
    /// Half the extent along unit direction `d`, for a solid rectangle.
    fn half_along(&self, d: [f64; 2]) -> f64 {
        let [xx, yy, xy] = self.cov;
        (3. * (d[0] * d[0] * xx + d[1] * d[1] * yy + 2. * d[0] * d[1] * xy)).sqrt()
    }
}

#[derive(Clone, Copy)]
struct Node {
    parent: u32,
    moments: Moments,
    /// Last row with a run of this component, or `usize::MAX` once oversized.
    row: usize,
    bounds: [usize; 4],
}

fn find(nodes: &mut [Node], mut i: u32) -> u32 {
    while nodes[i as usize].parent != i {
        let parent = nodes[i as usize].parent;
        nodes[i as usize].parent = nodes[parent as usize].parent;
        i = parent;
    }
    i
}

/// Merge the component rooted at `root` into the one rooted at `own`.
fn union(nodes: &mut [Node], own: u32, root: u32) {
    let other = nodes[root as usize];
    let node = &mut nodes[own as usize];
    node.moments.merge(&other.moments);
    node.bounds = [
        node.bounds[0].min(other.bounds[0]),
        node.bounds[1].min(other.bounds[1]),
        node.bounds[2].max(other.bounds[2]),
        node.bounds[3].max(other.bounds[3]),
    ];
    nodes[root as usize].parent = own;
}

/// Four-connected components of `bits`, streamed row by row so that memory
/// follows the image width. Returns `true` when the blob cap stopped the pass.
fn blobs(bits: &[bool], width: usize, height: usize, max_extent: usize) -> (Vec<Blob>, bool) {
    let mut out = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut free: Vec<u32> = Vec::new();
    let mut previous: Vec<(usize, usize, u32)> = Vec::new();
    let mut current: Vec<(usize, usize, u32)> = Vec::new();
    let mut pending: Vec<u32> = Vec::new();
    let mut next_pending: Vec<u32> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    let emit = |node: &Node, out: &mut Vec<Blob>| {
        let [x0, y0, x1, y1] = node.bounds;
        if node.moments.n >= 3.
            && x1 - x0 <= max_extent
            && y1 - y0 <= max_extent
            && let Some(blob) = Blob::from_moments(&node.moments)
        {
            out.push(blob);
        }
    };
    for y in 0..=height {
        current.clear();
        if y < height {
            let row = &bits[y * width..(y + 1) * width];
            crate::transition_offsets_into(row, &mut offsets);
            // Runs alternate from the first pixel; the ink ones start here.
            let first = usize::from(!row.first().is_some_and(|&ink| ink));
            let mut j = 0;
            for &[start, x] in offsets[first..].as_chunks::<2>().0 {
                while j < previous.len() && previous[j].1 <= start {
                    j += 1;
                }
                let mut label: Option<u32> = None;
                let mut k = j;
                while k < previous.len() && previous[k].0 < x {
                    let root = find(&mut nodes, previous[k].2);
                    match label {
                        None => label = Some(root),
                        Some(own) if own != root => {
                            union(&mut nodes, own, root);
                            pending.push(root);
                        }
                        Some(_) => {}
                    }
                    k += 1;
                }
                let label = label.unwrap_or_else(|| {
                    let node = Node {
                        parent: 0,
                        moments: Moments::default(),
                        row: y,
                        bounds: [start, y, x, y + 1],
                    };
                    let id = if let Some(id) = free.pop() {
                        nodes[id as usize] = node;
                        id
                    } else {
                        nodes.push(node);
                        u32::try_from(nodes.len() - 1).expect("labels are bounded by pixels")
                    };
                    nodes[id as usize].parent = id;
                    pending.push(id);
                    id
                });
                let node = &mut nodes[label as usize];
                node.moments.add_run(y, start, x);
                node.bounds[0] = node.bounds[0].min(start);
                node.bounds[2] = node.bounds[2].max(x);
                node.bounds[3] = y + 1;
                current.push((start, x, label));
            }
        }
        for run in &mut current {
            run.2 = find(&mut nodes, run.2);
            nodes[run.2 as usize].row = y;
        }
        next_pending.clear();
        pending.sort_unstable();
        pending.dedup();
        for &id in &pending {
            let node = &nodes[id as usize];
            if node.parent != id {
                free.push(id);
            } else if node.row == y && y < height {
                next_pending.push(id);
            } else {
                emit(node, &mut out);
                free.push(id);
                if out.len() >= MAX_BLOBS {
                    return (out, true);
                }
            }
        }
        std::mem::swap(&mut pending, &mut next_pending);
        std::mem::swap(&mut previous, &mut current);
    }
    (out, false)
}

/// Uniform grid over blob centroids.
struct Grid {
    cell: f64,
    columns: usize,
    rows: usize,
    starts: Vec<usize>,
    items: Vec<usize>,
}

impl Grid {
    fn new(blobs: &[Blob], width: usize, height: usize, cell: f64) -> Self {
        let columns = crate::numeric::f64_usize(usize_f64(width) / cell) + 1;
        let rows = crate::numeric::f64_usize(usize_f64(height) / cell) + 1;
        let index = |b: &Blob| {
            let x = crate::numeric::f64_usize(b.c[0] / cell).min(columns - 1);
            let y = crate::numeric::f64_usize(b.c[1] / cell).min(rows - 1);
            y * columns + x
        };
        let mut starts = vec![0; columns * rows + 1];
        for b in blobs {
            starts[index(b) + 1] += 1;
        }
        for i in 1..starts.len() {
            starts[i] += starts[i - 1];
        }
        let mut fill = starts.clone();
        let mut items = vec![0; blobs.len()];
        for (i, b) in blobs.iter().enumerate() {
            let cell = index(b);
            items[fill[cell]] = i;
            fill[cell] += 1;
        }
        Self {
            cell,
            columns,
            rows,
            starts,
            items,
        }
    }
    fn near(&self, p: [f64; 2], radius: f64, mut visit: impl FnMut(usize)) {
        let range = |v: f64, n: usize| {
            let lo = crate::numeric::f64_usize(((v - radius) / self.cell).max(0.));
            let hi = crate::numeric::f64_usize(((v + radius) / self.cell).max(0.)).min(n - 1);
            lo..=hi
        };
        for y in range(p[1], self.rows) {
            for x in range(p[0], self.columns) {
                let cell = y * self.columns + x;
                for &i in &self.items[self.starts[cell]..self.starts[cell + 1]] {
                    visit(i);
                }
            }
        }
    }
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// The "up" direction for reading direction `u` (image y points down).
const fn up_of(u: [f64; 2]) -> [f64; 2] {
    [u[1], -u[0]]
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_unstable_by(f64::total_cmp);
    values[values.len() / 2]
}

/// Principal direction of the member centroids, oriented like `previous`.
fn fit_direction(blobs: &[Blob], members: &[usize], previous: [f64; 2]) -> [f64; 2] {
    let n = usize_f64(members.len());
    let (mx, my) = members.iter().fold((0., 0.), |(x, y), &i| {
        (x + blobs[i].c[0] / n, y + blobs[i].c[1] / n)
    });
    let (mut xx, mut yy, mut xy) = (0., 0., 0.);
    for &i in members {
        let (dx, dy) = (blobs[i].c[0] - mx, blobs[i].c[1] - my);
        xx += dx * dx;
        yy += dy * dy;
        xy += dx * dy;
    }
    let angle = 0.5 * (2. * xy).atan2(xx - yy);
    let u = [angle.cos(), angle.sin()];
    if dot(u, previous) < 0. {
        [-u[0], -u[1]]
    } else {
        u
    }
}

/// Reading direction across the members' long axes, oriented like `previous`.
/// Unlike a fit through centroids it does not tilt with the bar states.
fn axis_direction(blobs: &[Blob], members: &[usize], previous: [f64; 2]) -> [f64; 2] {
    let d = up_of(previous);
    let (x, y) = members.iter().fold((0., 0.), |(x, y), &i| {
        let b = &blobs[i];
        let sign = dot(b.axis, d).signum();
        let weight = b.length - b.width;
        (x + sign * weight * b.axis[0], y + sign * weight * b.axis[1])
    });
    let norm = f64::hypot(x, y);
    if norm < 1e-9 {
        return previous;
    }
    // `up_of` turns `u` by -90 degrees; turn the axis back by +90 degrees.
    [-y / norm, x / norm]
}

/// Length along `d` that two bars share.
fn shared(a: &Blob, b: &Blob, d: [f64; 2]) -> f64 {
    let (ac, ah) = (dot(a.c, d), a.half_along(d));
    let (bc, bh) = (dot(b.c, d), b.half_along(d));
    (ac + ah).min(bc + bh) - (ac - ah).max(bc - bh)
}

/// Grow a row of evenly spaced bars from `seed` in both directions. Members are
/// returned in reading order along the returned direction.
fn chain(blobs: &[Blob], grid: &Grid, seed: usize) -> (Vec<usize>, [f64; 2]) {
    let s = &blobs[seed];
    let mut u = [-s.axis[1], s.axis[0]];
    let width = s.width.max(1.);
    let mut members = vec![seed];
    let mut ends = [seed, seed];
    let mut gaps: Vec<f64> = Vec::new();
    let mut pitch: Option<f64> = None;
    // Neighbours share at least the tracker part. The smallest shared length
    // so far estimates it; comparing neighbours only tolerates shear and bends.
    let mut tracker: Option<f64> = None;
    let mut direction = 0;
    while direction < 2 {
        let sign = if direction == 0 { 1. } else { -1. };
        let d = up_of(u);
        let current = &blobs[ends[direction]];
        let (lo, hi) = pitch.map_or((1.2 * width, 6. * width), |p| (0.6 * p, 1.45 * p));
        let mut best: Option<(f64, usize)> = None;
        grid.near(current.c, hi + width, |i| {
            let q = &blobs[i];
            let du = sign * dot([q.c[0] - current.c[0], q.c[1] - current.c[1]], u);
            if du < lo || du > hi || best.is_some_and(|(b, _)| b <= du) {
                return;
            }
            if q.width > 2.2 * width || q.width < 0.45 * width {
                return;
            }
            // An elongated part must run along the bars.
            if q.length > 2. * q.width && dot(q.axis, d).abs() < 0.9 {
                return;
            }
            if shared(current, q, d) >= tracker.map_or(0.4 * width, |t| (0.6 * t).max(0.4 * width))
            {
                best = Some((du, i));
            }
        });
        if let Some((du, i)) = best {
            members.push(i);
            ends[direction] = i;
            gaps.push(du);
            let mut sorted = gaps.clone();
            pitch = Some(median(&mut sorted));
            let overlap = shared(current, &blobs[i], d);
            tracker = Some(tracker.map_or(overlap, |t| t.min(overlap)));
            u = axis_direction(blobs, &members, u);
            if members.len() > 4 * MAX_ROW_BARS {
                break;
            }
        } else {
            direction += 1;
        }
    }
    members.sort_by(|&a, &b| dot(blobs[a].c, u).total_cmp(&dot(blobs[b].c, u)));
    (members, u)
}

/// Longest row accepted; RM4SCC symbols in practice have under 100 bars.
const MAX_ROW_BARS: usize = 256;

/// Split sorted values at their widest gap: the threshold between the two groups
/// and whether the groups are clearly separated relative to `pitch`.
fn split(values: &[f64], pitch: f64) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    let (gap, at) = sorted
        .windows(2)
        .enumerate()
        .map(|(i, w)| (w[1] - w[0], i))
        .max_by(|a, b| a.0.total_cmp(&b.0))?;
    let (low, high) = sorted.split_at(at + 1);
    let spread = (low[low.len() - 1] - low[0]).max(high[high.len() - 1] - high[0]);
    (gap >= 0.35 * pitch && gap >= spread).then(|| f64::midpoint(sorted[at], sorted[at + 1]))
}

/// Classify a chained row, or `None` when its spacing or heights are irregular.
/// Heights are measured along the bars from the row's centre line, whose
/// direction differs from the bars' normal under shear. Symbols have as many
/// ascender-only as descender-only bars, so a line through all centroids
/// estimates it; one through the tracker and full bars, which lie on the
/// centre line, refines it.
fn classify(blobs: &[Blob], members: &[usize], u: [f64; 2], ink: &Ink<'_>) -> Option<Row> {
    if members.len() < MIN_BARS || members.len() > MAX_ROW_BARS {
        return None;
    }
    let u = axis_direction(blobs, members, u);
    let up = up_of(u);
    let first = classify_along(blobs, members, fit_direction(blobs, members, u), up, ink)
        .or_else(|| classify_along(blobs, members, u, up, ink))?;
    let centred: Vec<usize> = members
        .iter()
        .zip(&first.bars)
        .filter(|(_, bar)| bar.up() == bar.down())
        .map(|(&i, _)| i)
        .collect();
    if centred.len() < 4 {
        return Some(first);
    }
    classify_along(blobs, members, fit_direction(blobs, &centred, u), up, ink).or(Some(first))
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// The threshold image a row was found in.
struct Ink<'a> {
    bits: &'a [bool],
    width: usize,
    height: usize,
}

impl Ink<'_> {
    /// Whether `p` is ink. Points outside the image count as ink: a row that
    /// reaches the border may continue beyond it.
    fn at(&self, p: [f64; 2]) -> bool {
        if p[0] < 0. || p[1] < 0. {
            return true;
        }
        let (x, y) = (
            crate::numeric::f64_usize(p[0]),
            crate::numeric::f64_usize(p[1]),
        );
        x >= self.width || y >= self.height || self.bits[y * self.width + x]
    }
}

/// [`classify`] for row direction `row` and bar direction `up`.
fn classify_along(
    blobs: &[Blob],
    members: &[usize],
    row: [f64; 2],
    up: [f64; 2],
    ink: &Ink<'_>,
) -> Option<Row> {
    // Bars far from upright to the row are not a four-state row.
    let det = cross(row, up);
    if det.abs() < 0.5 {
        return None;
    }
    let n = usize_f64(members.len());
    let mean = members.iter().fold([0., 0.], |m, &i| {
        [m[0] + blobs[i].c[0] / n, m[1] + blobs[i].c[1] / n]
    });
    // Oblique coordinates: centroid = mean + along * row + across * up.
    let coordinates = |i: usize| {
        let v = [blobs[i].c[0] - mean[0], blobs[i].c[1] - mean[1]];
        (cross(v, up) / det, cross(row, v) / det)
    };
    let positions: Vec<f64> = members.iter().map(|&i| coordinates(i).0).collect();
    let mut gaps: Vec<f64> = positions.windows(2).map(|w| w[1] - w[0]).collect();
    let pitch = median(&mut gaps);
    // `median` sorted the gaps: the first and last are the extremes.
    if gaps[0] < 0.7 * pitch || gaps[gaps.len() - 1] > 1.3 * pitch {
        return None;
    }
    let (tops, bottoms): (Vec<f64>, Vec<f64>) = members
        .iter()
        .map(|&i| {
            let (offset, half) = (coordinates(i).1, blobs[i].half_along(up));
            (offset + half, half - offset)
        })
        .unzip();
    let (top_cut, bottom_cut) = (split(&tops, pitch)?, split(&bottoms, pitch)?);
    let top = tops.iter().copied().fold(0., f64::max);
    let bottom = bottoms.iter().copied().fold(0., f64::max);
    // Full bars are a few pitches tall.
    if !(1.5 * pitch..=14. * pitch).contains(&(top + bottom)) {
        return None;
    }
    let bars = tops
        .iter()
        .zip(&bottoms)
        .map(|(&t, &b)| Bar::new(t > top_cut, b > bottom_cut))
        .collect();
    let (first, last) = (
        positions[0] - pitch / 2.,
        positions[positions.len() - 1] + pitch / 2.,
    );
    let point = |along: f64, across: f64| {
        [
            f64_f32(mean[0] + row[0] * along + up[0] * across),
            f64_f32(mean[1] + row[1] * along + up[1] * across),
        ]
    };
    // The next bar beyond an end would cross the centre line one pitch on, or
    // two where a bar is missing. Two of three lines through the tracker must
    // agree, so that a speck in the quiet zone does not count.
    let continues = |end: f64, sign: f64| {
        let lines = [-0.2, 0., 0.2].map(|across| {
            (0..=12).any(|step| {
                let along = end + sign * pitch * (0.75 + 0.125 * f64::from(step));
                ink.at([
                    mean[0] + row[0] * along + up[0] * across * pitch,
                    mean[1] + row[1] * along + up[1] * across * pitch,
                ])
            })
        });
        lines.iter().filter(|&&hit| hit).count() >= 2
    };
    let open = continues(positions[0], -1.) || continues(positions[positions.len() - 1], 1.);
    Some(Row {
        bars,
        open,
        polygon: [
            point(first, top),
            point(last, top),
            point(last, -bottom),
            point(first, -bottom),
        ],
    })
}

/// `coarse` restricted to pixels darker than the mean of the
/// `(2 * radius + 1)`-square around them. Blur fills the gaps between bars in a
/// coarse local threshold; a window about one module across keeps them apart.
#[must_use]
pub fn fine_threshold(
    gray: &[u8],
    coarse: &[bool],
    width: usize,
    height: usize,
    radius: usize,
) -> Vec<bool> {
    // Integral image with a zero first row and column. Its entries wrap on bright
    // frames above 16 megapixels; window sums are differences, which stay exact.
    let stride = width + 1;
    let mut sums = vec![0u32; stride * (height + 1)];
    for (y, line) in gray.chunks_exact(width.max(1)).take(height).enumerate() {
        let (above, below) = sums[y * stride..].split_at_mut(stride);
        let mut row = 0u32;
        for ((&value, &up), sum) in line.iter().zip(&above[1..]).zip(&mut below[1..stride]) {
            row += u32::from(value);
            *sum = up.wrapping_add(row);
        }
    }
    let mut out = vec![false; width * height];
    let side = 2 * radius + 1;
    let full = u32::try_from(side * side).unwrap_or(u32::MAX);
    for y in 0..height {
        let (y0, y1) = (y.saturating_sub(radius), (y + radius + 1).min(height));
        let (top, bottom) = (
            &sums[y0 * stride..][..stride],
            &sums[y1 * stride..][..stride],
        );
        let line = &gray[y * width..][..width];
        let ink = &coarse[y * width..][..width];
        let out = &mut out[y * width..][..width];
        let inner = radius.min(width)..width.saturating_sub(radius).max(radius.min(width));
        let edge = |x: usize| {
            let (x0, x1) = (x.saturating_sub(radius), (x + radius + 1).min(width));
            let sum = bottom[x1]
                .wrapping_add(top[x0])
                .wrapping_sub(top[x1])
                .wrapping_sub(bottom[x0]);
            let area = u32::try_from((x1 - x0) * (y1 - y0)).unwrap_or(u32::MAX);
            ink[x] && u32::from(line[x]) * area < sum
        };
        for x in (0..inner.start).chain(inner.end..width) {
            out[x] = edge(x);
        }
        if inner.is_empty() {
            continue;
        }
        // Away from the left and right borders the window has a fixed width, so
        // the row is four parallel slices and one comparison per pixel.
        let area = if y1 - y0 == side {
            full
        } else {
            u32::try_from(side * (y1 - y0)).unwrap_or(u32::MAX)
        };
        let count = inner.len();
        let left_top = &top[inner.start - radius..][..count];
        let right_top = &top[inner.start + radius + 1..][..count];
        let left_bottom = &bottom[inner.start - radius..][..count];
        let right_bottom = &bottom[inner.start + radius + 1..][..count];
        let line = &line[inner.clone()];
        let ink = &ink[inner.clone()];
        for (i, out) in out[inner].iter_mut().enumerate() {
            let sum = right_bottom[i]
                .wrapping_add(left_top[i])
                .wrapping_sub(right_top[i])
                .wrapping_sub(left_bottom[i]);
            *out = ink[i] & (u32::from(line[i]) * area < sum);
        }
    }
    out
}

/// Rows of evenly spaced bars in a threshold image (`true` is ink). Returns the
/// rows and whether a work cap stopped the search early.
#[must_use]
pub fn rows(bits: &[bool], width: usize, height: usize) -> (Vec<Row>, bool) {
    let max_extent = width.max(height) / 4 + 8;
    let (blobs, mut limited) = blobs(bits, width, height, max_extent);
    if blobs.len() < MIN_BARS {
        return (Vec::new(), limited);
    }
    let mut widths: Vec<f64> = blobs.iter().map(|b| b.width).collect();
    let cell = (4. * median(&mut widths)).clamp(4., 64.);
    let grid = Grid::new(&blobs, width, height, cell);
    let mut visited = vec![false; blobs.len()];
    let mut rows = Vec::new();
    let ink = Ink {
        bits,
        width,
        height,
    };
    for seed in 0..blobs.len() {
        let s = &blobs[seed];
        if visited[seed] || s.length < 2.2 * s.width || s.length < 4. {
            continue;
        }
        let (members, u) = chain(&blobs, &grid, seed);
        for &i in &members {
            visited[i] = true;
        }
        if let Some(row) = classify(&blobs, &members, u, &ink) {
            rows.push(row);
            if rows.len() >= MAX_ROWS {
                limited = true;
                break;
            }
        }
    }
    (rows, limited)
}

/// Symbol rendering shared by the postal decoders' tests.
#[cfg(test)]
pub mod testing {
    use super::Bar;

    /// Bars from Zint codes: 0 full, 1 ascender, 2 descender, 3 tracker.
    #[must_use]
    pub fn from_codes(codes: &[u8]) -> Vec<Bar> {
        codes
            .iter()
            .map(|&c| Bar::new(c < 2, c % 2 == 0 && c < 3))
            .collect()
    }

    /// Bars of a Zint 2.16 `--dump` of a four-state symbol: ascender, tracker and descender rows.
    #[must_use]
    pub fn zint_bars(dump: [&str; 3]) -> Vec<Bar> {
        let bits = dump.map(|row| {
            row.chars()
                .filter_map(|c| c.to_digit(16))
                .flat_map(|v| (0..4).rev().map(move |b| v >> b & 1 == 1))
                .collect::<Vec<_>>()
        });
        (0..bits[1].len())
            .step_by(2)
            .filter(|&i| bits[1][i])
            .map(|i| Bar::new(bits[0][i], bits[2][i]))
            .collect()
    }

    /// `bars` drawn dark on light with bar width `module` pixels, pitch two
    /// modules and full height ten modules, rotated by `degrees` about the image
    /// centre, 4x4 supersampled, then box-blurred `blur` times.
    // A test rasterizer over small images: its float/index casts stay in range.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    #[must_use]
    pub fn render(bars: &[Bar], module: f32, degrees: f32, blur: usize) -> (Vec<u8>, usize, usize) {
        let length = module * (2. * bars.len() as f32 + 8.);
        let size = (length * 1.2) as usize;
        let (sin, cos) = degrees.to_radians().sin_cos();
        let centre = size as f32 / 2.;
        let mut gray = vec![0u8; size * size];
        for y in 0..size {
            for x in 0..size {
                let mut dark = 0;
                for s in 0..16 {
                    let px = x as f32 + (s % 4) as f32 / 4. + 0.125 - centre;
                    let py = y as f32 + (s / 4) as f32 / 4. + 0.125 - centre;
                    // Symbol frame: u along the row, v downwards across it.
                    let u = (cos * px + sin * py) / module + bars.len() as f32;
                    let v = (-sin * px + cos * py) / module + 5.;
                    let i = (u / 2.).floor();
                    if i < 0. || u - 2. * i >= 1. || i as usize >= bars.len() {
                        continue;
                    }
                    let bar = bars[i as usize];
                    let top = if bar.up() { 0. } else { 3.75 };
                    let bottom = if bar.down() { 10. } else { 6.25 };
                    dark += i32::from((top..bottom).contains(&v));
                }
                gray[y * size + x] = (230 - dark * 190 / 16) as u8;
            }
        }
        for _ in 0..blur {
            let previous = gray.clone();
            for y in 1..size - 1 {
                for x in 1..size - 1 {
                    let sum: u32 = (0..9)
                        .map(|k| u32::from(previous[(y + k / 3 - 1) * size + x + k % 3 - 1]))
                        .sum();
                    gray[y * size + x] = (sum / 9) as u8;
                }
            }
        }
        (gray, size, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draw upright bars: pitch 4, width 2, tracker rows 6..10, full rows 0..16.
    fn draw(bars: &[Bar]) -> (Vec<bool>, usize, usize) {
        let (width, height) = (bars.len() * 4 + 16, 32);
        let mut bits = vec![false; width * height];
        for (i, bar) in bars.iter().enumerate() {
            let top = if bar.up() { 8 } else { 14 };
            let bottom = if bar.down() { 24 } else { 18 };
            for y in top..bottom {
                for x in 8 + i * 4..10 + i * 4 {
                    bits[y * width + x] = true;
                }
            }
        }
        (bits, width, height)
    }

    #[test]
    fn reads_bar_states_in_some_direction() {
        use Bar::{Ascender as A, Descender as D, Full as F, Tracker as T};
        let bars = [A, T, F, D, A, F, T, D, A, T, D, F, F];
        let (bits, width, height) = draw(&bars[..]);
        let (rows, limited) = rows(&bits, width, height);
        assert!(!limited);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!(
            row.bars == bars || row.rotated().bars == bars,
            "{:?}",
            row.bars
        );
    }

    #[test]
    fn equal_bars_have_no_states() {
        let (bits, width, height) = draw(&[Bar::Full; 20]);
        assert!(rows(&bits, width, height).0.is_empty());
    }

    #[test]
    fn components_stream_by_row() {
        // A U shape joins two columns only at its bottom row.
        let (width, height) = (5, 6);
        let mut bits = vec![false; width * height];
        for y in 0..height {
            bits[y * width + 1] = true;
            bits[y * width + 3] = true;
        }
        bits[(height - 1) * width + 2] = true;
        let (found, _) = blobs(&bits, width, height, 100);
        assert!(found.len() <= 1);
        let (_, limited) = blobs(&bits, width, height, 100);
        assert!(!limited);
    }
}

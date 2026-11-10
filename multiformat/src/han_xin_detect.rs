//! Han Xin Code localization from its four corner finders.
//!
//! Every finder is a 3x3 dark block inside two nested L shapes. A line through
//! the block towards the L corner crosses dark and light runs in the ratio
//! 1:1:1:1:3, so row and column scans propose blocks in every rotation. Flood
//! fills of the block and of the inner L confirm a finder and give its
//! diagonal, the direction from the block to the L corner. The four finders
//! differ in where their block sits and where their L points, so any three
//! assign roles, fix the module grid and the symbol size, and the function
//! information decides between neighbouring sizes.
use crate::han_xin;
use crate::numeric::{f32_isize, usize_f32};
use crate::qr_detect::{homography, map};
use crate::{Detection, binarization, regions};

/// Finders kept per binary image.
const MAX_FINDERS: usize = 24;
/// Proposed blocks examined per binary image.
const MAX_PROPOSALS: usize = 64;

/// A confirmed finder in image coordinates.
#[derive(Clone, Copy, Debug)]
struct Finder {
    /// Block centroid.
    center: [f32; 2],
    /// Unit vector from the block towards the L corner.
    diagonal: [f32; 2],
    /// Module size.
    module: f32,
    /// Scan lines that proposed the block.
    support: usize,
}

/// A block proposed by a scan line.
#[derive(Clone, Copy, Debug)]
struct Proposal {
    block: [f32; 2],
    /// Inner L points of the merged scan lines, at least a module apart.
    inner: [[f32; 2]; 4],
    inners: usize,
    module: f32,
    support: usize,
}

/// Whether run lengths (dark first) have the finder ratio; returns the index of
/// the three-module run.
fn finder_runs(runs: &[usize; 5]) -> Option<(usize, f32)> {
    let total = usize_f32(runs.iter().sum());
    let module = total / 7.;
    if module < 1. {
        return None;
    }
    // Neighbouring runs are compared edge to edge, as a dark and a light run
    // together, so that a binarization threshold that thickens dark modules
    // does not shift the ratios.
    let pair = |a: usize, b: usize, modules: f32| {
        (0.7 * modules * module..=1.3 * modules * module).contains(&usize_f32(a + b))
    };
    let thin = |r: &usize| usize_f32(*r) <= 1.8 * module;
    let three = |r: usize| (2. * module..=4.5 * module).contains(&usize_f32(r));
    // Either orientation needs the three middle runs thin and paired, which
    // rejects most windows before the orientations are told apart.
    if !(runs[1..4].iter().all(thin) && pair(runs[1], runs[2], 2.) && pair(runs[2], runs[3], 2.)) {
        return None;
    }
    let fits = |big: usize| {
        let [a, b, c, d] = if big == 4 {
            [runs[0], runs[1], runs[2], runs[3]]
        } else {
            [runs[4], runs[3], runs[2], runs[1]]
        };
        three(runs[big])
            && [a, b, c, d].iter().all(thin)
            && pair(a, b, 2.)
            && pair(b, c, 2.)
            && pair(c, d, 2.)
            && pair(d, runs[big], 4.)
    };
    if fits(4) {
        Some((4, module))
    } else if fits(0) {
        Some((0, module))
    } else {
        None
    }
}

/// Proposals from the runs of one line. `offsets` holds the positions where the
/// line changes value, from 0 to its length; `at(i)` is the image point of the
/// line's pixel `i`.
fn scan_line(
    offsets: &[usize],
    first_dark: bool,
    at: impl Fn(f32) -> [f32; 2],
    proposals: &mut Vec<Proposal>,
) {
    // Run `i` lies between offsets `i` and `i + 1`. A window covers seven runs:
    // light, the five finder runs starting dark, light.
    for (k, window) in offsets.windows(8).enumerate() {
        if (k % 2 == 1) != first_dark {
            continue;
        }
        let length = |i: usize| window[i + 1] - window[i];
        let lengths = [1, 2, 3, 4, 5].map(length);
        let Some((big, module)) = finder_runs(&lengths) else {
            continue;
        };
        // Light runs on both sides: quiet zone, separator or data.
        if usize_f32(length(0)) < 0.5 * module || usize_f32(length(6)) < 0.5 * module {
            continue;
        }
        let center = |i: usize| at(usize_f32(window[i]) + usize_f32(length(i)) / 2.);
        let block = center(big + 1);
        let inner = center(3);
        proposals.push(Proposal {
            block,
            inner: [inner; 4],
            inners: 1,
            module,
            support: 1,
        });
    }
}

/// Moments of a flood-filled component.
#[derive(Clone, Copy, Debug, Default)]
struct Component {
    area: f32,
    center: [f32; 2],
    /// Central second moments xx, yy, xy.
    spread: [f32; 3],
}

/// The dark four-connected component at `seed`, when it stays within `radius`
/// pixels of the seed and below `max_area` pixels.
fn component(
    bits: &[bool],
    [width, height]: [usize; 2],
    start: [f32; 2],
    radius: usize,
    max_area: usize,
    seen: &mut Vec<bool>,
) -> Option<Component> {
    let sx = usize::try_from(f32_isize(start[0].floor())).ok()?;
    let sy = usize::try_from(f32_isize(start[1].floor())).ok()?;
    if sx >= width || sy >= height || !bits[sy * width + sx] {
        return None;
    }
    let (left, top) = (sx.saturating_sub(radius), sy.saturating_sub(radius));
    let (right, bottom) = ((sx + radius + 1).min(width), (sy + radius + 1).min(height));
    let span = right - left;
    seen.clear();
    seen.resize(span * (bottom - top), false);
    let mut stack = vec![(sx, sy)];
    let (mut n, mut x1, mut y1, mut xx, mut yy, mut xy) = (0usize, 0., 0., 0., 0., 0.);
    while let Some((x, y)) = stack.pop() {
        let at = (y - top) * span + x - left;
        if seen[at] {
            continue;
        }
        let row = y * width;
        let (mut a, mut b) = (x, x + 1);
        while a > left && bits[row + a - 1] && !seen[at - (x - a) - 1] {
            a -= 1;
        }
        while b < right && bits[row + b] && !seen[at + (b - x)] {
            b += 1;
        }
        // A component touching the window is larger than any finder part.
        if (a == left && left > 0) || (b == right && right < width) {
            return None;
        }
        if (y == top && top > 0) || (y + 1 == bottom && bottom < height) {
            return None;
        }
        seen[at - (x - a)..at + (b - x)].fill(true);
        n += b - a;
        if n > max_area {
            return None;
        }
        let fy = usize_f32(y) + 0.5;
        for px in a..b {
            let fx = usize_f32(px) + 0.5;
            x1 += fx;
            y1 += fy;
            xx += fx * fx;
            yy += fy * fy;
            xy += fx * fy;
        }
        for yy in [y.wrapping_sub(1), y + 1] {
            if yy < top || yy >= bottom {
                continue;
            }
            let mut xx = a;
            while xx < b {
                if bits[yy * width + xx] && !seen[(yy - top) * span + xx - left] {
                    stack.push((xx, yy));
                    while xx < b && bits[yy * width + xx] {
                        xx += 1;
                    }
                } else {
                    xx += 1;
                }
            }
        }
    }
    let area = usize_f32(n);
    let center = [x1 / area, y1 / area];
    Some(Component {
        area,
        center,
        spread: [
            xx / area - center[0] * center[0],
            yy / area - center[1] * center[1],
            xy / area - center[0] * center[1],
        ],
    })
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Confirm a proposal as a finder.
fn finder(
    bits: &[bool],
    size: [usize; 2],
    proposal: &Proposal,
    seen: &mut Vec<bool>,
) -> Option<Finder> {
    let m = proposal.module;
    let radius = crate::numeric::f32_usize((8. * m).ceil()) + 2;
    let limit = |modules: f32| crate::numeric::f32_usize((modules * m * m * 2.2).ceil()) + 4;
    let block = component(bits, size, proposal.block, radius, limit(9.), seen);
    let block = block?;
    let module = (block.area / 9.).sqrt();
    if !(0.5 * m..=1.3 * m).contains(&module) {
        return None;
    }
    let [cxx, cyy, cxy] = block.spread;
    let trace = cxx + cyy;
    // A square of side 3 has trace 1.5 module^2 and no anisotropy.
    if !(0.8..=1.25).contains(&(trace / (1.5 * module * module)))
        || ((cxx - cyy).powi(2) + 4. * cxy * cxy).sqrt() > 0.25 * trace
    {
        return None;
    }
    // The inner L's centroid is 2.67 modules from the block's, along the
    // diagonal. Scan lines near the edge of the finder's cone may end on data,
    // so every distinct seed is tried.
    let (offset, length) = proposal.inner[..proposal.inners].iter().find_map(|&seed| {
        let inner = component(bits, size, seed, radius, limit(9.), seen)?;
        if !(0.5..=1.8).contains(&(inner.area / (9. * module * module))) {
            return None;
        }
        let offset = [
            inner.center[0] - block.center[0],
            inner.center[1] - block.center[1],
        ];
        let length = offset[0].hypot(offset[1]);
        (0.75..=1.3)
            .contains(&(length / (2.671 * module)))
            .then_some((offset, length))
    })?;
    Some(Finder {
        center: block.center,
        diagonal: [offset[0] / length, offset[1] / length],
        module: f32::midpoint(module, length / 2.671),
        support: proposal.support,
    })
}

/// Block centre and diagonal of each finder in module coordinates of a symbol
/// with `s` modules per side: top left, top right, bottom right, bottom left.
fn roles(s: f32) -> [([f32; 2], [f32; 2]); 4] {
    let d = std::f32::consts::FRAC_1_SQRT_2;
    [
        ([5.5, 5.5], [-d, -d]),
        ([s - 5.5, 5.5], [d, -d]),
        ([s - 5.5, s - 5.5], [d, d]),
        ([1.5, s - 1.5], [d, -d]),
    ]
}

/// The affine map taking `from` to `to`, as a homography.
fn affine(from: [[f32; 2]; 3], to: [[f32; 2]; 3]) -> Option<[f32; 8]> {
    let [[x0, y0], [x1, y1], [x2, y2]] = from;
    let det = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if det.abs() < 1e-6 {
        return None;
    }
    let solve = |values: [f32; 3]| {
        let (du, dv) = (values[1] - values[0], values[2] - values[0]);
        let along_x = (du * (y2 - y0) - dv * (y1 - y0)) / det;
        let along_y = (dv * (x1 - x0) - du * (x2 - x0)) / det;
        [along_x, along_y, values[0] - along_x * x0 - along_y * y0]
    };
    let row_x = solve([to[0][0], to[1][0], to[2][0]]);
    let row_y = solve([to[0][1], to[1][1], to[2][1]]);
    Some([
        row_x[0], row_x[1], row_x[2], row_y[0], row_y[1], row_y[2], 0., 0.,
    ])
}

/// Image direction of the module-space vector `v` at module point `p`.
fn direction(t: &[f32; 8], p: [f32; 2], v: [f32; 2]) -> [f32; 2] {
    let start = map(t, p[0], p[1]);
    let end = map(t, p[0] + v[0], p[1] + v[1]);
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length = delta[0].hypot(delta[1]);
    [delta[0] / length, delta[1] / length]
}

fn sample(bits: &[bool], [width, height]: [usize; 2], point: [f32; 2]) -> bool {
    let x = f32_isize(point[0].floor());
    let y = f32_isize(point[1].floor());
    let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
        return false;
    };
    x < width && y < height && bits[y * width + x]
}

/// Function information read through `t` with its distance to the nearest
/// valid value: error correction level, mask and differing bits.
fn info(bits: &[bool], size: [usize; 2], t: &[f32; 8], s: usize) -> (usize, usize, u32) {
    let Some(version) = han_xin::version(s) else {
        return (0, 0, u32::MAX);
    };
    let mut read = [0u64; 2];
    for pair in han_xin::info_positions(s) {
        for (copy, &p) in read.iter_mut().zip(&pair) {
            let point = map(t, usize_f32(p % s) + 0.5, usize_f32(p / s) + 0.5);
            *copy = *copy << 1 | u64::from(sample(bits, size, point));
        }
    }
    han_xin::nearest_info(version, read)
}

/// A grid hypothesis: size and the finders it uses.
struct Hypothesis {
    s: usize,
    used: Vec<usize>,
}

/// Grid hypotheses of finders `chosen` in roles `assigned`.
fn hypothesis(finders: &[Finder], chosen: [usize; 3], assigned: [usize; 3]) -> Option<Hypothesis> {
    let module = chosen.iter().map(|&i| finders[i].module).sum::<f32>() / 3.;
    if chosen
        .iter()
        .any(|&i| !(0.6..=1.67).contains(&(finders[i].module / module)))
    {
        return None;
    }
    // Diagonals keep their mutual angles under the grid map.
    let unit = roles(0.);
    for a in 0..3 {
        for b in a + 1..3 {
            let (da, db) = (unit[assigned[a]].1, unit[assigned[b]].1);
            let expected = da[0] * db[0] + da[1] * db[1];
            let (fa, fb) = (finders[chosen[a]].diagonal, finders[chosen[b]].diagonal);
            if (fa[0] * fb[0] + fa[1] * fb[1] - expected).abs() > 0.4 {
                return None;
            }
        }
    }
    // Size whose module-space distances best match the image distances.
    let image = chosen.map(|i| finders[i].center);
    let mut best = (f32::MAX, 0);
    for s in han_xin::VERSIONS.map(han_xin::size) {
        let at = roles(usize_f32(s));
        let mut error = 0.;
        for a in 0..3 {
            for b in a + 1..3 {
                let predicted = distance(at[assigned[a]].0, at[assigned[b]].0) * module;
                error += (distance(image[a], image[b]) / predicted).ln().powi(2);
            }
        }
        if error < best.0 {
            best = (error, s);
        }
    }
    // Pair distances may disagree by a quarter (perspective, module estimate).
    if best.0 > 3. * 0.25f32.powi(2) {
        return None;
    }
    let at = roles(usize_f32(best.1));
    let t = affine(assigned.map(|r| at[r].0), image)?;
    for (k, &r) in assigned.iter().enumerate() {
        let d = direction(&t, at[r].0, at[r].1);
        let f = finders[chosen[k]].diagonal;
        if d[0] * f[0] + d[1] * f[1] < 0.85 {
            return None;
        }
    }
    Some(Hypothesis {
        s: best.1,
        used: chosen.to_vec(),
    })
}

/// Decode the symbol of a hypothesis, trying neighbouring sizes.
/// A read with its module-to-image map, size, function information errors
/// and the finders used.
type Decoded = (han_xin::Read, [f32; 8], usize, u32, Vec<usize>);

fn decode(
    bits: &[bool],
    size: [usize; 2],
    finders: &[Finder],
    hypothesis: &Hypothesis,
    assigned: [usize; 3],
) -> Option<Decoded> {
    let image: [[f32; 2]; 3] = std::array::from_fn(|k| finders[hypothesis.used[k]].center);
    let mut candidates = Vec::new();
    // Perspective changes the module size across the symbol, so the size is
    // known only to about an eighth.
    let spread = (hypothesis.s / 8).max(4);
    for s in hypothesis.s.saturating_sub(spread)..=hypothesis.s + spread {
        if han_xin::version(s).is_none() {
            continue;
        }
        let at = roles(usize_f32(s));
        let Some(mut t) = affine(assigned.map(|r| at[r].0), image) else {
            continue;
        };
        let mut used = hypothesis.used.clone();
        // A finder near the fourth role's predicted place turns the affine
        // map into a homography. Perspective moves it away from the
        // prediction in proportion to the symbol size.
        let fourth = (0..4).find(|r| !assigned.contains(r))?;
        let predicted = map(&t, at[fourth].0[0], at[fourth].0[1]);
        let facing = direction(&t, at[fourth].0, at[fourth].1);
        let module = hypothesis
            .used
            .iter()
            .map(|&i| finders[i].module)
            .sum::<f32>()
            / 3.;
        let reach = (usize_f32(s) / 5.).max(3.) * module;
        let other = (0..finders.len())
            .filter(|i| !hypothesis.used.contains(i))
            .filter(|&i| distance(finders[i].center, predicted) < reach)
            .filter(|&i| {
                let f = finders[i].diagonal;
                facing[0] * f[0] + facing[1] * f[1] > 0.7
            })
            .min_by(|&a, &b| {
                distance(finders[a].center, predicted)
                    .total_cmp(&distance(finders[b].center, predicted))
            });
        if let Some(other) = other {
            let mut from = [at[fourth].0; 4];
            let mut to = [finders[other].center; 4];
            for k in 0..3 {
                from[k] = at[assigned[k]].0;
                to[k] = image[k];
            }
            if let Some(h) = homography(from, to) {
                t = h;
                used.push(other);
            }
        }
        let (level, mask, errors) = info(bits, size, &t, s);
        if errors <= han_xin::MAX_INFO_ERRORS {
            candidates.push((errors, s, t, level, mask, used));
        }
    }
    candidates.sort_by_key(|c| (c.0, c.1.abs_diff(hypothesis.s)));
    for (errors, s, t, level, mask, used) in candidates {
        let version = han_xin::version(s)?;
        let modules: Vec<bool> = (0..s * s)
            .map(|i| {
                let point = map(&t, usize_f32(i % s) + 0.5, usize_f32(i / s) + 0.5);
                sample(bits, size, point)
            })
            .collect();
        if let Some(read) = han_xin::decode_data(&modules, version, level, mask) {
            return Some((read, t, s, errors, used));
        }
    }
    None
}

fn inside(polygon: &[[f32; 2]; 4], p: [f32; 2]) -> bool {
    let mut sign = 0.;
    for i in 0..4 {
        let (a, b) = (polygon[i], polygon[(i + 1) % 4]);
        let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        if cross * sign < 0. {
            return false;
        }
        if cross != 0. {
            sign = cross;
        }
    }
    true
}

/// Merged proposals by block position, one grid per octave of module size, so
/// that merging stays linear in images full of finder-like texture.
#[derive(Default)]
struct Index {
    /// Per level and cell, the proposals of that level.
    own: Cells,
    /// Per level and cell, the proposals of that level and all finer ones.
    upto: Cells,
    /// Bit `l` is set when level `l` has proposals of its own.
    levels: u32,
    /// Proposals with modules beyond the last level.
    large: Vec<usize>,
}

type Cells = std::collections::HashMap<
    (i32, isize, isize),
    Vec<usize>,
    std::hash::BuildHasherDefault<CellHasher>,
>;

/// A multiplicative hash of the cell key. Textured images look up millions of
/// cells, where the default keyed hash costs more than the merge itself.
#[derive(Default)]
struct CellHasher(u64);

impl CellHasher {
    fn mix(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for CellHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.mix(u64::from(byte));
        }
    }

    fn write_i32(&mut self, value: i32) {
        self.mix(u64::from(value.cast_unsigned()));
    }

    fn write_isize(&mut self, value: isize) {
        self.mix(u64::try_from(value.cast_unsigned()).unwrap_or(u64::MAX));
    }
}

impl Index {
    const LEVELS: i32 = 10;

    /// Largest module size of a level, exclusive.
    fn bound(level: i32) -> f32 {
        2f32.powi(level + 1)
    }

    fn level(p: &Proposal) -> Option<i32> {
        (0..Self::LEVELS).find(|&l| p.module < Self::bound(l))
    }

    /// The cell of a point; its edge is the merge distance of the level's
    /// largest module, so a match lies in the cell or one of its neighbours.
    fn key(level: i32, at: [f32; 2]) -> (i32, isize, isize) {
        let cell = 1.5 * Self::bound(level);
        (
            level,
            f32_isize((at[0] / cell).floor()),
            f32_isize((at[1] / cell).floor()),
        )
    }

    fn add(&mut self, p: &Proposal, at: usize) {
        let Some(level) = Self::level(p) else {
            self.large.push(at);
            return;
        };
        self.own
            .entry(Self::key(level, p.block))
            .or_default()
            .push(at);
        for l in level..Self::LEVELS {
            self.upto.entry(Self::key(l, p.block)).or_default().push(at);
        }
        self.levels |= 1 << level;
    }

    /// The first merged proposal within merge distance of `p`.
    fn find(&self, merged: &[Proposal], p: &Proposal) -> Option<usize> {
        let mut first: Option<usize> = None;
        let mut consider = |i: usize| {
            let q = &merged[i];
            let reach = 1.5 * q.module.max(p.module);
            // The distance is at least either coordinate difference, which
            // settles most candidates without a square root.
            if first.is_none_or(|f| i < f)
                && (q.block[0] - p.block[0]).abs() < reach
                && (q.block[1] - p.block[1]).abs() < reach
                && distance(q.block, p.block) < reach
            {
                first = Some(i);
            }
        };
        let mut around = |cells: &Cells, level: i32| {
            let (_, x, y) = Self::key(level, p.block);
            for y in y - 1..=y + 1 {
                for x in x - 1..=x + 1 {
                    if let Some(items) = cells.get(&(level, x, y)) {
                        items.iter().copied().for_each(&mut consider);
                    }
                }
            }
        };
        // Finer and equal proposals merge within the distance of `p`'s level,
        // coarser ones within the distance of their own.
        let from = if let Some(level) = Self::level(p) {
            around(&self.upto, level);
            level + 1
        } else {
            (0..merged.len()).for_each(&mut consider);
            return first;
        };
        for level in (from..Self::LEVELS).filter(|l| self.levels >> l & 1 == 1) {
            around(&self.own, level);
        }
        self.large.iter().copied().for_each(&mut consider);
        first
    }
}

/// Finders of one binary image; `true` when a cap dropped candidates.
fn finders(bits: &[bool], [width, height]: [usize; 2]) -> (Vec<Finder>, bool) {
    /// Columns transposed together, so that column scans read memory in order.
    const STRIP: usize = 64;
    let mut proposals = Vec::new();
    let mut offsets = Vec::new();
    if width == 0 || height == 0 {
        return (Vec::new(), false);
    }
    // The image may be longer than its `height` rows.
    let bits = &bits[..width * height];
    for (y, row) in bits.chunks_exact(width).enumerate() {
        let fy = usize_f32(y) + 0.5;
        crate::transition_offsets_into(row, &mut offsets);
        scan_line(&offsets, row[0], |x| [x, fy], &mut proposals);
    }
    let mut strip = vec![false; STRIP.min(width) * height];
    for left in (0..width).step_by(STRIP) {
        let columns = STRIP.min(width - left);
        for (y, row) in bits.chunks_exact(width).enumerate() {
            for (column, &bit) in row[left..left + columns].iter().enumerate() {
                strip[column * height + y] = bit;
            }
        }
        for (column, line) in strip.chunks_exact(height).take(columns).enumerate() {
            let fx = usize_f32(left + column) + 0.5;
            crate::transition_offsets_into(line, &mut offsets);
            scan_line(&offsets, line[0], |y| [fx, y], &mut proposals);
        }
    }
    // Merge proposals of the same block; isolated proposals are noise.
    let mut merged: Vec<Proposal> = Vec::new();
    let mut index = Index::default();
    for p in proposals {
        if let Some(at) = index.find(&merged, &p) {
            let q = &mut merged[at];
            q.support += 1;
            let seed = p.inner[0];
            if q.inners < 4
                && q.inner[..q.inners]
                    .iter()
                    .all(|&i| distance(i, seed) > q.module)
            {
                q.inner[q.inners] = seed;
                q.inners += 1;
            }
        } else {
            index.add(&p, merged.len());
            merged.push(p);
        }
    }
    merged.retain(|p| p.support >= 2);
    merged.sort_by_key(|p| std::cmp::Reverse(p.support));
    let mut limited = merged.len() > MAX_PROPOSALS;
    let mut seen = Vec::new();
    let mut found: Vec<Finder> = Vec::new();
    for p in merged.iter().take(MAX_PROPOSALS) {
        let Some(f) = finder(bits, [width, height], p, &mut seen) else {
            continue;
        };
        if found
            .iter()
            .any(|g| distance(g.center, f.center) < 2. * g.module)
        {
            continue;
        }
        if found.len() == MAX_FINDERS {
            limited = true;
            break;
        }
        found.push(f);
    }
    (found, limited)
}

/// Read Han Xin symbols from the binary images.
pub fn detect(
    width: usize,
    height: usize,
    regions: &mut regions::Regions,
    images: &mut binarization::Images<'_>,
) -> (Vec<Detection>, bool) {
    let size = [width, height];
    let mut results: Vec<Detection> = Vec::new();
    let mut limited = false;
    for mode in 0..4 {
        if images.is_duplicate(mode) {
            continue;
        }
        let bits = images.get(mode);
        let (finders, cut) = finders(bits, size);
        limited |= cut;
        let mut used = vec![false; finders.len()];
        for i in 0..finders.len() {
            if results
                .iter()
                .any(|d| inside(&d.polygon, finders[i].center))
            {
                used[i] = true;
            }
        }
        for a in 0..finders.len() {
            for b in a + 1..finders.len() {
                for c in b + 1..finders.len() {
                    let chosen = [a, b, c];
                    'roles: for assigned in permutations() {
                        if chosen.iter().any(|&i| used[i]) {
                            break 'roles;
                        }
                        let Some(h) = hypothesis(&finders, chosen, assigned) else {
                            continue;
                        };
                        let Some((read, t, s, errors, decoded)) =
                            decode(bits, size, &finders, &h, assigned)
                        else {
                            continue;
                        };
                        let extent = usize_f32(s);
                        let polygon = [[0., 0.], [extent, 0.], [extent, extent], [0., extent]]
                            .map(|[x, y]| map(&t, x, y));
                        let support = decoded.iter().map(|&i| finders[i].support).sum();
                        regions.add(
                            "HanXin",
                            polygon,
                            1. - f32::from(u16::try_from(errors).unwrap_or(68)) / 68.,
                            support,
                        );
                        for &i in &decoded {
                            used[i] = true;
                        }
                        results.push(Detection {
                            assembled_rows: false,
                            bytes: Some(read.bytes),
                            structured_append: None,
                            reader_initialization: false,
                            addon: None,
                            codabar_start_stop: None,
                            format: "HanXin".into(),
                            text: read.text,
                            polygon,
                            support,
                            error: usize_f32(read.corrected),
                            gs1: false,
                        });
                        break 'roles;
                    }
                }
            }
        }
    }
    (results, limited)
}

/// The 24 ordered choices of three of the four roles.
fn permutations() -> impl Iterator<Item = [usize; 3]> {
    (0..4).flat_map(|a| {
        (0..4).flat_map(move |b| {
            (0..4)
                .filter(move |&c| a != b && a != c && b != c)
                .map(move |c| [a, b, c])
        })
    })
}

#[cfg(test)]
pub(crate) mod testing {
    /// Modules of a `zint --dump` matrix, rows of hex bytes, leftmost module
    /// in the high bit.
    pub fn zint_modules(rows: &[&str]) -> (Vec<bool>, usize) {
        let s = rows.len();
        let mut modules = Vec::with_capacity(s * s);
        for row in rows {
            let bits = row
                .split(' ')
                .flat_map(|h| {
                    let byte = u8::from_str_radix(&format!("{h:0<2}"), 16).unwrap();
                    (0..8).map(move |i| byte & 0x80 >> i != 0)
                })
                .take(s);
            modules.extend(bits);
        }
        (modules, s)
    }

    /// A gray image of the matrix with a quiet zone of three modules, rotated
    /// by `degrees`, 4x4 supersampled and box-blurred `blur` times.
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
    #[allow(clippy::cast_sign_loss)] // test renderer: small positive coordinates
    pub fn render(
        modules: &[bool],
        s: usize,
        module: f32,
        degrees: f32,
        blur: usize,
    ) -> (Vec<u8>, usize, usize) {
        let extent = (s as f32 + 6.) * module;
        let size = (extent * 1.45) as usize;
        let (sin, cos) = degrees.to_radians().sin_cos();
        let centre = size as f32 / 2.;
        let mut gray = vec![0u8; size * size];
        for y in 0..size {
            for x in 0..size {
                let mut dark = 0;
                for k in 0..16 {
                    let px = x as f32 + (k % 4) as f32 / 4. + 0.125 - centre;
                    let py = y as f32 + (k / 4) as f32 / 4. + 0.125 - centre;
                    let u = (cos * px + sin * py) / module + s as f32 / 2.;
                    let v = (-sin * px + cos * py) / module + s as f32 / 2.;
                    if u >= 0. && v >= 0. && (u as usize) < s && (v as usize) < s {
                        dark += i32::from(modules[v as usize * s + u as usize]);
                    }
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
    use super::testing::{render, zint_modules};
    use super::*;

    fn scan(gray: &[u8], width: usize, height: usize) -> Vec<String> {
        let mut images = binarization::Images::new(gray, width, height);
        let mut regions = regions::Regions::default();
        detect(width, height, &mut regions, &mut images)
            .0
            .into_iter()
            .map(|d| d.text)
            .collect()
    }

    /// zint --dump --barcode=HANXIN --vers=4 -d "汉信码 Han Xin 2026"
    const VERSION_4: [&str; 29] = [
        "FE 2A D3 F8",
        "80 08 B0 08",
        "BE EF EB E8",
        "A0 D5 48 28",
        "AE 41 43 A8",
        "AE A8 93 A8",
        "AE 93 4B A8",
        "00 BD 08 00",
        "18 A7 66 00",
        "E3 BA 2B E8",
        "8A 60 67 50",
        "C4 27 4D 40",
        "A7 A3 35 58",
        "89 A5 E1 C0",
        "FF FF 2F A8",
        "00 02 DF 80",
        "3E 52 C3 C0",
        "CE 5A A9 E0",
        "CC 12 D2 48",
        "F7 52 CA E0",
        "03 22 98 C0",
        "00 D2 88 00",
        "FE E3 0B A8",
        "02 1B DB A8",
        "FA 53 D3 A8",
        "0A 92 28 28",
        "EA BA 8B E8",
        "EA 33 20 08",
        "EA 63 F3 F8",
    ];

    #[test]
    fn reads_rendered_symbols() {
        let (modules, s) = zint_modules(&VERSION_4);
        assert_eq!(
            han_xin::decode_matrix(&modules, s)
                .map(|r| r.text)
                .as_deref(),
            Some("汉信码 Han Xin 2026")
        );
        for (module, degrees, blur) in [(4., 0., 0), (3., 30., 0), (4., 135., 1), (3., 250., 1)] {
            let (gray, width, height) = render(&modules, s, module, degrees, blur);
            assert_eq!(
                scan(&gray, width, height),
                ["汉信码 Han Xin 2026"],
                "{module} {degrees} {blur}"
            );
        }
    }

    #[test]
    fn rows_past_the_height_are_ignored() {
        let (modules, s) = zint_modules(&VERSION_4);
        let (mut gray, width, height) = render(&modules, s, 4., 0., 0);
        gray.extend((0..3 * width).map(|i| if i / 7 % 2 == 0 { 0 } else { 255 }));
        assert_eq!(scan(&gray, width, height), ["汉信码 Han Xin 2026"]);
    }
}

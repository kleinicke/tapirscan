//! Template-matched re-read of a rectified PDF417 crop.
//!
//! Print gain and blur fill the narrow gaps of a row, so its run lengths stop
//! spelling codewords and the run reader loses whole rows. The start guard still
//! gives the left edge and the module width, and the end of the rows gives the
//! column count. Each codeword is then matched, as a blurred bar pattern,
//! against the pixels it covers, row by row, and the matches vote like the cells
//! of the run reader.
use super::{GUESS, Row, accepted_start, decode_group};
use crate::{Detection, numeric, pdf417_tables::CODES};
use std::{cell::OnceCell, collections::BTreeMap};

/// Edge blur of a modelled bar, in modules.
const BLUR: f32 = 0.3;
/// Lines of a crop that must show the same start guard, and the same end.
const MIN_LINES: usize = 4;
/// Lines of each row that vote, and the lines of it read to find them.
const BEST_LINES: usize = 2;
const CANDIDATE_LINES: usize = 3;
/// Widest column count tried (the format allows 30).
const MAX_CELLS: usize = 34;

struct Scanline {
    pixels: Vec<u8>,
    /// Left edge and module width of an accepted start guard.
    start: Option<(f32, f32)>,
    /// Dark runs: end edge, width, and the light run before and after.
    bars: Vec<[f32; 4]>,
}

fn scanline(pixels: Vec<u8>) -> Scanline {
    let bits = crate::threshold(&pixels, 0);
    let offsets = crate::run_offsets(&bits);
    let mut widths = Vec::new();
    crate::refine_run_offsets_into(&offsets, &pixels, false, &mut widths);
    let first = usize::from(!bits[0]);
    let mut edge = 0.;
    let mut bars = Vec::new();
    let mut edges = Vec::with_capacity(widths.len());
    for (i, &width) in widths.iter().enumerate() {
        edge += width;
        edges.push(edge);
        if i % 2 == first % 2 {
            let before = if i == 0 { 0. } else { widths[i - 1] };
            let after = widths.get(i + 1).copied().unwrap_or(f32::INFINITY);
            bars.push([edge, width, before, after]);
        }
    }
    let start = (first..widths.len().saturating_sub(9))
        .step_by(2)
        .find(|&s| {
            accepted_start(&offsets, s, &pixels, false)
                && crate::linear::pattern_error(&widths[s..], &[8, 1, 1, 1, 1, 1, 1, 3]) <= 0.18
        })
        .map(|s| {
            let left = if s == 0 { 0. } else { edges[s - 1] };
            let module = widths[s..s + 8].iter().sum::<f32>() / 17.;
            // Print gain widens the first bar of the guard by the amount it
            // moves the bar's left edge outwards.
            (left + (widths[s] - 8. * module) / 2., module)
        });
    Scanline {
        pixels,
        start,
        bars,
    }
}

fn median(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// Gaussian CDF (Abramowitz and Stegun 7.1.26 for the error function).
fn phi(x: f32) -> f32 {
    let z = x.abs() * std::f32::consts::FRAC_1_SQRT_2;
    let t = 1. / (1. + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152_1 + t * 1.061_405_4))));
    let erf = 1. - poly * (-z * z).exp();
    f32::midpoint(1., erf.copysign(x))
}

/// Dark coverage of every codeword, as blurred bars seen through one pixel window.
struct Templates {
    /// Window start relative to the whole pixel of the codeword's origin.
    first: isize,
    /// Pixels merged into one sample.
    stride: usize,
    /// Samples of the window, padded to a multiple of eight.
    samples: usize,
    /// Cluster, value, sample.
    cover: Vec<f32>,
    norm: Vec<f32>,
}

/// Dot product in eight independent sums, which vectorize.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sums = [0f32; 8];
    for (a, b) in a.as_chunks::<8>().0.iter().zip(b.as_chunks::<8>().0) {
        for ((sum, x), y) in sums.iter_mut().zip(a).zip(b) {
            *sum += x * y;
        }
    }
    sums.iter().sum()
}

impl Templates {
    /// Templates for an origin `fraction` pixels right of a pixel edge.
    fn new(fraction: f32, module: f32) -> Self {
        let first = numeric::f32_isize((fraction - module).floor());
        let stride = if module >= 3. { 2 } else { 1 };
        let pixels = numeric::f32_usize((module * 19.).ceil()) + 2;
        let used = pixels.div_ceil(stride);
        let samples = used.div_ceil(8) * 8;
        let sigma = module * BLUR;
        // Module `i` of the window spans `fraction + (i - 1) * module` for one
        // module: a white neighbour, the 17 modules of the codeword, a black one.
        let at = |x: usize, boundary: usize| {
            let center = numeric::isize_f32(first)
                + numeric::usize_f32(x * stride)
                + numeric::usize_f32(stride) / 2.;
            phi((center - fraction - (numeric::usize_f32(boundary) - 1.) * module) / sigma)
        };
        let table: Vec<[f32; 20]> = (0..used)
            .map(|x| std::array::from_fn(|boundary| at(x, boundary)))
            .collect();
        let mut cover = vec![0.; 3 * 929 * samples];
        let mut norm = vec![0.; 3 * 929];
        for (cluster, row) in CODES.iter().enumerate() {
            for (value, &pattern) in row.iter().enumerate() {
                let out = &mut cover[(cluster * 929 + value) * samples..][..samples];
                let mut previous = false;
                let mut begin = 0;
                for module in 1..=18 {
                    let black = module == 18 || (pattern >> (17 - module)) & 1 == 1;
                    if black && !previous {
                        begin = module;
                    }
                    if !black && previous {
                        for (o, p) in out.iter_mut().zip(&table) {
                            *o += p[begin] - p[module];
                        }
                    }
                    previous = black;
                }
                for (o, p) in out.iter_mut().zip(&table) {
                    *o += p[18] - p[19];
                }
                norm[cluster * 929 + value] = dot(out, out);
            }
        }
        Self {
            first,
            stride,
            samples,
            cover,
            norm,
        }
    }

    /// Mean squared error of the best codeword of `clusters` for the pixels of a
    /// line from `start` on, with the codeword's cluster and value.
    fn best(
        &self,
        line: &[u8],
        start: isize,
        clusters: std::ops::Range<usize>,
    ) -> Option<(f32, usize, usize)> {
        let pixel = |at: isize| {
            usize::try_from(at)
                .ok()
                .and_then(|at| line.get(at))
                .map_or(255., |&v| f32::from(v))
        };
        let mut values = vec![0f32; self.samples];
        let mut used = 0;
        for (k, value) in values.iter_mut().enumerate() {
            let at = start + (k * self.stride).cast_signed();
            if at >= line.len().cast_signed() {
                break;
            }
            *value = (0..self.stride)
                .map(|i| pixel(at + i.cast_signed()))
                .sum::<f32>()
                / numeric::usize_f32(self.stride);
            used = k + 1;
        }
        if used < 8 {
            return None;
        }
        let mut sorted = values[..used].to_vec();
        sorted.sort_by(f32::total_cmp);
        let take = (used / 8).max(1);
        let low = sorted[..take].iter().sum::<f32>() / numeric::usize_f32(take);
        let high = sorted[used - take..].iter().sum::<f32>() / numeric::usize_f32(take);
        if high - low < 40. {
            return None;
        }
        for value in &mut values[..used] {
            *value = ((high - *value) / (high - low)).clamp(-0.1, 1.1);
        }
        let energy = dot(&values, &values);
        let mut best = (f32::INFINITY, 0, 0);
        for cluster in clusters {
            for value in 0..929 {
                let index = cluster * 929 + value;
                let template = &self.cover[index * self.samples..][..self.samples];
                let error = energy - 2. * dot(template, &values) + self.norm[index];
                if error < best.0 {
                    best = (error, cluster, value);
                }
            }
        }
        Some((best.0 / numeric::usize_f32(used), best.1, best.2))
    }
}

/// A straight edge of the symbol: the position `at` row 0 and the drift per row.
#[derive(Clone, Copy)]
struct Edge(f32, f32);

impl Edge {
    fn at(self, y: usize) -> f32 {
        self.0 + self.1 * numeric::usize_f32(y)
    }

    /// Least-squares line through `(row, position)` points, refitted once
    /// without the points more than `tolerance` away from the first fit.
    fn fit(points: &[(usize, f32)], tolerance: f32) -> Option<Self> {
        let line = |points: &[(usize, f32)]| {
            let fit = crate::pdf_localize::fit_line(
                points.iter().map(|&(y, x)| [x, numeric::usize_f32(y)]),
            );
            Self(fit.intercept, fit.slope)
        };
        if points.len() < MIN_LINES {
            return None;
        }
        let rough = line(points);
        let kept: Vec<(usize, f32)> = points
            .iter()
            .copied()
            .filter(|&(y, x)| (x - rough.at(y)).abs() <= tolerance)
            .collect();
        (kept.len() >= MIN_LINES).then(|| line(&kept))
    }
}

struct Layout {
    left: Edge,
    right: Edge,
    /// Modules from the left edge to the right edge.
    length: f32,
    /// Data columns.
    columns: usize,
    /// Right row indicator and full stop pattern, not a compact symbol's single bar.
    full: bool,
}

impl Layout {
    fn module(&self, y: usize) -> f32 {
        (self.right.at(y) - self.left.at(y)) / self.length
    }

    fn origin(&self, y: usize, cell: usize) -> f32 {
        self.left.at(y) + self.module(y) * 17. * numeric::usize_f32(cell + 1)
    }
}

/// Templates for the four quarter-pixel fractions of a cell origin, built on first use.
struct Bank {
    module: f32,
    sets: [OnceCell<Templates>; 4],
}

impl Bank {
    fn new(module: f32) -> Self {
        Self {
            module,
            sets: Default::default(),
        }
    }

    fn get(&self, quarter: usize) -> &Templates {
        self.sets[quarter]
            .get_or_init(|| Templates::new(numeric::usize_f32(quarter) / 4., self.module))
    }
}

/// Best codeword of `clusters` for a cell whose origin is `origin`, over the
/// sub-pixel offsets in `offsets` (quarter pixels).
fn match_cell(
    bank: &Bank,
    line: &[u8],
    origin: f32,
    offsets: &[isize],
    clusters: std::ops::Range<usize>,
) -> Option<(f32, usize, usize, isize)> {
    let quarter = numeric::f32_isize((origin * 4.).round());
    offsets
        .iter()
        .filter_map(|&offset| {
            let at = quarter + offset;
            let set = bank.get(usize::try_from(at.rem_euclid(4)).unwrap_or(0));
            let (error, cluster, value) =
                set.best(line, at.div_euclid(4) + set.first, clusters.clone())?;
            Some((error, cluster, value, offset))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// Read every cell of a line of known cluster, each at its best of the
/// sub-pixel `offsets`: the codewords and the mean error.
fn read_line(
    bank: &Bank,
    layout: &Layout,
    y: usize,
    line: &[u8],
    cluster: usize,
    offsets: &[isize],
) -> Option<(Vec<usize>, f32)> {
    let cells = layout.columns + 1 + usize::from(layout.full);
    let mut values = Vec::with_capacity(cells);
    let mut total = 0.;
    // Each cell is found where the one before it was, so a grid that drifts
    // from the straight-edge model is followed.
    let mut shift = 0.;
    for cell in 0..cells {
        let (error, _, value, offset) = match_cell(
            bank,
            line,
            layout.origin(y, cell) + shift,
            offsets,
            cluster..cluster + 1,
        )?;
        shift += numeric::isize_f32(offset) / 4.;
        values.push(value);
        total += error;
    }
    Some((values, total / numeric::usize_f32(cells)))
}

fn read_lines(lines: &[Scanline], layout: &Layout, module: f32) -> Vec<Row> {
    let bank = Bank::new(module);
    let step = numeric::f32_isize((module * 0.6).round()).clamp(1, 3);
    // The left indicator tells each line's cluster.
    let first: Vec<Option<(f32, usize, usize)>> = lines
        .iter()
        .enumerate()
        .map(|(y, line)| {
            match_cell(&bank, &line.pixels, layout.origin(y, 0), &[0], 0..3)
                .map(|m| (m.0, m.1, m.2))
        })
        .collect();
    // Row indicators repeat per cluster: cluster 2 carries the column count and
    // the others one value each for the whole symbol. A line that disagrees lies
    // between two rows (or does not belong to the symbol) and is not read further.
    let mut votes = [[0usize; 30]; 3];
    for &(_, cluster, value) in first.iter().flatten() {
        votes[cluster][value % 30] += 1;
    }
    let expected: Vec<Option<usize>> = (0..3)
        .map(|cluster| {
            if cluster == 2 {
                Some(layout.columns - 1)
            } else {
                let (value, &count) = votes[cluster].iter().enumerate().max_by_key(|(_, n)| **n)?;
                (count >= MIN_LINES).then_some(value)
            }
        })
        .collect();
    // Inside a row, neighbouring lines are alike; between two rows they differ.
    let change = |y: usize| -> f32 {
        let from = numeric::f32_usize(layout.left.at(y).max(0.));
        let to = numeric::f32_usize(layout.right.at(y).max(0.)).min(lines[y].pixels.len());
        [y.checked_sub(1), Some(y + 1)]
            .into_iter()
            .flatten()
            .filter_map(|n| lines.get(n))
            .map(|n| {
                lines[y].pixels[from.min(to)..to]
                    .iter()
                    .zip(&n.pixels[from.min(to)..to])
                    .map(|(&a, &b)| f32::from(a.abs_diff(b)))
                    .sum::<f32>()
            })
            .sum()
    };
    let mut by_row: BTreeMap<usize, Vec<(f32, usize, usize)>> = BTreeMap::new();
    for (y, f) in first.into_iter().enumerate() {
        if let Some((_, cluster, value)) = f.filter(|f| expected[f.1] == Some(f.2 % 30)) {
            by_row
                .entry(value / 30 * 3 + cluster)
                .or_default()
                .push((change(y), y, cluster));
        }
    }
    for row in by_row.values_mut() {
        row.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    let offsets = [-step, 0, step];
    // Lines that straddle two rows fit worse than the lines at a row's centre:
    // only the best few lines of each row are read in full.
    let mut chosen: Vec<(f32, usize, usize, Vec<usize>)> = Vec::new();
    for row in by_row.values() {
        let mut read: Vec<_> = row
            .iter()
            .take(CANDIDATE_LINES)
            .filter_map(|&(_, y, cluster)| {
                let (values, error) =
                    read_line(&bank, layout, y, &lines[y].pixels, cluster, &offsets)?;
                Some((error, y, cluster, values))
            })
            .collect();
        read.sort_by(|a, b| a.0.total_cmp(&b.0));
        chosen.extend(read.into_iter().take(BEST_LINES));
    }
    chosen.sort_by_key(|c| c.1);
    chosen
        .into_iter()
        .map(|(_, y, cluster, values)| {
            let cells = values.len();
            Row {
                left: Some(values[0]),
                right: layout.full.then(|| values[cells - 1]),
                cluster,
                values: values[1..=layout.columns]
                    .iter()
                    .map(|&v| Some(v + GUESS))
                    .collect(),
                x0: layout.left.at(y),
                x1: layout.right.at(y),
                y: numeric::usize_f32(y),
            }
        })
        .collect()
}

/// The lines of a crop whose rows run left to right, and where its start guards lie.
struct Crop {
    lines: Vec<Scanline>,
    /// Left edge of the symbol, as drawn by its start guards.
    left: Edge,
    /// Module width of the guards.
    module: f32,
    /// Lines whose start guard agrees with `left`.
    aligned: Vec<usize>,
}

impl Crop {
    /// The column count that most cluster-2 left row indicators spell.
    fn indicator_columns(&self) -> Option<usize> {
        let bank = Bank::new(self.module);
        let mut votes = [0usize; 30];
        for &y in &self.aligned {
            let origin = self.left.at(y) + self.module * 17.;
            if let Some((_, 2, value, _)) =
                match_cell(&bank, &self.lines[y].pixels, origin, &[0], 0..3)
            {
                votes[value % 30] += 1;
            }
        }
        let (best, &count) = votes.iter().enumerate().max_by_key(|(_, n)| **n)?;
        (count >= MIN_LINES && count * 2 > votes.iter().sum::<usize>()).then_some(best + 1)
    }

    fn new(gray: &[u8], w: usize, h: usize, mirrored: bool) -> Option<Self> {
        let lines: Vec<Scanline> = (0..h)
            .map(|y| {
                let mut pixels = gray[y * w..(y + 1) * w].to_vec();
                if mirrored {
                    pixels.reverse();
                }
                scanline(pixels)
            })
            .collect();
        let starts: Vec<(usize, f32, f32)> = lines
            .iter()
            .enumerate()
            .filter_map(|(y, l)| l.start.map(|(left, module)| (y, left, module)))
            .collect();
        if starts.len() < MIN_LINES {
            return None;
        }
        let module = median(&mut starts.iter().map(|s| s.2).collect::<Vec<_>>());
        let alike = |m: f32| (m / module - 1.).abs() < 0.08;
        let points: Vec<(usize, f32)> = starts
            .iter()
            .filter(|s| alike(s.2))
            .map(|s| (s.0, s.1))
            .collect();
        let left = Edge::fit(&points, module)?;
        let aligned: Vec<usize> = starts
            .iter()
            .filter(|s| (s.1 - left.at(s.0)).abs() < module && alike(s.2))
            .map(|s| s.0)
            .collect();
        (aligned.len() >= MIN_LINES).then_some(Self {
            lines,
            left,
            module,
            aligned,
        })
    }

    /// Every span (in 17-module cells, the start guard included) whose end is a
    /// lone bar one module wide at the same place in most lines, with the share
    /// of lines that agree and the right edge.
    fn spans(&self, w: usize) -> Vec<(f32, usize, Edge)> {
        let module = self.module;
        let mut spans = Vec::new();
        for span in 3..=MAX_CELLS {
            let length = 17. * numeric::usize_f32(span) + 1.;
            if self.left.at(0) + module * length - module > numeric::usize_f32(w) {
                break;
            }
            let window = (0.04 * length).max(3.) * module;
            let ends: Vec<(usize, f32)> = self
                .aligned
                .iter()
                .filter_map(|&y| {
                    let predicted = self.left.at(y) + module * length;
                    self.lines[y]
                        .bars
                        .iter()
                        .filter(|&&[end, width, before, after]| {
                            (end - predicted).abs() <= window
                                && (0.5 * module..=1.9 * module).contains(&width)
                                && before >= 0.7 * module
                                && after >= 2. * module
                        })
                        // The stop pattern's inner bars qualify too: prefer the bar
                        // with the widest light run after it, then the nearest.
                        .min_by(|a, b| {
                            let room = |bar: &[f32; 4]| bar[3].min(5. * module);
                            room(b).total_cmp(&room(a)).then_with(|| {
                                (a[0] - predicted)
                                    .abs()
                                    .total_cmp(&(b[0] - predicted).abs())
                            })
                        })
                        // Print gain widens the bar on both sides.
                        .map(|bar| (y, bar[0] - (bar[1] - module) / 2.))
                })
                .collect();
            let Some(right) = Edge::fit(&ends, 0.75 * module) else {
                continue;
            };
            let agree = ends
                .iter()
                .filter(|&&(y, x)| (x - right.at(y)).abs() < 0.75 * module)
                .count();
            let middle = self.lines.len() / 2;
            let exact = (right.at(middle) - self.left.at(middle)) / length;
            if (exact / module - 1.).abs() <= 0.035 {
                let share = numeric::usize_f32(agree) / numeric::usize_f32(self.aligned.len());
                spans.push((share, span, right));
            }
        }
        spans.sort_by(|a, b| b.0.total_cmp(&a.0));
        spans
    }

    /// A full symbol ends in a stop pattern whose first bar is seven modules
    /// wide; a compact one ends right after its last codeword and one bar.
    fn is_full(&self, right: Edge) -> bool {
        let dark = self
            .aligned
            .iter()
            .filter(|&&y| {
                let pixels = &self.lines[y].pixels;
                let end = right.at(y);
                let from = numeric::f32_usize((end - 17.2 * self.module).max(0.));
                let to = numeric::f32_usize((end - 11.8 * self.module).max(0.)).min(pixels.len());
                let bar = &pixels[from.min(to)..to];
                let low = pixels.iter().copied().min().unwrap_or(0);
                let high = pixels.iter().copied().max().unwrap_or(255);
                let mid =
                    u8::try_from(u16::midpoint(u16::from(low), u16::from(high))).unwrap_or(128);
                !bar.is_empty() && bar.iter().all(|&v| v <= mid)
            })
            .count();
        dark * 2 > self.aligned.len()
    }
}

/// Decode a crop whose rows run left to right (`mirrored` flips them).
fn read_oriented(gray: &[u8], w: usize, h: usize, mirrored: bool) -> Option<Detection> {
    let crop = Crop::new(gray, w, h, mirrored)?;
    for &(share, span, right) in crop.spans(w).iter().take(2) {
        if share < 0.3 {
            break;
        }
        // The indicators settle whether the last cell is a right indicator
        // before a stop pattern (full) or a data column (compact); blur can hide
        // the stop pattern's wide bar from the darkness test.
        let full = match crop.indicator_columns() {
            Some(c) if c + 4 == span => true,
            Some(c) if c + 2 == span => false,
            _ => crop.is_full(right),
        };
        let Some(columns) = span
            .checked_sub(if full { 4 } else { 2 })
            .filter(|&c| c > 0)
        else {
            continue;
        };
        let layout = Layout {
            left: crop.left,
            right,
            length: 17. * numeric::usize_f32(span) + 1.,
            columns,
            full,
        };
        let rows = read_lines(&crop.lines, &layout, crop.module);
        let Some((read, corrected)) = decode_group(&rows) else {
            continue;
        };
        let (top, bottom) = rows
            .iter()
            .fold((f32::INFINITY, 0f32), |(t, b), r| (t.min(r.y), b.max(r.y)));
        let (x0, x1) = (layout.left.at(h / 2), layout.right.at(h / 2));
        let (x0, x1) = if mirrored {
            (numeric::usize_f32(w) - x1, numeric::usize_f32(w) - x0)
        } else {
            (x0, x1)
        };
        return Some(Detection {
            bytes: Some(read.bytes),
            structured_append: read.structured_append,
            reader_initialization: read.reader_initialization,
            addon: None,
            format: "PDF417".into(),
            text: read.text,
            polygon: [[x0, top], [x1, top], [x1, bottom], [x0, bottom]],
            support: rows.len(),
            error: numeric::usize_f32(corrected),
            gs1: false,
        });
    }
    None
}

/// Template-matched read of a rectified crop, in either row direction.
pub(super) fn read(gray: &[u8], w: usize, h: usize) -> Vec<Detection> {
    [false, true]
        .into_iter()
        .find_map(|mirrored| read_oriented(gray, w, h, mirrored))
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Edge, MIN_LINES, phi, read};
    use crate::{pdf417_tables::CODES, reed_prime};

    const ROWS: usize = 9;
    const COLUMNS: usize = 4;
    const LEVEL: usize = 1;
    const TEXT: &str = "TAPIRSCAN GRID TESTS";

    /// Data codewords of `TEXT` in text compaction, padded, followed by the check codewords.
    fn codewords() -> Vec<usize> {
        let checks = 2 << LEVEL;
        let letters: Vec<usize> = TEXT
            .bytes()
            .map(|b| if b == b' ' { 26 } else { usize::from(b - b'A') })
            .collect();
        let mut code = vec![0];
        code.extend(letters.chunks(2).map(|pair| pair[0] * 30 + pair[1]));
        code.resize(ROWS * COLUMNS - checks, 900);
        code[0] = code.len();
        // Remainder of the data polynomial modulo the product of (x - 3^i).
        let mut generator = vec![1usize];
        let mut root = 1;
        for _ in 0..checks {
            root = root * 3 % 929;
            let mut next = vec![0; generator.len() + 1];
            for (i, &c) in generator.iter().enumerate() {
                next[i] = (next[i] + c) % 929;
                next[i + 1] = (next[i + 1] + 929 - c * root % 929) % 929;
            }
            generator = next;
        }
        let mut remainder = vec![0usize; checks];
        for &d in &code {
            let factor = (d + remainder[0]) % 929;
            remainder.rotate_left(1);
            remainder[checks - 1] = 0;
            for (r, &g) in remainder.iter_mut().zip(&generator[1..]) {
                *r = (*r + 929 - factor * g % 929) % 929;
            }
        }
        code.extend(remainder.iter().map(|&r| (929 - r) % 929));
        code
    }

    /// Dark and light run widths, in modules, of every row.
    fn rows(full: bool) -> Vec<Vec<usize>> {
        let code = codewords();
        (0..ROWS)
            .map(|row| {
                let cluster = row % 3;
                let base = row / 3 * 30;
                let upper = (ROWS - 1) / 3;
                let lower = LEVEL * 3 + (ROWS - 1) % 3;
                let (left, right) = match cluster {
                    0 => (base + upper, base + COLUMNS - 1),
                    1 => (base + lower, base + upper),
                    _ => (base + COLUMNS - 1, base + lower),
                };
                let mut values = vec![left];
                values.extend_from_slice(&code[row * COLUMNS..(row + 1) * COLUMNS]);
                if full {
                    values.push(right);
                }
                let mut runs = vec![8, 1, 1, 1, 1, 1, 1, 3];
                for value in values {
                    let pattern = CODES[cluster][value];
                    let mut previous = true;
                    let mut width = 0;
                    for bit in (0..17).rev().map(|shift| (pattern >> shift) & 1 == 1) {
                        if bit == previous {
                            width += 1;
                        } else {
                            runs.push(width);
                            previous = bit;
                            width = 1;
                        }
                    }
                    runs.push(width);
                }
                runs.extend_from_slice(if full {
                    &[7, 1, 1, 3, 1, 1, 1, 2, 1]
                } else {
                    &[1]
                });
                runs
            })
            .collect()
    }

    /// Blurred rendering at `module` pixels per module and eight lines per row.
    fn render(full: bool, module: f32, blur: f32) -> (Vec<u8>, usize, usize) {
        let quiet = 10. * module;
        let rows = rows(full);
        let modules: usize = rows[0].iter().sum();
        let width = crate::numeric::f32_usize(
            (quiet * 2. + module * crate::numeric::usize_f32(modules)).ceil(),
        );
        let height = 8 * ROWS + 16;
        let mut pixels = vec![255u8; width * height];
        for (row, runs) in rows.iter().enumerate() {
            let mut edges = vec![quiet];
            for &run in runs {
                edges.push(edges.last().unwrap() + module * crate::numeric::usize_f32(run));
            }
            let line: Vec<u8> = (0..width)
                .map(|x| {
                    let center = crate::numeric::usize_f32(x) + 0.5;
                    let dark: f32 = edges
                        .chunks(2)
                        .filter(|pair| pair.len() == 2)
                        .map(|pair| phi((center - pair[0]) / blur) - phi((center - pair[1]) / blur))
                        .sum();
                    crate::numeric::f32_u8(255. - 200. * dark.clamp(0., 1.))
                })
                .collect();
            for y in 0..8 {
                let at = (8 + row * 8 + y) * width;
                pixels[at..at + width].copy_from_slice(&line);
            }
        }
        (pixels, width, height)
    }

    #[test]
    fn the_test_symbol_is_a_valid_codeword_set() {
        let mut code = codewords();
        assert_eq!(reed_prime::correct(&mut code, 2 << LEVEL, &[]), Some(0));
    }

    #[test]
    fn blurred_compact_and_full_symbols_are_matched_cell_by_cell() {
        for full in [false, true] {
            for module in [2.5, 3.8] {
                let (pixels, width, height) = render(full, module, module * 0.45);
                let reads = read(&pixels, width, height);
                assert_eq!(reads.len(), 1, "full {full} module {module}");
                assert_eq!(reads[0].text, TEXT);
            }
        }
    }

    #[test]
    fn a_mirrored_crop_reads_the_same_symbol() {
        let (mut pixels, width, height) = render(false, 3.8, 1.7);
        for line in pixels.chunks_mut(width) {
            line.reverse();
        }
        let reads = read(&pixels, width, height);
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, TEXT);
    }

    #[test]
    fn a_blank_crop_reads_nothing() {
        assert!(read(&vec![200; 300 * 60], 300, 60).is_empty());
    }

    #[test]
    fn an_edge_fit_ignores_a_stray_point() {
        let mut points: Vec<(usize, f32)> = (0..10)
            .map(|y| (y, 5. + 0.1 * crate::numeric::usize_f32(y)))
            .collect();
        points.push((4, 9.));
        let edge = Edge::fit(&points, 0.5).unwrap();
        assert!((edge.at(0) - 5.).abs() < 1e-3 && (edge.at(10) - 6.).abs() < 1e-3);
        assert!(Edge::fit(&points[..MIN_LINES - 1], 0.5).is_none());
    }
}

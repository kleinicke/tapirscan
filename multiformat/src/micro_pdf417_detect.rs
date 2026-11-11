//! Row-address-driven `MicroPDF417` discovery. Rows vote by physical position;
//! error correction assembles missing cells without inventing observed addresses.
use crate::{Detection, micro_pdf417_tables as t, numeric::usize_f32 as f, pdf417, reed_prime};

const MAX_OBSERVATIONS: usize = 8192;
const MAX_HITS: usize = 16384;
const MAX_LADDERS: usize = 64;

/// A row address pattern on one scan line, with or without a readable row.
#[derive(Clone)]
struct Hit {
    address: usize,
    x: f32,
    end: f32,
    y: f32,
    module: f32,
}

#[derive(Clone)]
struct Row {
    variant: usize,
    index: usize,
    x: f32,
    y: f32,
    module: f32,
    words: [usize; 4],
}
/// One symbol size and row that a left row address can belong to.
#[derive(Clone, Copy)]
struct Candidate {
    variant: usize,
    row: usize,
    cols: usize,
    cluster: usize,
    centre: usize,
    right: usize,
}
struct Tables {
    /// Ten-module pattern to left or right row address plus one.
    side: [u8; 1024],
    side_widths: [[u8; 6]; 52],
    centre_widths: [[u8; 6]; 52],
    candidates: Vec<Vec<Candidate>>,
}
fn widths(pattern: usize) -> [u8; 6] {
    let mut widths = [0; 6];
    let mut index = 0;
    let mut prior = true;
    for bit in (0..10).rev() {
        let dark = pattern & (1 << bit) != 0;
        if dark != prior {
            index += 1;
            prior = dark;
        }
        widths[index] += 1;
    }
    widths
}
fn tables() -> &'static Tables {
    static TABLES: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    TABLES.get_or_init(|| {
        let mut side = [0; 1024];
        for (address, &pattern) in (1..).zip(&t::SIDE) {
            side[pattern] = address;
        }
        let candidates = (0..52)
            .map(|address| {
                (0..34)
                    .filter_map(|variant| {
                        let row = (address + 53 - t::RAP_START[variant]) % 52;
                        (row < t::VARIANTS[variant + 34]).then(|| Candidate {
                            variant,
                            row,
                            cols: t::VARIANTS[variant],
                            cluster: (t::RAP_START[variant + 102] / 3 + row) % 3,
                            centre: (t::RAP_START[variant + 34] + 51 + row) % 52,
                            right: (t::RAP_START[variant + 68] + 51 + row) % 52,
                        })
                    })
                    .collect()
            })
            .collect();
        Tables {
            side,
            side_widths: t::SIDE.map(widths),
            centre_widths: t::CENTRE.map(widths),
            candidates,
        }
    })
}
/// Module pattern of alternating runs that start with a bar, with the pitch.
#[inline]
fn pattern_gain(runs: &[usize], modules: usize, gain: f32) -> Option<(usize, f32)> {
    let pitch = f(runs.iter().sum()) / f(modules);
    if pitch < 0.8 {
        return None;
    }
    let scale = 1. / pitch;
    let mut value = 0;
    let mut total = 0;
    for (i, &run) in runs.iter().enumerate() {
        let bar = i % 2 == 0;
        let adjusted = f(run) * scale - if bar { gain } else { -gain };
        // Truncation after the half-module offset rounds every accepted width.
        let width = crate::numeric::f32_usize(adjusted + 0.5);
        if width == 0 || width > 6 {
            return None;
        }
        value = (value << width) | if bar { (1 << width) - 1 } else { 0 };
        total += width;
    }
    (total == modules).then_some((value, pitch))
}
fn address_matches(runs: &[usize], widths: [u8; 6]) -> Option<f32> {
    let pitch = f(runs.iter().sum()) / 10.;
    if pitch < 0.8 {
        return None;
    }
    let scale = 1. / pitch;
    for gain in [0., -0.3, 0.3, -0.6, 0.6] {
        let mut sum = 0.;
        let mut fits = true;
        for i in 0..6 {
            let error =
                (f(runs[i]) * scale - f32::from(widths[i]) - if i % 2 == 0 { gain } else { -gain })
                    .abs();
            fits &= error < 0.9;
            sum += error;
        }
        if fits && sum < 1.8 {
            return Some(pitch);
        }
    }
    None
}
fn codeword(runs: &[usize]) -> Option<(usize, usize)> {
    pattern_gain(runs, 17, 0.)
        .and_then(|(bits, _)| u32::try_from(bits).ok())
        .and_then(pdf417::lookup)
        .or_else(|| {
            let widths: [f32; 8] = std::array::from_fn(|i| f(runs[i]));
            pdf417::symbol(&widths)
        })
}
/// Whether a codeword of `span` pixels has its pitch within 30% of the module
/// of a row address of `total` pixels. Integers settle every ratio that is
/// not next to a limit.
#[inline]
fn codeword_pitch_fits(span: usize, total: usize) -> bool {
    if span * 100 < total * 118 || span * 100 > total * 222 {
        return false;
    }
    if span * 100 > total * 120 && span * 100 < total * 220 {
        return true;
    }
    ((f(span) / 17.) / (f(total) / 10.) - 1.).abs() <= 0.3
}
/// Row address at `runs[at..at + 6]` with its module, next to a codeword of
/// the same pitch. The flag tells whether a row may start here. Bar growth is
/// only compensated for a left address after a quiet zone and for a right
/// address before its stop bar and quiet zone.
fn side(runs: &[usize], edges: &[usize], at: usize) -> Option<(usize, f32, bool)> {
    let window = &runs[at..at + 6];
    // `edges` are the running sums of `runs`.
    let total = edges[at + 6] - edges[at];
    // Every side pattern starts with a bar of two to four of its ten modules.
    if window[0] * 100 < total * 8 || window[0] * 100 > total * 52 {
        return None;
    }
    // A module below 0.8 pixels.
    if total < 8 {
        return None;
    }
    let module = f(total) / 10.;
    let codeword = |from: usize| codeword_pitch_fits(edges[from + 8] - edges[from], total);
    let quiet = |run: usize| f(run) >= module * 1.5;
    let starts = at + 20 < runs.len() && codeword(at + 6);
    let ends = at >= 8
        && codeword(at - 8)
        && runs.get(at + 7).is_some_and(|&run| quiet(run))
        && (f(runs[at + 6]) / module - 1.).abs() <= 0.7;
    if !starts && !ends {
        return None;
    }
    let before = at > 0 && quiet(runs[at - 1]);
    let gains: &[f32] = if (starts && before) || ends {
        &[0., -0.3, 0.3, -0.6, 0.6]
    } else {
        &[0.]
    };
    let scale = 1. / module;
    let scaled: [f32; 6] = std::array::from_fn(|i| f(window[i]) * scale);
    let side = &tables().side;
    for (tried, &gain) in gains.iter().enumerate() {
        let mut value = 0;
        let mut modules = 0;
        for (i, &run) in scaled.iter().enumerate() {
            let bar = i % 2 == 0;
            let adjusted = run - if bar { gain } else { -gain };
            // Truncation after the half-module offset rounds every accepted width.
            let width = crate::numeric::f32_usize(adjusted + 0.5);
            if width == 0 || width > 4 {
                modules = 0;
                break;
            }
            value = (value << width) | if bar { (1 << width) - 1 } else { 0 };
            modules += width;
        }
        if modules == 10 && side[value] != 0 {
            let row = starts && (tried == 0 || before);
            return Some((usize::from(side[value]) - 1, module, row));
        }
    }
    None
}
/// Rows and row addresses of one scan line. `runs[i]` starts at `edges[i]`;
/// bars are the runs whose index has the parity of `start`.
fn rows(
    runs: &[usize],
    edges: &[usize],
    start: usize,
    y: f32,
    out: &mut Vec<Row>,
    hits: &mut Vec<Hit>,
) {
    let tables = tables();
    for at in (start..runs.len().saturating_sub(7)).step_by(2) {
        let Some((address, module, starts)) = side(runs, edges, at) else {
            continue;
        };
        if hits.len() < MAX_HITS {
            hits.push(Hit {
                address,
                x: f(edges[at]),
                end: f(edges[at + 6]),
                y,
                module,
            });
        }
        if !starts {
            continue;
        }
        let fits = |pitch: f32| (pitch / module - 1.).abs() <= 0.3;
        let pitch = |from: usize, runs_count: usize, modules: f32| {
            fits(f(runs[from..from + runs_count].iter().sum()) / modules)
        };
        // Sizes with the same column count share every position: the cheap
        // pitch tests of a count run once, and they run before any codeword
        // is decoded.
        let mut shapes = [None; 5];
        let mut decoded = [None; 40];
        for candidate in &tables.candidates[address] {
            let cols = candidate.cols;
            let centre = (cols >= 3).then(|| at + 6 + 8 * (cols - 2));
            let right = at + 6 + 8 * cols + usize::from(cols >= 3) * 6;
            if right + 7 > runs.len() {
                continue;
            }
            let shape = *shapes[cols].get_or_insert_with(|| {
                (f(runs[right + 6]) / module - 1.).abs() <= 0.7
                    && pitch(right, 6, 10.)
                    && centre.is_none_or(|centre| pitch(centre, 6, 10.))
                    && (0..cols).all(|col| {
                        let pos = at
                            + 6
                            + 8 * col
                            + usize::from(centre.is_some_and(|c| c <= at + 6 + 8 * col)) * 6;
                        pitch(pos, 8, 17.)
                    })
            });
            if !shape
                || address_matches(&runs[right..right + 6], tables.side_widths[candidate.right])
                    .is_none()
                || centre.is_some_and(|centre| {
                    address_matches(
                        &runs[centre..centre + 6],
                        tables.centre_widths[candidate.centre],
                    )
                    .is_none()
                })
            {
                continue;
            }
            let mut words = [0; 4];
            let complete = (0..cols).all(|col| {
                let pos = at
                    + 6
                    + 8 * col
                    + usize::from(centre.is_some_and(|c| c <= at + 6 + 8 * col)) * 6;
                let word = decoded[pos - at].get_or_insert_with(|| codeword(&runs[pos..pos + 8]));
                word.is_some_and(|(value, cluster)| {
                    words[col] = value;
                    cluster == candidate.cluster
                })
            });
            if !complete {
                continue;
            }
            out.push(Row {
                variant: candidate.variant,
                index: candidate.row,
                x: f(edges[at]),
                y,
                module,
                words,
            });
            if out.len() >= MAX_OBSERVATIONS {
                return;
            }
        }
    }
}
/// The module of a symbol tilted away from the camera about its rows: linear
/// in the image `y`, and the row pitch follows its square.
#[derive(Clone, Copy)]
struct Receding {
    y: f32,
    module: f32,
    slope: f32,
}
impl Receding {
    fn module(&self, y: f32) -> f32 {
        self.module + self.slope * (y - self.y)
    }
    /// A coordinate in which the rows are evenly spaced, equal to `y` around
    /// the centre.
    fn level(&self, y: f32) -> f32 {
        self.y + self.module / self.slope * (1. - self.module / self.module(y))
    }
    /// Image `y` and module at a level; `None` beyond the vanishing line.
    fn at(&self, level: f32) -> Option<[f32; 2]> {
        let shrink = 1. - self.slope * (level - self.y) / self.module;
        (shrink > 0.2).then(|| {
            let module = self.module / shrink;
            [self.y + (module - self.module) / self.slope, module]
        })
    }
}
fn assemble(
    rows: &[Row],
    transform: impl Fn(f32, f32) -> [f32; 2],
    regions: &mut crate::regions::Regions,
    limited: &mut bool,
) -> Vec<Detection> {
    let mut result = Vec::new();
    let mut used = vec![false; rows.len()];
    let mut attempts = 0;
    for at in 0..rows.len() {
        if used[at] {
            continue;
        }
        if attempts >= 256 {
            *limited = true;
            break;
        }
        attempts += 1;
        // Rows of one module width first; a symbol that recedes from the camera
        // only when those do not read.
        let (mut region, mut read) = symbol(rows, at, false, &transform, &mut used);
        if read.is_none() {
            let receding = symbol(rows, at, true, &transform, &mut used);
            if receding.1.is_some() {
                (region, read) = receding;
            }
        }
        if let Some((polygon, support)) = region {
            regions.add("MicroPDF417", polygon, 0.9, support);
        }
        result.extend(read);
    }
    result
}
/// The outline and row count of the symbol of the row `at` and the rows below
/// that agree with it, and its read. `receding` follows a module that changes
/// from row to row instead of the first row's.
fn symbol(
    rows: &[Row],
    at: usize,
    receding: bool,
    transform: &impl Fn(f32, f32) -> [f32; 2],
    used: &mut [bool],
) -> (Option<Outline>, Option<Detection>) {
    let mut outline = None;
    let read = outlined(rows, at, receding, transform, used, &mut outline);
    (outline, read)
}
/// A symbol's corners and the number of its rows that were seen.
type Outline = ([[f32; 2]; 4], usize);
#[expect(
    clippy::too_many_lines,
    reason = "Row voting, geometric agreement and correction share the same bounded assembly transaction."
)]
fn outlined(
    rows: &[Row],
    at: usize,
    receding: bool,
    transform: &impl Fn(f32, f32) -> [f32; 2],
    used: &mut [bool],
    outline: &mut Option<Outline>,
) -> Option<Detection> {
    let first = &rows[at];
    let variant = first.variant;
    let cols = t::VARIANTS[variant];
    let count = t::VARIANTS[variant + 34];
    let cells = f(21 + cols * 17 + usize::from(cols >= 3) * 10);
    let width = cells * first.module;
    let mut group = Vec::new();
    let mut near = first;
    for (i, row) in rows.iter().enumerate().skip(at) {
        if row.variant == variant
            && (row.module / near.module - 1.).abs() < 0.15
            && (row.x - near.x).abs() < cells * near.module * 0.3
            && (row.y - first.y).abs() < first.module * f(count) * 6.
        {
            group.push((i, row));
            if receding {
                near = row;
            }
        }
    }
    // Rows of one module width were tried before.
    if receding && (group.last()?.1.module / first.module - 1.).abs() < 0.1 {
        return None;
    }
    let unit = |row: &Row| if receding { row.module } else { first.module };
    // A repeated logical row after a gap is a different physical symbol.
    // Keep its first contiguous band, never average vertically stacked copies.
    let mut band_end = vec![None; count];
    let mut closed = vec![false; count];
    for &(_, row) in &group {
        if closed[row.index] {
            continue;
        }
        if band_end[row.index].is_some_and(|last| row.y - last > unit(row) * 2.) {
            closed[row.index] = true;
        } else {
            band_end[row.index] = Some(row.y);
        }
    }
    group.retain(|(_, row)| band_end[row.index].is_some_and(|last| row.y <= last));
    let center_y = group.iter().map(|(_, r)| r.y).sum::<f32>() / f(group.len());
    let center_x = group.iter().map(|(_, r)| r.x).sum::<f32>() / f(group.len());
    let variance = group
        .iter()
        .map(|(_, r)| (r.y - center_y).powi(2))
        .sum::<f32>();
    let depth = if receding {
        let module = group.iter().map(|(_, r)| r.module).sum::<f32>() / f(group.len());
        let slope = group
            .iter()
            .map(|(_, r)| (r.y - center_y) * (r.module - module))
            .sum::<f32>()
            / variance.max(1.);
        let depth = Receding {
            y: center_y,
            module,
            slope,
        };
        let ends = [group[0].1.y, group[group.len() - 1].1.y].map(|y| depth.module(y));
        if (ends[1] / ends[0] - 1.).abs() < 0.15 || ends.iter().any(|&end| end < module * 0.4) {
            return None;
        }
        Some(depth)
    } else {
        None
    };
    let level = |y: f32| depth.map_or(y, |depth| depth.level(y));
    let module = depth.map_or(first.module, |depth| depth.module);
    let mut samples: Vec<Vec<&Row>> = vec![Vec::new(); count];
    for &(_, row) in &group {
        samples[row.index].push(row);
    }
    let points: Vec<_> = samples
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| {
            [
                f(i) + 0.5,
                s.iter().map(|r| level(r.y)).sum::<f32>() / f(s.len()),
            ]
        })
        .collect();
    if points.len() < 3 {
        return None;
    }
    let n = f(points.len());
    let mx = points.iter().map(|p| p[0]).sum::<f32>() / n;
    let my = points.iter().map(|p| p[1]).sum::<f32>() / n;
    let den = points.iter().map(|p| (p[0] - mx).powi(2)).sum::<f32>();
    let pitch = points
        .iter()
        .map(|p| (p[0] - mx) * (p[1] - my))
        .sum::<f32>()
        / den;
    if !(0.8..=6.).contains(&(pitch / module)) {
        return None;
    }
    let top = my - pitch * mx;
    if points
        .iter()
        .any(|p| (p[1] - top - pitch * p[0]).abs() > pitch * 0.7)
    {
        return None;
    }
    let shear = group
        .iter()
        .map(|(_, r)| (r.y - center_y) * (r.x - center_x))
        .sum::<f32>()
        / variance.max(1.);
    if group
        .iter()
        .any(|(_, r)| (r.x - center_x - shear * (r.y - center_y)).abs() > unit(r) * 2.)
    {
        return None;
    }
    let polygon = if let Some(depth) = depth {
        let [bottom, bottom_module] = depth.at(top + pitch * f(count))?;
        let [top, top_module] = depth.at(top)?;
        let left = |y: f32| center_x + shear * (y - center_y);
        [
            [left(top), top],
            [left(top) + cells * top_module, top],
            [left(bottom) + cells * bottom_module, bottom],
            [left(bottom), bottom],
        ]
    } else {
        let left = center_x + shear * (top - center_y);
        let bottom_left = left + shear * pitch * f(count);
        [
            [left, top],
            [left + width, top],
            [bottom_left + width, top + pitch * f(count)],
            [bottom_left, top + pitch * f(count)],
        ]
    }
    .map(|[x, y]| transform(x, y));
    *outline = Some((polygon, points.len()));
    // A receding symbol keeps its rows for other symbols until it reads.
    if !receding {
        for &(i, _) in &group {
            used[i] = true;
        }
    }
    let mut words = vec![0; cols * count];
    let mut erasures = Vec::new();
    for (row, observations) in samples.iter().enumerate() {
        for col in 0..cols {
            let mut votes = std::collections::BTreeMap::new();
            for observation in observations {
                *votes.entry(observation.words[col]).or_insert(0usize) += 1;
            }
            if let Some((&word, _)) = votes.iter().max_by_key(|(_, votes)| *votes) {
                words[row * cols + col] = word;
            } else {
                erasures.push(row * cols + col);
            }
        }
    }
    let ecc = t::VARIANTS[variant + 68];
    if erasures.len() + 2 > ecc {
        return None;
    }
    let corrected = reed_prime::correct(&mut words, ecc, &erasures)?;
    let payload = pdf417::micro_payload(&words[..words.len() - ecc])?;
    for (i, _) in group {
        used[i] = true;
    }
    Some(Detection {
        format: "MicroPDF417".into(),
        text: payload.text,
        bytes: Some(payload.bytes),
        polygon,
        support: points.len(),
        error: f(corrected),
        structured_append: payload.structured_append,
        reader_initialization: payload.reader_initialization,
        gs1: false,
        assembled_rows: true,
        addon: None,
        codabar_start_stop: None,
    })
}
/// The same row address on neighbouring scan lines.
struct Stack {
    address: usize,
    low: f32,
    high: f32,
    last: f32,
    x: f32,
    end: f32,
    module: f32,
    count: f32,
}
impl Stack {
    fn start(&self) -> [f32; 2] {
        [self.x / self.count, f32::midpoint(self.low, self.high)]
    }
    fn stop(&self) -> [f32; 2] {
        [self.end / self.count, f32::midpoint(self.low, self.high)]
    }
    fn module(&self) -> f32 {
        self.module / self.count
    }
}
/// Least-squares line through `(row, point)` pairs: offset and step per row.
fn row_line(points: &[(f32, [f32; 2])]) -> Option<[[f32; 2]; 2]> {
    let n = f(points.len());
    let mean = points.iter().map(|p| p.0).sum::<f32>() / n;
    let spread = points.iter().map(|p| (p.0 - mean).powi(2)).sum::<f32>();
    if spread < 0.5 {
        return None;
    }
    Some([0, 1].map(|axis| {
        let centre = points.iter().map(|p| p.1[axis]).sum::<f32>() / n;
        let slope = points
            .iter()
            .map(|p| (p.0 - mean) * (p.1[axis] - centre))
            .sum::<f32>()
            / spread;
        [centre - slope * mean, slope]
    }))
}
/// Line of row positions: offset and step per row for each axis.
type RowLine = [[f32; 2]; 2];
fn along(line: &RowLine, row: f32) -> [f32; 2] {
    [line[0][0] + line[0][1] * row, line[1][0] + line[1][1] * row]
}
/// Modules from the left row address to the right one.
fn span(cols: usize) -> f32 {
    f(10 + 17 * cols + usize::from(cols >= 3) * 10)
}
/// Stacks of the hits, and the stacks of every address ordered by their first line.
fn stack_hits(hits: &mut [Hit], gap: f32) -> (Vec<Stack>, Vec<Vec<usize>>) {
    hits.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut stacks: Vec<Stack> = Vec::new();
    let mut by_address = vec![Vec::new(); 52];
    for hit in hits.iter() {
        let open = by_address[hit.address]
            .iter()
            .rev()
            .take(8)
            .copied()
            .find(|&i: &usize| {
                let stack = &stacks[i];
                hit.y - stack.high <= gap
                    && (hit.x - stack.last).abs() <= hit.module * 1.5
                    && (hit.module / stack.module() - 1.).abs() < 0.3
            });
        if let Some(i) = open {
            let stack = &mut stacks[i];
            stack.high = hit.y;
            stack.last = hit.x;
            stack.x += hit.x;
            stack.end += hit.end;
            stack.module += hit.module;
            stack.count += 1.;
        } else {
            by_address[hit.address].push(stacks.len());
            stacks.push(Stack {
                address: hit.address,
                low: hit.y,
                high: hit.y,
                last: hit.x,
                x: hit.x,
                end: hit.end,
                module: hit.module,
                count: 1.,
            });
        }
    }
    (stacks, by_address)
}
/// Runs of at least three stacks with consecutive addresses below each other.
fn ladders(stacks: &[Stack], by_address: &[Vec<usize>], gap: f32) -> Vec<Vec<usize>> {
    let mut next = vec![None; stacks.len()];
    let mut head = vec![true; stacks.len()];
    for (i, stack) in stacks.iter().enumerate() {
        let module = stack.module();
        let bucket = &by_address[(stack.address + 1) % 52];
        let from = bucket.partition_point(|&j| stacks[j].low <= stack.low);
        let below = bucket[from..]
            .iter()
            .copied()
            .take_while(|&j| stacks[j].low <= stack.high + gap + module)
            .find(|&j| {
                let other = &stacks[j];
                (other.start()[0] - stack.start()[0]).abs() <= module * 3.
                    && (other.module() / module - 1.).abs() < 0.35
            });
        if let Some(j) = below {
            next[i] = Some(j);
            head[j] = false;
        }
    }
    (0..stacks.len())
        .filter(|&i| head[i] && next[i].is_some())
        .map(|first| {
            std::iter::successors(Some(first), |&i| next[i])
                .take(52)
                .collect::<Vec<_>>()
        })
        .filter(|ladder| ladder.len() >= 3)
        .collect()
}
/// Lines through the left and the right end of the rows of a symbol of the
/// size of `candidate` whose left row addresses are the stacks of `ladder`.
fn row_ends(
    ladder: &[usize],
    stacks: &[Stack],
    by_address: &[Vec<usize>],
    candidate: &Candidate,
) -> Option<[RowLine; 2]> {
    let count = t::VARIANTS[candidate.variant + 34];
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (k, &i) in ladder.iter().enumerate() {
        let row = candidate.row + k;
        if row >= count {
            break;
        }
        let module = stacks[i].module();
        let from = stacks[i].start();
        left.push((f(row), from));
        let reach = span(candidate.cols) * module;
        let partner = by_address[(candidate.right + k) % 52]
            .iter()
            .map(|&j| &stacks[j])
            .filter(|other| {
                let to = other.start();
                (0.86..1.16).contains(&((to[0] - from[0]) / reach))
                    && (to[1] - from[1]).abs() < reach * 0.35
                    && (0.6..1.6).contains(&(other.module() / module))
            })
            .min_by(|a, b| {
                (a.start()[0] - from[0] - reach)
                    .abs()
                    .total_cmp(&(b.start()[0] - from[0] - reach).abs())
            });
        if let Some(other) = partner {
            right.push((f(row), other.stop()));
        }
    }
    if left.len() < 3 || right.len() < 2 {
        return None;
    }
    let lines = [row_line(&left)?, row_line(&right)?];
    // Both ends advance by about the same step from row to row.
    let steps = lines.map(|line| [line[0][1], line[1][1]]);
    let lengths = steps.map(|step| step[0].hypot(step[1]));
    (lengths[0] >= 1.
        && (0.6..1.6).contains(&(lengths[1] / lengths[0]))
        && steps[0][0] * steps[1][0] + steps[0][1] * steps[1][1] >= 0.8 * lengths[0] * lengths[1])
        .then_some(lines)
}
/// Buffers for reading rows along their own lines.
#[derive(Default)]
struct Sampler {
    line: Vec<bool>,
    edges: Vec<usize>,
    runs: Vec<usize>,
    ignored: Vec<Hit>,
}
impl Sampler {
    /// Rows of a symbol of the size of `candidate` between `lines`, in a frame
    /// with unit modules and rows three modules high. `expected` is the module
    /// of the row addresses.
    fn rows(
        &mut self,
        lines: &[RowLine; 2],
        candidate: &Candidate,
        expected: f32,
        bar: &dyn Fn(f32, f32) -> bool,
    ) -> Vec<Row> {
        let count = t::VARIANTS[candidate.variant + 34];
        let mut found = Vec::new();
        // The rows of the ladder come first: a place where four of them
        // give no row is not a symbol of this size.
        let order = (candidate.row..count).chain(0..candidate.row);
        for (tried, row) in order.enumerate() {
            if tried == 4 && found.is_empty() {
                break;
            }
            for shift in [-0.25, 0., 0.25] {
                let position = f(row) + shift;
                let from = along(&lines[0], position);
                let to = along(&lines[1], position);
                let length = (to[0] - from[0]).hypot(to[1] - from[1]);
                let module = length / (span(candidate.cols) + 10.);
                // Fitted lines that run apart leave the module of the ladder.
                if module < 0.8 || !(0.5..2.).contains(&(module / expected)) {
                    continue;
                }
                let direction = [(to[0] - from[0]) / length, (to[1] - from[1]) / length];
                // Quiet zones on both sides and the stop bar belong to the line.
                let samples = crate::numeric::f32_usize(length + module * 9.) + 1;
                self.line.clear();
                self.line.extend((0..samples).map(|i| {
                    let step = f(i) - module * 4.;
                    bar(from[0] + direction[0] * step, from[1] + direction[1] * step)
                }));
                crate::transition_offsets_into(&self.line, &mut self.edges);
                self.runs.clear();
                self.runs
                    .extend(self.edges.windows(2).map(|pair| pair[1] - pair[0]));
                let before = found.len();
                self.ignored.clear();
                rows(
                    &self.runs,
                    &self.edges,
                    usize::from(!self.line[0]),
                    0.,
                    &mut found,
                    &mut self.ignored,
                );
                let mut kept = before;
                for i in before..found.len() {
                    if found[i].variant == candidate.variant && found[i].index == row {
                        found[i].x = 0.;
                        found[i].y = (position + 0.5) * 3.;
                        found[i].module = 1.;
                        found.swap(kept, i);
                        kept += 1;
                    }
                }
                found.truncate(kept);
            }
        }
        found
    }
}
/// Symbols whose rows no scan line follows from end to end. Row addresses of
/// consecutive rows stack at the left edge; the right addresses of the same
/// rows give each row its own line, which is then read like a scan line.
/// `gap` is the largest distance of lines that see the same row, `bar` samples
/// the scanned frame and `known` tells whether a place already has a read.
fn rescue(
    hits: &mut [Hit],
    gap: f32,
    bar: &dyn Fn(f32, f32) -> bool,
    map: &dyn Fn(f32, f32) -> [f32; 2],
    known: &dyn Fn(&[[f32; 2]; 4]) -> bool,
    regions: &mut crate::regions::Regions,
    limited: &mut bool,
) -> Vec<Detection> {
    let mut result = Vec::new();
    if hits.len() < 5 {
        return result;
    }
    let (stacks, by_address) = stack_hits(hits, gap);
    let mut sampler = Sampler::default();
    let mut attempts = 0;
    for (examined, ladder) in ladders(&stacks, &by_address, gap).iter().enumerate() {
        if examined >= MAX_LADDERS {
            *limited = true;
            break;
        }
        for candidate in &tables().candidates[stacks[ladder[0]].address] {
            let Some(lines) = row_ends(ladder, &stacks, &by_address, candidate) else {
                continue;
            };
            // Row centres sit at whole rows, the symbol edges half a row outside.
            let last = f(t::VARIANTS[candidate.variant + 34]) - 0.5;
            let outline = [
                along(&lines[0], -0.5),
                along(&lines[1], -0.5),
                along(&lines[1], last),
                along(&lines[0], last),
            ]
            .map(|[x, y]| map(x, y));
            if known(&outline)
                || result
                    .iter()
                    .any(|read: &Detection| crate::regions::overlap(&outline, &read.polygon) > 0.5)
            {
                continue;
            }
            if attempts >= 8 {
                *limited = true;
                return result;
            }
            attempts += 1;
            let found = sampler.rows(&lines, candidate, stacks[ladder[0]].module(), bar);
            let width = span(candidate.cols) + 10.;
            let rectified = |x: f32, y: f32| {
                let from = along(&lines[0], y / 3. - 0.5);
                let to = along(&lines[1], y / 3. - 0.5);
                map(
                    from[0] + (to[0] - from[0]) * x / width,
                    from[1] + (to[1] - from[1]) * x / width,
                )
            };
            result.extend(assemble(&found, rectified, regions, limited));
        }
    }
    result
}
/// Tangents of 2.5°, 7.5°, … 42.5°, the bin borders of the first octant.
const BIN_BORDERS: [f32; 9] = [
    0.043_660_943,
    0.131_652_5,
    0.221_694_66,
    0.315_298_8,
    0.414_213_57,
    0.520_567_05,
    0.637_070_26,
    0.767_327,
    0.916_331_2,
];
/// The 5° bin of the undirected direction of `(dx, dy)`: bin `k` is centred
/// on `5k` degrees. Exact for the integer gradients of a 3×3 Sobel filter.
#[inline]
fn direction_bin(dx: f32, dy: f32) -> usize {
    // Folded to the first octant, the bin is the number of borders below.
    let (x, y) = (dx.abs(), dy.abs());
    let (long, short) = if y > x { (y, x) } else { (x, y) };
    let folded = BIN_BORDERS
        .iter()
        .filter(|&&border| short > long * border)
        .count();
    let quadrant = if y > x { 18 - folded } else { folded };
    if (dx < 0.) == (dy < 0.) || quadrant == 0 {
        quadrant
    } else {
        36 - quadrant
    }
}
/// Dominant non-cardinal bar normals. Diffuse texture never adds angle sweeps.
fn angles(gray: &[u8], w: usize, h: usize) -> Vec<f32> {
    let mut result = vec![0., 90., 180., 270.];
    if w < 3 || h < 3 {
        return result;
    }
    let mut bins = [0f32; 36];
    let stride = ((w * h) / 200_000).isqrt().max(1);
    for y in (1..h - 1).step_by(stride) {
        for x in (1..w - 1).step_by(stride) {
            let at = y * w + x;
            let sample = |offset: usize| f32::from(gray[offset]);
            let dx = sample(at - w + 1) + 2. * sample(at + 1) + sample(at + w + 1)
                - sample(at - w - 1)
                - 2. * sample(at - 1)
                - sample(at + w - 1);
            let dy = sample(at + w - 1) + 2. * sample(at + w) + sample(at + w + 1)
                - sample(at - w - 1)
                - 2. * sample(at - w)
                - sample(at - w + 1);
            let weight = dx * dx + dy * dy;
            if weight < 400. {
                continue;
            }
            bins[direction_bin(dx, dy)] += weight;
        }
    }
    let total = bins.iter().sum::<f32>();
    for _ in 0..2 {
        let Some((index, &weight)) = bins.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))
        else {
            break;
        };
        if weight < total * 0.08 || weight == 0. {
            break;
        }
        for offset in [35, 0, 1] {
            bins[(index + offset) % 36] = 0.;
        }
        if index % 18 == 0 {
            continue;
        }
        // The cardinal scans reach the inner part of the bins beside them.
        let near = matches!(index % 18, 1 | 17);
        let deltas: &[f32] = if near {
            &[-1.5, 1.5]
        } else {
            &[0., -2., 2., -4., 4.]
        };
        for &delta in deltas {
            let angle = f(index) * 5. + delta;
            result.extend([angle, angle + 180.]);
        }
    }
    result
}
/// Both scan directions and both polarities of the lines of one threshold image.
#[derive(Default)]
struct Scanner {
    /// Forward, forward inverse, reversed and reversed inverse rows.
    lists: [Vec<Row>; 4],
    hits: [Vec<Hit>; 4],
    runs: Vec<usize>,
    mirrored: Vec<usize>,
    mirrored_runs: Vec<usize>,
    previous: Vec<usize>,
    previous_dark: bool,
    repeated: [usize; 4],
    repeated_hits: [usize; 4],
    limited: bool,
}
impl Scanner {
    /// `edges` are the transition offsets of a line with both ends; `dark` is its
    /// first pixel. `ys` number the line in the forward and in the reversed scan.
    fn line(&mut self, edges: &[usize], dark: bool, ys: [f32; 2]) {
        if edges == self.previous.as_slice() && dark == self.previous_dark {
            for (k, list) in self.lists.iter_mut().enumerate() {
                let count = self.repeated[k];
                let from = list.len() - count;
                let copies = count.min(MAX_OBSERVATIONS - list.len());
                for at in from..from + copies {
                    let mut row = list[at].clone();
                    row.y = ys[k / 2];
                    list.push(row);
                }
                self.limited |= list.len() >= MAX_OBSERVATIONS;
                let hits = &mut self.hits[k];
                let count = self.repeated_hits[k];
                let from = hits.len() - count;
                for at in from..from + count.min(MAX_HITS - hits.len()) {
                    let mut hit = hits[at].clone();
                    hit.y = ys[k / 2];
                    hits.push(hit);
                }
            }
            return;
        }
        let length = edges.last().copied().unwrap_or(0);
        self.runs.clear();
        self.runs
            .extend(edges.windows(2).map(|pair| pair[1] - pair[0]));
        self.mirrored.clear();
        self.mirrored
            .extend(edges.iter().rev().map(|&edge| length - edge));
        self.mirrored_runs.clear();
        self.mirrored_runs.extend(self.runs.iter().rev());
        let mirrored_dark = dark ^ self.runs.len().is_multiple_of(2);
        for (k, list) in self.lists.iter_mut().enumerate() {
            let before = list.len();
            let hits = &mut self.hits[k];
            let hits_before = hits.len();
            if before < MAX_OBSERVATIONS {
                let (runs, edges, dark) = if k < 2 {
                    (&self.runs, edges, dark)
                } else {
                    (&self.mirrored_runs, self.mirrored.as_slice(), mirrored_dark)
                };
                // The inverse image has bars where this one has spaces.
                rows(
                    runs,
                    edges,
                    usize::from(dark == (k % 2 == 1)),
                    ys[k / 2],
                    list,
                    hits,
                );
            }
            self.repeated_hits[k] = hits.len() - hits_before;
            self.repeated[k] = list.len() - before;
            self.limited |= list.len() >= MAX_OBSERVATIONS;
        }
        self.previous.clear();
        self.previous.extend_from_slice(edges);
        self.previous_dark = dark;
    }
}
/// The sampled columns of a column scan: for each word of eight pixels the
/// bytes of its sampled columns, and for each column its list or `usize::MAX`.
struct Sampled {
    words: Vec<u64>,
    list: Vec<usize>,
}
impl Sampled {
    fn new(w: usize, step: usize) -> Self {
        let mut words = vec![0u64; w.div_ceil(8)];
        let mut list = vec![usize::MAX; w];
        for (index, x) in (0..w).step_by(step).enumerate() {
            words[x / 8] |= 1 << (8 * (x % 8));
            list[x] = index;
        }
        Self { words, list }
    }
}
/// Adds `y` to every sampled column whose pixel differs between two rows.
fn column_transitions(
    above: &[bool],
    row: &[bool],
    sampled: &Sampled,
    y: usize,
    transitions: &mut [Vec<usize>],
) {
    let mut push = |chunk: usize, mut pending: u64| {
        while pending != 0 {
            let x = chunk * 8 + pending.trailing_zeros() as usize / 8;
            transitions[sampled.list[x]].push(y);
            pending &= pending - 1;
        }
    };
    let (above_words, above_rest) = above.as_chunks::<8>();
    let (row_words, row_rest) = row.as_chunks::<8>();
    for (chunk, ((above, row), &mask)) in above_words
        .iter()
        .zip(row_words)
        .zip(&sampled.words)
        .enumerate()
    {
        let changed =
            u64::from_le_bytes(above.map(u8::from)) ^ u64::from_le_bytes(row.map(u8::from));
        if changed & mask != 0 {
            push(chunk, changed & mask);
        }
    }
    let mut changed = 0u64;
    for (k, (above, row)) in above_rest.iter().zip(row_rest).enumerate() {
        changed |= u64::from(above != row) << (8 * k);
    }
    if let Some(&mask) = sampled.words.get(row_words.len()) {
        push(row_words.len(), changed & mask);
    }
}
/// `value.round()` from operations that every target has as an instruction;
/// the fraction left by the truncation is exact.
#[inline]
fn round(value: f32) -> f32 {
    let whole = value.trunc();
    if (value - whole).abs() >= 0.5 {
        whole + 1f32.copysign(value)
    } else {
        whole
    }
}
/// The columns for which `inside` holds, when it only changes once along them.
fn monotone_range(columns: &[[f32; 2]], inside: impl Fn(&[f32; 2]) -> bool) -> [usize; 2] {
    let (Some(first), Some(last)) = (columns.first(), columns.last()) else {
        return [0, 0];
    };
    match (inside(first), inside(last)) {
        (true, true) => [0, columns.len()],
        (true, false) => [0, columns.partition_point(inside)],
        (false, true) => [
            columns.partition_point(|column| !inside(column)),
            columns.len(),
        ],
        (false, false) => [0, 0],
    }
}
/// Threshold pixels along one oblique line; pixels outside the image are light.
/// Both coordinates of `columns` are monotone.
fn oblique_line(
    image: &[bool],
    [w, h]: [usize; 2],
    columns: &[[f32; 2]],
    offset: [f32; 2],
    line: &mut Vec<bool>,
) {
    let xx = |column: &[f32; 2]| round(column[0] - offset[0]);
    let yy = |column: &[f32; 2]| round(column[1] + offset[1]);
    // Each image border is crossed once, so the pixels inside are one range.
    let ranges = [
        monotone_range(columns, |column| xx(column) >= 0.),
        monotone_range(columns, |column| xx(column) < f(w)),
        monotone_range(columns, |column| yy(column) >= 0.),
        monotone_range(columns, |column| yy(column) < f(h)),
    ];
    let inside = ranges.iter().fold([0, columns.len()], |inside, range| {
        [inside[0].max(range[0]), inside[1].min(range[1])]
    });
    line.clear();
    line.resize(columns.len(), false);
    if inside[0] >= inside[1] {
        return;
    }
    for (pixel, column) in line[inside[0]..inside[1]]
        .iter_mut()
        .zip(&columns[inside[0]..inside[1]])
    {
        *pixel = image
            [crate::numeric::f32_usize(yy(column)) * w + crate::numeric::f32_usize(xx(column))];
    }
}
/// Sine and cosine of a turn with the lower corner and the extent of the
/// image in the turned frame.
fn oblique_frame(turn: f32, w: usize, h: usize) -> (f32, f32, [f32; 2], [usize; 2]) {
    let (sin, cos) = turn.to_radians().sin_cos();
    let corners = [
        [0., 0.],
        [f(w) - 1., 0.],
        [f(w) - 1., f(h) - 1.],
        [0., f(h) - 1.],
    ];
    let projected = corners.map(|[x, y]| [x * cos + y * sin, -x * sin + y * cos]);
    let low = [0, 1].map(|i| projected.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min));
    let high = [0, 1].map(|i| {
        projected
            .iter()
            .map(|p| p[i])
            .fold(f32::NEG_INFINITY, f32::max)
    });
    let extent = [0, 1].map(|i| crate::numeric::f32_usize((high[i] - low[i]).round()) + 1);
    (sin, cos, low, extent)
}
/// Every `step`th column, read in one row-major pass over the image.
fn scan_columns(image: &[bool], w: usize, h: usize, step: usize) -> Scanner {
    let mut scanner = Scanner::default();
    let columns: Vec<usize> = (0..w).step_by(step).collect();
    let mut transitions = vec![vec![0]; columns.len()];
    let sampled = Sampled::new(w, step);
    for y in 1..h {
        let (above, row) = image[(y - 1) * w..(y + 1) * w].split_at(w);
        column_transitions(above, row, &sampled, y, &mut transitions);
    }
    for (&x, mut transitions) in columns.iter().zip(transitions) {
        transitions.push(h);
        scanner.line(&transitions, image[x], [f(w - 1 - x), f(x)]);
    }
    scanner
}
/// Reads in scan order without the repeats of one symbol.
fn unique(mut found: Vec<(usize, Detection)>) -> Vec<Detection> {
    found.sort_by_key(|&(key, _)| key);
    let mut results: Vec<Detection> = Vec::new();
    for (_, read) in found {
        if !results.iter().any(|r| {
            r.text == read.text && crate::regions::overlap(&r.polygon, &read.polygon) > 0.6
        }) {
            results.push(read);
        }
    }
    results
}
/// Fewer row addresses than this in an oblique scan of the global threshold
/// mean that it lost the bars; the scan then repeats on the local threshold.
const BARE_HITS: usize = 16;
/// Scan cardinal directions and strong oblique bar directions with bounded observations.
pub fn detect(
    gray: &[u8],
    size: [usize; 2],
    images: &mut crate::binarization::Images<'_>,
    regions: &mut crate::regions::Regions,
) -> (Vec<Detection>, bool) {
    let [w, h] = size;
    if w == 0 || h == 0 {
        return (Vec::new(), false);
    }
    let directions = angles(gray, w, h);
    let mut found: Vec<(usize, Detection)> = Vec::new();
    let mut limited = false;
    let mut edges = Vec::new();
    // `forward` maps the forward scan of `extent` (line length, line count) to
    // the image; the reversed scan reads the same lines from the other end.
    let mut finish = |scanner: Scanner,
                      image: &[bool],
                      gap: usize,
                      base: usize,
                      order: usize,
                      extent: [usize; 2],
                      forward: &dyn Fn(f32, f32) -> [f32; 2]| {
        limited |= scanner.limited;
        let reversed = |x: f32, y: f32| forward(f(extent[0] - 1) - x, f(extent[1] - 1) - y);
        for (k, (mut list, mut hits)) in scanner.lists.into_iter().zip(scanner.hits).enumerate() {
            list.sort_by(|a, b| a.y.total_cmp(&b.y));
            let key = (base + 2 * (k % 2)) * 64 + order + 2 * (k / 2);
            let map: &dyn Fn(f32, f32) -> [f32; 2] = if k < 2 { forward } else { &reversed };
            let reads = assemble(&list, map, regions, &mut limited);
            found.extend(reads.into_iter().map(|read| (key, read)));
            // Bars are light in the inverse lists; outside the image is never a bar.
            let bar = |x: f32, y: f32| {
                let [x, y] = map(x, y).map(f32::round);
                x >= 0.
                    && y >= 0.
                    && x < f(w)
                    && y < f(h)
                    && image[crate::numeric::f32_usize(y) * w + crate::numeric::f32_usize(x)]
                        == (k % 2 == 0)
            };
            let known = |outline: &[[f32; 2]; 4]| {
                found
                    .iter()
                    .any(|(_, read)| crate::regions::overlap(outline, &read.polygon) > 0.5)
            };
            let reads = rescue(
                &mut hits,
                f(gap * 2) + 0.5,
                &bar,
                map,
                &known,
                regions,
                &mut limited,
            );
            found.extend(reads.into_iter().map(|read| (key, read)));
        }
    };
    // Directions in which the global threshold shows almost no row address.
    let mut bare = vec![false; directions.len()];
    for base in 0..2 {
        if images.is_duplicate(base) {
            continue;
        }
        let mut scanner = Scanner::default();
        let step = (h / 1024).max(2);
        let (image, offsets) = images.get_with_runs(base, step);
        for y in (0..h).step_by(step) {
            let row = &image[y * w..(y + 1) * w];
            offsets.copy_or_extract(y, row, &mut edges);
            scanner.line(&edges, row[0], [f(y), f(h - 1 - y)]);
        }
        finish(scanner, image, step, base, 0, [w, h], &|x, y| [x, y]);

        let step = (w / 1024).max(2);
        let scanner = scan_columns(image, w, h, step);
        finish(scanner, image, step, base, 1, [h, w], &|x, y| {
            [f(w - 1) - y, x]
        });

        let mut line = Vec::new();
        for (index, &turn) in directions.iter().enumerate().skip(4) {
            if turn >= 180. || (base == 1 && !bare[index]) {
                continue;
            }
            let (sin, cos, low, extent) = oblique_frame(turn, w, h);
            let columns: Vec<_> = (0..extent[0])
                .map(|x| [(f(x) + low[0]) * cos, (f(x) + low[0]) * sin])
                .collect();
            let mut scanner = Scanner::default();
            let step = (extent[1] / 1024).max(2);
            for y in (0..extent[1]).step_by(step) {
                let offset = [(f(y) + low[1]) * sin, (f(y) + low[1]) * cos];
                oblique_line(image, size, &columns, offset, &mut line);
                crate::transition_offsets_into(&line, &mut edges);
                scanner.line(&edges, line[0], [f(y), f(extent[1] - 1 - y)]);
            }
            bare[index] = scanner.hits.iter().map(Vec::len).sum::<usize>() < BARE_HITS;
            finish(scanner, image, step, base, index, extent, &|x, y| {
                [
                    (x + low[0]) * cos - (y + low[1]) * sin,
                    (x + low[0]) * sin + (y + low[1]) * cos,
                ]
            });
        }
    }
    (unique(found), limited)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_rounding_matches_the_standard_one() {
        let mut state = 0x9e37_79b9_u32;
        for step in 0..2_000_000_u32 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let value = if step % 4 == 0 {
                f32::from_bits(state)
            } else {
                f32::from_bits(0x3f00_0000 + (state >> 20)) * f((step % 251) as usize) - 60.
            };
            if value.is_nan() {
                continue;
            }
            assert_eq!(round(value).to_bits(), value.round().to_bits(), "{value}");
        }
        for value in [
            0.5,
            -0.5,
            1.5,
            -1.5,
            0.499_999_97,
            -0.499_999_97,
            8_388_609.,
            -0.,
            0.,
        ] {
            assert_eq!(
                round(value).to_bits(),
                f32::round(value).to_bits(),
                "{value}"
            );
        }
    }
    #[test]
    fn codeword_pitch_limits_match_the_float_ratio() {
        for total in 8..600 {
            for span in 0..1500 {
                let ratio = (f(span) / 17.) / (f(total) / 10.);
                assert_eq!(
                    codeword_pitch_fits(span, total),
                    (ratio - 1.).abs() <= 0.3,
                    "{span} {total}"
                );
            }
        }
    }
    #[test]
    fn direction_bins_match_the_exact_angle() {
        // Every gradient of a 3×3 Sobel filter on eight-bit pixels.
        for dy in -1020i16..=1020 {
            for dx in -1020i16..=1020 {
                let angle = f64::from(dy).atan2(f64::from(dx)).to_degrees();
                let bin = crate::numeric::f64_usize((angle.rem_euclid(180.) / 5.).round()) % 36;
                assert_eq!(direction_bin(dx.into(), dy.into()), bin, "{dx} {dy}");
            }
        }
    }
    #[test]
    fn oblique_lines_preserve_rotated_pixel_coordinates() {
        let [w, h] = [7, 5];
        let image: Vec<bool> = (0..w * h).map(|i| (i * 13 + i / w) % 7 < 3).collect();
        let mut line = Vec::new();
        for angle in [17f32, 43., 137., 223.] {
            let (sin, cos) = angle.to_radians().sin_cos();
            let low = [-3.25, -2.75];
            let columns: Vec<_> = (0..13)
                .map(|x| [(f(x) + low[0]) * cos, (f(x) + low[0]) * sin])
                .collect();
            for y in 0..11 {
                let expected: Vec<bool> = (0..13)
                    .map(|x| {
                        let xx = ((f(x) + low[0]) * cos - (f(y) + low[1]) * sin).round();
                        let yy = ((f(x) + low[0]) * sin + (f(y) + low[1]) * cos).round();
                        xx >= 0.
                            && yy >= 0.
                            && xx < f(w)
                            && yy < f(h)
                            && image
                                [crate::numeric::f32_usize(yy) * w + crate::numeric::f32_usize(xx)]
                    })
                    .collect();
                let offset = [(f(y) + low[1]) * sin, (f(y) + low[1]) * cos];
                oblique_line(&image, [w, h], &columns, offset, &mut line);
                assert_eq!(line, expected);
            }
        }
    }
    #[test]
    fn side_patterns_start_with_a_bar_of_two_to_four_modules() {
        for pattern in t::SIDE {
            let widths = widths(pattern);
            assert!((2..=4).contains(&widths[0]));
            assert!(widths.iter().all(|&width| (1..=4).contains(&width)));
        }
    }
    #[test]
    fn slanted_rows_are_read_along_their_row_addresses() {
        let patterns = [
            68708, 128_478, 102_290, 68708, 128_088, 100_732, 81384, 126_152, 81722, 116_080, 97830,
        ];
        let [w, h, left, top, module, height] = [170, 120, 24, 20, 3, 6];
        // 38 modules: left address, codeword, right address and stop bar.
        let dark = |x: usize, y: usize| {
            let Some(along) = x.checked_sub(left) else {
                return false;
            };
            // Rows drop by a row and a half over the width of the symbol.
            let Some(down) = (y * 100).checked_sub(top * 100 + along * 8) else {
                return false;
            };
            let [column, row] = [along / module, down / (height * 100)];
            if column >= 38 || row >= patterns.len() {
                return false;
            }
            let address = |start: usize| t::SIDE[(start + 51 + row) % 52];
            match column {
                0..10 => address(t::RAP_START[0]) >> (9 - column) & 1 == 1,
                10..27 => patterns[row] >> (26 - column) & 1 == 1,
                27..37 => address(t::RAP_START[68]) >> (36 - column) & 1 == 1,
                _ => true,
            }
        };
        let image: Vec<bool> = (0..w * h).map(|i| dark(i % w, i / w)).collect();
        let mut scanner = Scanner::default();
        let mut edges = Vec::new();
        for (y, line) in image.chunks_exact(w).enumerate() {
            crate::transition_offsets_into(line, &mut edges);
            scanner.line(&edges, line[0], [f(y), f(h - 1 - y)]);
        }
        let mut regions = crate::regions::Regions::default();
        let mut limited = false;
        let direct = assemble(&scanner.lists[0], |x, y| [x, y], &mut regions, &mut limited);
        assert!(direct.is_empty());
        let bar = |x: f32, y: f32| {
            x >= 0. && y >= 0. && dark(crate::numeric::f32_usize(x), crate::numeric::f32_usize(y))
        };
        let reads = rescue(
            &mut scanner.hits[0],
            2.5,
            &bar,
            &|x, y| [x, y],
            &|_| false,
            &mut regions,
            &mut limited,
        );
        assert_eq!(reads.len(), 1);
        assert_eq!(reads[0].text, "A");
        assert!(!limited);
    }
    #[test]
    fn stacked_equal_payloads_remain_two_physical_symbols() {
        let patterns = [
            68708, 128_478, 102_290, 68708, 128_088, 100_732, 81384, 126_152, 81722, 116_080, 97830,
        ];
        let mut observations = Vec::new();
        for top in [20., 98.] {
            for (index, &pattern) in patterns.iter().enumerate() {
                let (word, _) = pdf417::lookup(pattern).unwrap();
                for offset in 0..6 {
                    observations.push(Row {
                        variant: 0,
                        index,
                        x: 18.,
                        y: top + f(index) * 6. + f(offset),
                        module: 3.,
                        words: [word, 0, 0, 0],
                    });
                }
            }
        }
        let mut regions = crate::regions::Regions::default();
        let mut limited = false;
        let reads = assemble(&observations, |x, y| [x, y], &mut regions, &mut limited);
        assert_eq!(reads.len(), 2);
        assert!(!limited);
        for (read, top) in reads.iter().zip([20., 98.]) {
            assert_eq!(read.text, "A");
            assert!((read.polygon[0][1] - top).abs() < 1.);
        }
    }
    #[test]
    fn rows_that_widen_down_the_image_form_one_symbol() {
        let patterns = [
            68708, 128_478, 102_290, 68708, 128_088, 100_732, 81384, 126_152, 81722, 116_080, 97830,
        ];
        // The module grows from 3 to 5 pixels and the rows spread with it.
        let depth = Receding {
            y: 60.,
            module: 4.,
            slope: 0.03,
        };
        let mut observations = Vec::new();
        for (index, &pattern) in patterns.iter().enumerate() {
            let (word, _) = pdf417::lookup(pattern).unwrap();
            for offset in 0..6 {
                let [y, module] = depth.at(27.5 + f(index * 6 + offset)).unwrap();
                observations.push(Row {
                    variant: 0,
                    index,
                    x: 100. - module * 19.,
                    y,
                    module,
                    words: [word, 0, 0, 0],
                });
            }
        }
        let mut regions = crate::regions::Regions::default();
        let mut used = vec![false; observations.len()];
        let (_, level) = symbol(&observations, 0, false, &|x, y| [x, y], &mut used);
        assert!(level.is_none());
        let mut limited = false;
        let reads = assemble(&observations, |x, y| [x, y], &mut regions, &mut limited);
        assert_eq!(reads.len(), 1);
        assert!(!limited);
        assert_eq!(reads[0].text, "A");
        let [top, bottom] = [27., 93.].map(|level| depth.at(level).unwrap());
        let expected = [
            [100. - top[1] * 19., top[0]],
            [100. + top[1] * 19., top[0]],
            [100. + bottom[1] * 19., bottom[0]],
            [100. - bottom[1] * 19., bottom[0]],
        ];
        for (corner, expected) in reads[0].polygon.iter().zip(expected) {
            assert!(
                (corner[0] - expected[0]).abs() < 1. && (corner[1] - expected[1]).abs() < 1.,
                "{corner:?} {expected:?}"
            );
        }
    }
}

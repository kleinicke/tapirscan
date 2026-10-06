//! Symbol area grown from one decoded scanline along its bars.
//!
//! The decoded line is sampled as a profile; the line then steps outward on both sides along the
//! bars. Each new row must correlate with the previous one block by block (small end shifts follow
//! perspective and slightly bent labels), and a few dark probe bars are followed at half-pixel
//! resolution between rows so a one-pixel separator ends the area exactly. The grown polygon
//! decides which other reads lie on the same physical symbol.
use super::{distance, line, Evidence};
use crate::Quad;

const SAMPLES: usize = 256;
const BLOCKS: usize = 8;
const BLOCK: usize = SAMPLES / BLOCKS;
type Row = [f64; SAMPLES];

fn index_f64(index: usize) -> f64 {
    f64::from(u16::try_from(index).expect("profile index is bounded by 256"))
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn offset(p: [f64; 2], d: [f64; 2], amount: f64) -> [f64; 2] {
    [p[0] + d[0] * amount, p[1] + d[1] * amount]
}

/// One scanline across the bars, from the symbol start `a` to its end `b`.
#[derive(Clone, Copy)]
struct Scan {
    a: [f64; 2],
    b: [f64; 2],
}

impl Scan {
    fn at(&self, index: usize) -> [f64; 2] {
        lerp(
            self.a,
            self.b,
            (index_f64(index) + 0.5) / index_f64(SAMPLES),
        )
    }

    fn profile(&self, evidence: &mut Evidence<'_>) -> Option<Row> {
        let mut row = [0.; SAMPLES];
        for (index, value) in row.iter_mut().enumerate() {
            *value = evidence.smooth(self.at(index))?;
        }
        Some(row)
    }

    fn towards(&self, other: &Self, t: f64) -> Self {
        Self {
            a: lerp(self.a, other.a, t),
            b: lerp(self.b, other.b, t),
        }
    }
}

/// Profile minus its local mean, so illumination gradients do not enter the comparison.
fn centered(row: &Row) -> Row {
    let mut result = [0.; SAMPLES];
    for (index, value) in result.iter_mut().enumerate() {
        let low = index.saturating_sub(8);
        let high = (index + 9).min(SAMPLES);
        *value = row[index] - row[low..high].iter().sum::<f64>() / index_f64(high - low);
    }
    result
}

/// Blocks whose normalized correlation exceeds 0.5 (one-sample phase tolerance) and the mean
/// correlation. Blocks without contrast count as disagreement.
fn agreement(reference: &Row, row: &Row) -> (usize, f64) {
    let mut good = 0;
    let mut total = 0.;
    for block in 0..BLOCKS {
        let mut best: f64 = -1.;
        for shift in 0..3 {
            let (mut dot, mut reference_energy, mut row_energy) = (0., 0., 0.);
            for index in block * BLOCK + 1..(block + 1) * BLOCK - 1 {
                let value = reference[index];
                let other = row[index + shift - 1];
                dot += value * other;
                reference_energy += value * value;
                row_energy += other * other;
            }
            if row_energy > reference_energy * 0.15 {
                best = best.max(dot / (reference_energy * row_energy).sqrt().max(1.));
            }
        }
        if best > 0.5 {
            good += 1;
        }
        total += best;
    }
    (good, total / index_f64(BLOCKS))
}

/// Darkest sample of each block and its threshold halfway (55%) towards the local paper level.
fn probes(row: &Row) -> [(usize, f64); BLOCKS] {
    std::array::from_fn(|block| {
        let index = (block * BLOCK..(block + 1) * BLOCK)
            .min_by(|&x, &y| row[x].total_cmp(&row[y]))
            .unwrap_or(block * BLOCK);
        let low = index.saturating_sub(8);
        let high = (index + 9).min(SAMPLES);
        let paper = row[low..high].iter().copied().fold(0., f64::max);
        (index, row[index] + (paper - row[index]) * 0.55)
    })
}

/// Fraction of the way from `from` to `to` that the probe bars stay dark, walking in half-pixel
/// steps; six broken probes end the walk.
fn walk(
    evidence: &mut Evidence<'_>,
    from: &Scan,
    to: &Scan,
    probes: &[(usize, f64); BLOCKS],
) -> Option<f64> {
    let length = distance(from.a, to.a).max(distance(from.b, to.b));
    let mut broken = [false; BLOCKS];
    let mut along = 0.5;
    while along <= length {
        let scan = from.towards(to, along / length);
        for (block, &(index, cut)) in probes.iter().enumerate() {
            if !broken[block] && evidence.smooth(scan.at(index))? > cut {
                broken[block] = true;
            }
        }
        if broken.iter().filter(|&&b| b).count() >= 6 {
            return Some((along - 0.5) / length);
        }
        along += 0.5;
    }
    Some(1.)
}

/// Last scanline on one side (`normal` points outward) that still belongs to the symbol.
/// `None` when the image or work budget ends before the bars do.
fn grow_side(evidence: &mut Evidence<'_>, seed: Scan, row: &Row, normal: [f64; 2]) -> Option<Scan> {
    let length = distance(seed.a, seed.b);
    let step = (length / 48.).clamp(1., 16.);
    let (mut current, mut raw, mut reference) = (seed, *row, centered(row));
    for _ in 0..barcode_research_core::numeric::f64_usize((2.5 * length / step).ceil()) {
        let axis = [
            (current.b[0] - current.a[0]) / length,
            (current.b[1] - current.a[1]) / length,
        ];
        let straight = Scan {
            a: offset(current.a, normal, step),
            b: offset(current.b, normal, step),
        };
        let mut best: Option<(Scan, Row, Row, f64)> = None;
        for (start, end) in [
            (0., 0.),
            (-1., 0.),
            (1., 0.),
            (0., -1.),
            (0., 1.),
            (-1., -1.),
            (1., 1.),
        ] {
            let next = Scan {
                a: offset(straight.a, axis, start * step * 0.5),
                b: offset(straight.b, axis, end * step * 0.5),
            };
            let candidate = next.profile(evidence)?;
            let values = centered(&candidate);
            let (good, mean) = agreement(&reference, &values);
            if good >= 6 && mean > 0.55 && best.as_ref().is_none_or(|b| mean > b.3) {
                best = Some((next, candidate, values, mean));
                // A clearly continuing straight row needs no search.
                if start == 0. && end == 0. && mean > 0.75 {
                    break;
                }
            }
        }
        let target = best.as_ref().map_or(straight, |b| b.0);
        let reached = walk(evidence, &current, &target, &probes(&raw))?;
        match best {
            Some((next, candidate, values, _)) if reached >= 1. => {
                current = next;
                raw = candidate;
                reference = values;
            }
            _ => return Some(current.towards(&target, reached)),
        }
    }
    // A work limit is not a measured end.
    None
}

pub(super) struct Area {
    pub(super) polygon: Quad,
    axis: [f64; 2],
    length: f64,
    top: Scan,
    bottom: Scan,
}

/// Grow the area of the symbol decoded along `quad` (first edge along the scan axis).
pub(super) fn grow(evidence: &mut Evidence<'_>, quad: Quad) -> Option<Area> {
    let seed = line(quad);
    let length = distance(seed[0], seed[1]);
    if !(48. ..=4096.).contains(&length) {
        return None;
    }
    let scan = Scan {
        a: seed[0],
        b: seed[1],
    };
    let row = scan.profile(evidence)?;
    let (low, high) = row
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &v| {
            (l.min(v), h.max(v))
        });
    if high - low < 20. {
        return None;
    }
    let axis = [
        (seed[1][0] - seed[0][0]) / length,
        (seed[1][1] - seed[0][1]) / length,
    ];
    let normal = [-axis[1], axis[0]];
    let top = grow_side(evidence, scan, &row, [-normal[0], -normal[1]])?;
    let bottom = grow_side(evidence, scan, &row, normal)?;
    Some(Area {
        polygon: [top.a, top.b, bottom.b, bottom.a],
        axis,
        length,
        top,
        bottom,
    })
}

impl Area {
    /// Signed offset of a scanline's midpoint across the bars, along this area's normal.
    fn depth(&self, scan: &Scan) -> f64 {
        let m = lerp(scan.a, scan.b, 0.5);
        let o = lerp(self.top.a, self.top.b, 0.5);
        (m[0] - o[0]) * -self.axis[1] + (m[1] - o[1]) * self.axis[0]
    }

    /// Whether `other`, grown from a read of the same payload, continues this symbol across an
    /// interruption such as glare: scan axes within 8 degrees (skewed reads), symbol start and end
    /// within 4% of the length along the scan axis, areas less than 6% of the length apart across
    /// the bars, and the middle of the gap still carrying the bar pattern of the facing edge (at
    /// least 10% of its energy and three of eight blocks agreeing).
    pub(super) fn adjoins(&self, other: &Self, evidence: &mut Evidence<'_>) -> bool {
        if (self.axis[0] * other.axis[1] - self.axis[1] * other.axis[0]).abs()
            > 8_f64.to_radians().sin()
        {
            return false;
        }
        let along = |p: [f64; 2]| p[0] * self.axis[0] + p[1] * self.axis[1];
        let tolerance = 0.04 * self.length.max(other.length);
        // Extent along the scan axis, whichever direction each read was decoded in.
        let extent = |a: &Self| {
            let (x, y) = (
                along(lerp(a.top.a, a.bottom.a, 0.5)),
                along(lerp(a.top.b, a.bottom.b, 0.5)),
            );
            (x.min(y), x.max(y))
        };
        let ((s0, s1), (o0, o1)) = (extent(self), extent(other));
        if (s0 - o0).abs() > tolerance || (s1 - o1).abs() > tolerance {
            return false;
        }
        // Closest corners: a skewed read grows a slanted edge.
        let span = |q: &Quad| {
            q.iter()
                .map(|&p| self.depth(&Scan { a: p, b: p }))
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), d| {
                    (l.min(d), h.max(d))
                })
        };
        let ((a0, a1), (b0, b1)) = (span(&self.polygon), span(&other.polygon));
        let below = b0 >= a1;
        let gap = if below { b0 - a1 } else { a0 - b1 };
        if gap >= 0.06 * self.length.max(other.length) {
            return false;
        }
        if gap <= 0. {
            return true;
        }
        // The facing edge moved to the middle of the gap must still show its bars; a printed
        // separator or paper between two equal labels carries none.
        let (near, sign) = if below {
            (self.bottom, 1.)
        } else {
            (self.top, -1.)
        };
        let normal = [-self.axis[1] * sign, self.axis[0] * sign];
        let middle = Scan {
            a: offset(near.a, normal, gap * 0.5),
            b: offset(near.b, normal, gap * 0.5),
        };
        let (Some(reference), Some(row)) = (near.profile(evidence), middle.profile(evidence))
        else {
            return false;
        };
        let (reference, row) = (centered(&reference), centered(&row));
        let energy = |values: &Row| values.iter().map(|v| v * v).sum::<f64>();
        energy(&row) >= 0.1 * energy(&reference) && agreement(&reference, &row).0 >= 3
    }

    /// Extend this area over an adjoining one.
    pub(super) fn absorb(&mut self, other: &Self) {
        for scan in [other.top, other.bottom] {
            let depth = self.depth(&scan);
            if depth < 0. {
                self.top = scan;
            } else if depth > self.depth(&self.bottom) {
                self.bottom = scan;
            }
        }
        self.polygon = [self.top.a, self.top.b, self.bottom.b, self.bottom.a];
    }

    /// Whether the read decoded along `quad` lies on this symbol: a roughly parallel scanline with
    /// at least seven of nine points inside the area (2% margin).
    pub(super) fn owns(&self, quad: Quad) -> bool {
        let [start, end] = line(quad);
        let width = distance(start, end);
        let direction = [end[0] - start[0], end[1] - start[1]];
        if width < 24.
            || (direction[0] * self.axis[0] + direction[1] * self.axis[1]).abs() / width < 0.8
        {
            return false;
        }
        let q = self.polygon;
        let winding = (0..4)
            .map(|k| q[k][0] * q[(k + 1) % 4][1] - q[(k + 1) % 4][0] * q[k][1])
            .sum::<f64>()
            .signum();
        let margin = 0.02 * self.length;
        let inside = |p: [f64; 2]| {
            (0..4).all(|k| {
                let (s, e) = (q[k], q[(k + 1) % 4]);
                let edge = distance(s, e).max(1e-9);
                winding * ((e[0] - s[0]) * (p[1] - s[1]) - (e[1] - s[1]) * (p[0] - s[0])) / edge
                    >= -margin
            })
        };
        (1..=9)
            .filter(|&k| inside(lerp(start, end, f64::from(k) / 10.)))
            .count()
            >= 7
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars() -> Vec<u8> {
        let mut pixels = vec![240; 320 * 260];
        for y in 20..240 {
            for x in 60..252 {
                if (x - 60) / 3 % 3 == 0 {
                    pixels[y * 320 + x] = 20;
                }
            }
        }
        pixels
    }

    fn evidence(data: &[u8], width: usize, height: usize, remaining: usize) -> Evidence<'_> {
        Evidence {
            image: crate::Image {
                data,
                width,
                height,
                channels: 1,
                stride: width,
            },
            remaining,
        }
    }

    fn band(y: f64) -> Quad {
        [[60., y - 2.], [252., y - 2.], [252., y + 2.], [60., y + 2.]]
    }

    fn grown(pixels: &[u8]) -> Option<Area> {
        grow(&mut evidence(pixels, 320, 260, 4_000_000), band(70.))
    }

    #[test]
    fn grows_to_the_physical_bar_ends() {
        let area = grown(&bars()).unwrap();
        assert!(
            area.polygon[0][1] < 21. && area.polygon[2][1] > 238.,
            "{:?}",
            area.polygon
        );
        assert!(area.owns(band(180.)));
        assert!(!area.owns([[60., 248.], [252., 248.], [252., 252.], [60., 252.]]));
    }

    #[test]
    fn one_pixel_separator_ends_the_area() {
        let mut pixels = bars();
        pixels[120 * 320..121 * 320].fill(240);
        let area = grown(&pixels).unwrap();
        assert!(area.polygon[2][1] < 121., "{:?}", area.polygon);
        assert!(!area.owns(band(180.)));
    }

    #[test]
    fn exhausted_budget_is_not_an_area() {
        assert!(grow(&mut evidence(&bars(), 320, 260, 1100), band(70.)).is_none());
    }

    #[test]
    fn illumination_changes_do_not_truncate_bars() {
        let mut gradient = bars();
        for y in 20..240 {
            for x in 60..252 {
                let lift = u8::try_from(y / 3).unwrap();
                gradient[y * 320 + x] = if (x - 60) / 3 % 3 == 0 { 20 } else { 120 } + lift;
            }
        }
        let area = grown(&gradient).unwrap();
        assert!(area.polygon[0][1] < 21. && area.polygon[2][1] > 238.);
        let mut step = bars();
        for value in &mut step[120 * 320..240 * 320] {
            if *value < 100 {
                *value = 110;
            }
        }
        // Bars that stay darker than the paper continue the symbol; never a partial area.
        assert!(grown(&step).is_none_or(|a| a.polygon[2][1] > 238.));
    }

    #[test]
    fn weak_single_bars_do_not_end_the_area() {
        let mut pixels = bars();
        for y in 120..240 {
            for start in [87, 141, 204] {
                pixels[y * 320 + start..y * 320 + start + 3].fill(110);
            }
        }
        let area = grown(&pixels).unwrap();
        assert!(area.polygon[2][1] > 238.);
        assert!(area.owns(band(180.)));
    }

    #[test]
    fn follows_perspective_and_large_bars_without_bridging_a_cut() {
        let (width, height) = (1280, 1040);
        let mut pixels = vec![240; width * height];
        // Bars widen towards the bottom like a code seen in perspective.
        for y in 80..960 {
            let scale = 1. + f64::from(u32::try_from(y).unwrap()) / 2000.;
            for x in 200..1080 {
                let u = (f64::from(u32::try_from(x).unwrap()) - 640.) / scale + 640.;
                if (240. ..1008.).contains(&u)
                    && (barcode_research_core::numeric::f64_usize(u - 240.) / 12).is_multiple_of(3)
                {
                    pixels[y * width + x] = 20;
                }
            }
        }
        let q = [[240., 278.], [1008., 278.], [1008., 282.], [240., 282.]];
        let full = grow(&mut evidence(&pixels, width, height, 8_000_000), q).unwrap();
        assert!(
            full.polygon[0][1] < 86. && full.polygon[2][1] > 953.,
            "{:?}",
            full.polygon
        );
        assert!(full.owns([[220., 900.], [1030., 900.], [1030., 904.], [220., 904.]]));
        pixels[520 * width..521 * width].fill(240);
        let split = grow(&mut evidence(&pixels, width, height, 8_000_000), q).unwrap();
        assert!(split.polygon[2][1] < 521.);
        assert!(!split.owns([[230., 778.], [1020., 778.], [1020., 782.], [230., 782.]]));
    }
}

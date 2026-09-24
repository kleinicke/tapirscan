//! Bounded profile agreement for display geometry, never duplicate ownership.
use super::{distance, line, point, project, Evidence};
use crate::Quad;

const SAMPLES: usize = 256;
type Row = [f64; SAMPLES];

struct Band {
    origin: [f64; 2],
    axis: [f64; 2],
    width: f64,
}

fn coordinate(index: usize) -> f64 {
    f64::from(u16::try_from(index).expect("profile index is bounded by 256"))
}

impl Band {
    fn point(&self, across: f64, depth: f64) -> [f64; 2] {
        point(self.origin, self.axis, across, depth)
    }

    fn profile(&self, evidence: &mut Evidence<'_>, shift: f64, depth: f64) -> Option<Row> {
        let mut row = [0.; SAMPLES];
        for (index, value) in row.iter_mut().enumerate() {
            *value = evidence
                .smooth(self.point(shift + (coordinate(index) + 0.5) * self.width / 256., depth))?;
        }
        Some(row)
    }
}

fn centered(row: &Row) -> Row {
    let mut result = [0.; SAMPLES];
    for (index, value) in result.iter_mut().enumerate() {
        let low = index.saturating_sub(8);
        let high = (index + 9).min(SAMPLES);
        *value = row[index] - row[low..high].iter().sum::<f64>() / coordinate(high - low);
    }
    result
}

fn score(reference: &Row, row: &Row) -> (usize, f64) {
    let values = centered(row);
    let mut good = 0;
    let mut total = 0.;
    for block in 0..8 {
        let mut correlation: f64 = -1.;
        // Small local phase changes allow perspective without matching unrelated runs.
        for offset in 0..3 {
            let mut dot = 0.;
            let mut reference_energy = 0.;
            let mut row_energy = 0.;
            for (index, &value) in reference
                .iter()
                .enumerate()
                .take((block + 1) * 32 - 1)
                .skip(block * 32 + 1)
            {
                let other = values[index + offset - 1];
                dot += value * other;
                reference_energy += value * value;
                row_energy += other * other;
            }
            if row_energy > reference_energy * 0.15 {
                correlation = correlation.max(dot / (reference_energy * row_energy).sqrt().max(1.));
            }
        }
        if correlation > 0.45 {
            good += 1;
        }
        total += correlation;
    }
    (good, total / 8.)
}

struct Cursor {
    row: Row,
    shift: f64,
    depth: f64,
    broken: [bool; 8],
}

impl Cursor {
    fn continuous(
        &mut self,
        evidence: &mut Evidence<'_>,
        band: &Band,
        probes: &[usize; 8],
        shift: f64,
        movement: (f64, f64),
    ) -> Option<bool> {
        let (distance, sign) = movement;
        let mut along = 0.5;
        while along <= distance {
            let fraction = along / distance;
            let across = self.shift + (shift - self.shift) * fraction;
            let depth = self.depth + sign * along;
            let mut light = 0;
            for (block, &index) in probes.iter().enumerate() {
                let low = index.saturating_sub(8);
                let high = (index + 9).min(SAMPLES);
                let paper = self.row[low..high].iter().copied().fold(0., f64::max);
                let cut = self.row[index] + (paper - self.row[index]) * 0.55;
                if evidence.smooth(band.point(
                    across + (coordinate(index) + 0.5) * band.width / 256.,
                    depth,
                ))? > cut
                {
                    self.broken[block] = true;
                }
                if self.broken[block] {
                    light += 1;
                }
            }
            if light >= 6 {
                return Some(false);
            }
            along += 0.5;
        }
        Some(true)
    }
}

fn follow(
    evidence: &mut Evidence<'_>,
    band: &Band,
    reference: &Row,
    template: &Row,
    probes: &[usize; 8],
    sign: f64,
) -> Option<(f64, f64)> {
    let advance = (band.width / 128.).clamp(0.5, 8.);
    let mut cursor = Cursor {
        row: *reference,
        shift: 0.,
        depth: 0.,
        broken: [false; 8],
    };
    for _ in 0..128 {
        let next = cursor.depth + sign * advance;
        let mut best = None;
        for offset in [0., -band.width / 512., band.width / 512.] {
            let shift = cursor.shift + offset;
            if shift.abs() > band.width * 0.04 {
                continue;
            }
            let row = band.profile(evidence, shift, next)?;
            let (good, agreement) = score(template, &row);
            if good >= 6
                && agreement > 0.55
                && best.as_ref().is_none_or(|(_, _, old)| agreement > *old)
            {
                best = Some((shift, row, agreement));
                // Most rows need no shift search.
                if offset == 0. {
                    break;
                }
            }
        }
        let Some((shift, row, _)) = best else {
            return Some((cursor.shift, cursor.depth));
        };
        if !cursor.continuous(evidence, band, probes, shift, (advance, sign))? {
            return Some((cursor.shift, cursor.depth));
        }
        cursor.row = row;
        cursor.shift = shift;
        cursor.depth = next;
    }
    // A work limit is not a measured endpoint.
    None
}

pub(super) fn measure(evidence: &mut Evidence<'_>, quad: Quad) -> Option<Quad> {
    let seed = line(quad);
    let width = distance(seed[0], seed[1]);
    if !(96. ..=4096.).contains(&width) {
        return None;
    }
    let band = Band {
        origin: seed[0],
        axis: [
            (seed[1][0] - seed[0][0]) / width,
            (seed[1][1] - seed[0][1]) / width,
        ],
        width,
    };
    let reference = band.profile(evidence, 0., 0.)?;
    let template = centered(&reference);
    let mut probes = [0; 8];
    for (block, probe) in probes.iter_mut().enumerate() {
        *probe = (block * 32..(block + 1) * 32)
            .min_by(|&left, &right| template[left].total_cmp(&template[right]))?;
    }
    let top = follow(evidence, &band, &reference, &template, &probes, -1.)?;
    let bottom = follow(evidence, &band, &reference, &template, &probes, 1.)?;
    let first = (project(quad[0], band.origin, band.axis)[1]
        + project(quad[1], band.origin, band.axis)[1])
        * 0.5;
    let last = (project(quad[2], band.origin, band.axis)[1]
        + project(quad[3], band.origin, band.axis)[1])
        * 0.5;
    let low = first.min(last);
    let high = first.max(last);
    // Corner winding must not turn an enlargement into a shrink.
    if top.1 > low + 2.
        || bottom.1 < high - 2.
        || bottom.1 - top.1 > width
        || low - top.1 + bottom.1 - high < 4.
    {
        return None;
    }
    Some([
        band.point(top.0, top.1),
        band.point(width + top.0, top.1),
        band.point(width + bottom.0, bottom.1),
        band.point(bottom.0, bottom.1),
    ])
}

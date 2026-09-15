//! Experimental full-image EAN-13 run-width scanner. No model or supplied boxes.
//! Axis-aligned rows/columns only; every source row is scanned. Consecutive-row
//! agreement emits text; gaps break groups, including one white source row.
#![forbid(unsafe_code)]
use crate::{
    ean,
    sampling::{Error, ImageView},
};
pub const MAX_RESULTS: usize = 4096;
#[derive(Clone, Debug)]
pub struct Candidate {
    pub polygon: [[f64; 2]; 4],
    pub digits: Option<[u8; 13]>,
    pub support: usize,
    pub fragments: usize,
    pub(crate) axis: usize,
    first: usize,
    last: usize,
    left: usize,
    right: usize,
    hypothesis: Option<[u8; 13]>,
}
#[derive(Default)]
pub struct Scanner {
    assembler: crate::row_group::Assembler,
    gray: Vec<u8>,
    transpose: Vec<u8>,
    runs: Vec<(usize, usize, bool)>,
    results: Vec<Candidate>,
    pub rows: usize,
    pub hypotheses: usize,
    pub truncated: bool,
}
fn structure(r: &[(usize, usize, bool)]) -> Option<([f32; 95], usize, usize)> {
    if r.len() < 61 || !r[1].2 {
        return None;
    }
    let runs = &r[1..60];
    let span = runs[58].1 - runs[0].0;
    let module = span as f64 / 95.;
    if module < 0.8
        || r[0].1 - r[0].0 < (7. * module) as usize
        || r[60].1 - r[60].0 < (7. * module) as usize
    {
        return None;
    }
    for i in [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58] {
        if ((runs[i].1 - runs[i].0) as f64 / module - 1.).abs() > 0.65 {
            return None;
        }
    }
    let mut bits = [0.; 95];
    for i in [0, 2, 46, 48, 92, 94] {
        bits[i] = 1.;
    }
    for digit in 0..12 {
        let start = if digit < 6 {
            3 + digit * 4
        } else {
            32 + (digit - 6) * 4
        };
        let scale = (runs[start + 3].1 - runs[start].0) as f64 / 7.;
        if !(0.55 * module..=1.8 * module).contains(&scale) {
            return None;
        }
        let mut at = if digit < 6 {
            3 + digit * 7
        } else {
            50 + (digit - 6) * 7
        };
        let end = at + 7;
        for run in &runs[start..start + 4] {
            let width = (run.1 - run.0) as f64 / scale;
            let n = width.round() as usize;
            if !(1..=4).contains(&n) || (width - n as f64).abs() > 0.55 || at + n > end {
                return None;
            }
            bits[at..at + n].fill(if run.2 { 1. } else { 0. });
            at += n;
        }
        if at != end {
            return None;
        }
    }
    Some((bits, runs[0].0, runs[58].1))
}
fn soft_structure(r: &[(usize, usize, bool)]) -> Option<(Option<[u8; 13]>, usize, usize)> {
    if r.len() < 61 || !r[1].2 {
        return None;
    }
    let start = r[1].0;
    let end = r[59].1;
    let module = (end - start) as f64 / 95.;
    if module < 0.8
        || r[0].1 - r[0].0 < (7. * module) as usize
        || r[60].1 - r[60].0 < (7. * module) as usize
    {
        return None;
    }
    let mut widths = [0.; 59];
    for i in 0..59 {
        widths[i] = (r[i + 1].1 - r[i + 1].0) as f32;
    }
    for i in [0, 1, 2, 27, 28, 29, 30, 31, 56, 57, 58] {
        if (f64::from(widths[i]) / module - 1.).abs() > 0.65 {
            return None;
        }
    }
    let forward = crate::run_ean::decode(&widths);
    widths.reverse();
    let reverse = crate::run_ean::decode(&widths);
    let digits = match (forward, reverse) {
        (Some(a), Some(b)) if a != b => None,
        (a, b) => a.or(b),
    };
    Some((digits, start, end))
}
impl Scanner {
    pub fn grouping_stats(&self) -> (usize, usize, bool) {
        (
            self.assembler.checks,
            self.assembler.merged,
            self.assembler.exhausted,
        )
    }
    pub fn scan_grouped(&mut self, image: ImageView<'_>) -> Result<&[Candidate], Error> {
        self.scan(image)?;
        let gray = ImageView::new(&self.gray, image.width, image.height, 1, image.width)?;
        self.assembler.assemble(gray, &self.results)
    }
    pub fn scan_soft_grouped(&mut self, image: ImageView<'_>) -> Result<&[Candidate], Error> {
        self.scan_mode(image, true)?;
        let gray = ImageView::new(&self.gray, image.width, image.height, 1, image.width)?;
        self.assembler.assemble(gray, &self.results)
    }
    pub fn results(&self) -> &[Candidate] {
        &self.results
    }
    /// Validated image is borrowed, all scratch/results owned and reusable. Failure
    /// clears results. Cap only limits retained observations, never row execution;
    /// truncated=true means output coverage is incomplete. No cross-gap text merge.
    pub fn scan(&mut self, image: ImageView<'_>) -> Result<&[Candidate], Error> {
        self.scan_mode(image, false)
    }
    fn scan_mode(&mut self, image: ImageView<'_>, soft: bool) -> Result<&[Candidate], Error> {
        self.results.clear();
        self.rows = 0;
        self.hypotheses = 0;
        self.truncated = false;
        let max = image.width.max(image.height);
        let pixels = image
            .width
            .checked_mul(image.height)
            .ok_or(Error::Dimensions)?;
        if max > 16384 || pixels > 32 * 1024 * 1024 {
            return Err(Error::Dimensions);
        }
        self.gray
            .try_reserve(pixels.saturating_sub(self.gray.len()))
            .map_err(|_| Error::Allocation)?;
        self.transpose
            .try_reserve(pixels.saturating_sub(self.transpose.len()))
            .map_err(|_| Error::Allocation)?;
        self.gray.resize(pixels, 0);
        self.transpose.resize(pixels, 0);
        for y in 0..image.height {
            for x in 0..image.width {
                let p = y * image.stride + x * image.channels;
                self.gray[y * image.width + x] = if image.channels == 1 {
                    image.data[p]
                } else {
                    ((306 * u32::from(image.data[p])
                        + 601 * u32::from(image.data[p + 1])
                        + 117 * u32::from(image.data[p + 2])
                        + 512)
                        >> 10) as u8
                };
            }
        }
        // Tiled transpose makes both scan directions sequential, avoiding a second
        // strided RGB traversal. Pixel values and all decoder decisions stay exact.
        for by in (0..image.height).step_by(32) {
            for bx in (0..image.width).step_by(32) {
                for y in by..(by + 32).min(image.height) {
                    for x in bx..(bx + 32).min(image.width) {
                        self.transpose[x * image.height + y] = self.gray[y * image.width + x];
                    }
                }
            }
        }
        self.runs
            .try_reserve(max.saturating_sub(self.runs.len()))
            .map_err(|_| Error::Allocation)?;
        self.results
            .try_reserve(MAX_RESULTS)
            .map_err(|_| Error::Allocation)?;
        for axis in 0..2 {
            let (length, lines) = if axis == 0 {
                (image.width, image.height)
            } else {
                (image.height, image.width)
            };
            let plane = if axis == 0 {
                &self.gray
            } else {
                &self.transpose
            };
            for line in 0..lines {
                self.rows += 1;
                let (mut lo, mut hi) = (255, 0);
                let row = &plane[line * length..(line + 1) * length];
                for &v in row {
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
                if hi - lo < 40 {
                    continue;
                }
                let threshold = (u16::from(lo) + u16::from(hi)) / 2;
                self.runs.clear();
                let mut start = 0;
                let mut black = u16::from(row[0]) <= threshold;
                for u in 1..=length {
                    let next = u < length && u16::from(row[u]) <= threshold;
                    if u == length || next != black {
                        self.runs.push((start, u, black));
                        start = u;
                        black = next;
                    }
                }
                for i in 0..self.runs.len().saturating_sub(60) {
                    let (digits, left, right) = if soft {
                        let Some(result) = soft_structure(&self.runs[i..i + 61]) else {
                            continue;
                        };
                        self.hypotheses += 1;
                        result
                    } else {
                        let Some((mut bits, left, right)) = structure(&self.runs[i..i + 61]) else {
                            continue;
                        };
                        self.hypotheses += 1;
                        let forward = ean::decode(&bits, 0.1, 0.02).map(|r| r.digits);
                        bits.reverse();
                        let reverse = ean::decode(&bits, 0.1, 0.02).map(|r| r.digits);
                        let digits = match (forward, reverse) {
                            (Some(a), Some(b)) if a != b => None,
                            (a, b) => a.or(b),
                        };
                        (digits, left, right)
                    };
                    // Only the previous source row can extend a region. A rejected/white row
                    // prevents association across physically separated equal-text symbols.
                    let index = self.results.iter().rposition(|c| {
                        c.axis == axis
                            && c.last + 1 == line
                            && c.hypothesis == digits
                            && c.right.min(right).saturating_sub(c.left.max(left)) as f64
                                > 0.9 * (c.right.max(right) - c.left.min(left)) as f64
                    });
                    if let Some(j) = index {
                        let c = &mut self.results[j];
                        c.last = line;
                        c.support += 1;
                        c.left = c.left.min(left);
                        c.right = c.right.max(right);
                        c.digits = digits.filter(|_| c.support >= 2);
                    } else if self.results.len() < MAX_RESULTS {
                        self.results.push(Candidate {
                            polygon: [[0.; 2]; 4],
                            digits: None,
                            support: 1,
                            fragments: 1,
                            axis,
                            first: line,
                            last: line,
                            left,
                            right,
                            hypothesis: digits,
                        });
                    } else {
                        self.truncated = true;
                    }
                }
            }
        }
        for c in &mut self.results {
            let p = [
                [c.left as f64 - 0.5, c.first as f64 - 0.5],
                [c.right as f64 - 0.5, c.first as f64 - 0.5],
                [c.right as f64 - 0.5, c.last as f64 + 0.5],
                [c.left as f64 - 0.5, c.last as f64 + 0.5],
            ];
            c.polygon = if c.axis == 0 {
                p
            } else {
                p.map(|[x, y]| [y, x])
            };
        }
        self.results
            .sort_by_key(|c| (c.digits.is_none(), std::cmp::Reverse(c.support)));
        Ok(&self.results)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn image(gap: bool) -> Vec<u8> {
        let bits = ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]);
        let mut p = vec![255; 440 * 80];
        for y in 10..70 {
            if gap && y == 40 {
                continue;
            }
            for x in 30..410 {
                if bits[(x - 30) / 4] > 0.5 {
                    p[y * 440 + x] = 0;
                }
            }
        }
        p
    }
    #[test]
    fn gap_and_ownership() {
        let mut s = Scanner::default();
        let p = image(true);
        let old = s
            .scan(ImageView::new(&p, 440, 80, 1, 440).unwrap())
            .unwrap()
            .to_vec();
        assert_eq!(old.iter().filter(|r| r.digits.is_some()).count(), 2);
        assert!(old[0].polygon[2][1] < 40. || old[0].polygon[0][1] > 40.);
        let p = image(false);
        assert_eq!(
            s.scan(ImageView::new(&p, 440, 80, 1, 440).unwrap())
                .unwrap()
                .iter()
                .filter(|r| r.digits.is_some())
                .count(),
            1
        );
        assert_eq!(old.len(), 2);
    }
    #[test]
    fn rotate_and_reverse() {
        let p = image(false);
        let mut rotated = vec![255; p.len()];
        for y in 0..80 {
            for x in 0..440 {
                rotated[(439 - x) * 80 + y] = p[y * 440 + x];
            }
        }
        let mut s = Scanner::default();
        let r = s
            .scan(ImageView::new(&rotated, 80, 440, 1, 80).unwrap())
            .unwrap();
        assert_eq!(r.iter().filter(|r| r.digits.is_some()).count(), 1);
        assert_eq!(
            r[0].digits.unwrap(),
            [5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 7]
        );
    }
    #[test]
    fn checksum_rejection_retains_undecoded_and_dimension_error_clears() {
        let bits = ean::encode(&[5, 9, 0, 1, 2, 3, 4, 1, 2, 3, 4, 5, 8]);
        let mut p = vec![255; 440 * 20];
        for y in 2..18 {
            for x in 30..410 {
                if bits[(x - 30) / 4] > 0.5 {
                    p[y * 440 + x] = 0;
                }
            }
        }
        let mut s = Scanner::default();
        let r = s
            .scan(ImageView::new(&p, 440, 20, 1, 440).unwrap())
            .unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].digits.is_none());
        let p = vec![255; 16385];
        assert!(s
            .scan(ImageView::new(&p, 16385, 1, 1, 16385).unwrap())
            .is_err());
        assert!(s.results().is_empty());
    }
    #[test]
    fn blank_and_single_row_do_not_emit_text() {
        let mut s = Scanner::default();
        let p = vec![255; 440 * 80];
        assert!(s
            .scan(ImageView::new(&p, 440, 80, 1, 440).unwrap())
            .unwrap()
            .is_empty());
        let mut p = image(false);
        for y in 11..70 {
            p[y * 440..(y + 1) * 440].fill(255);
        }
        let r = s
            .scan(ImageView::new(&p, 440, 80, 1, 440).unwrap())
            .unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].digits.is_none());
    }
    #[test]
    fn grouped_gap_and_decode_damage() {
        let mut s = Scanner::default();
        for gap in [false, true] {
            let p = image(gap);
            let r = s
                .scan_grouped(ImageView::new(&p, 440, 80, 1, 440).unwrap())
                .unwrap();
            assert_eq!(
                r.iter().filter(|c| c.digits.is_some()).count(),
                if gap { 2 } else { 1 }
            );
        }
        // A one-pixel defect changes a run width and rejects the row decoder, while
        // the surrounding 95-module profile still supplies continuity evidence.
        let mut p = image(false);
        for x in 50..54 {
            p[40 * 440 + x] = 255 - p[40 * 440 + x];
        }
        let im = ImageView::new(&p, 440, 80, 1, 440).unwrap();
        let raw = s
            .scan(im)
            .unwrap()
            .iter()
            .filter(|c| c.digits.is_some())
            .count();
        assert!(raw >= 2);
        let r = s.scan_grouped(im).unwrap();
        assert_eq!(r.iter().filter(|c| c.digits.is_some()).count(), 1);
        assert!(r[0].fragments >= 2);
    }
}

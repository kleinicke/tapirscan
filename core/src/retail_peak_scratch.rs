//! Allocation reuse for the existing eight-bit peak/valley crossing recipe.
//! Run widths preserve its f32 subtraction and f64 cumulative representation.
// Preserve validated arithmetic and observation layout. Coordinates are bounded
// by image/profile limits; digit and pixel casts follow explicit clamps.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss
)]
#[derive(Default)]
pub struct Scratch {
    extrema: Vec<(usize, u8)>,
    edges: Vec<f32>,
    runs: Vec<(f64, f64, bool)>,
}
impl Scratch {
    pub fn runs(&mut self, row: &[u8]) -> &[(f64, f64, bool)] {
        self.extrema.clear();
        self.edges.clear();
        self.runs.clear();
        if row.is_empty() {
            return &self.runs;
        }
        let (mut low, mut high) = (row[0], row[0]);
        let (mut low_at, mut high_at, mut direction) = (0, 0, 0);
        for (i, &value) in row.iter().enumerate() {
            if direction >= 0 {
                if value >= high {
                    high = value;
                    high_at = i;
                }
                if high.saturating_sub(value) >= 8 {
                    self.extrema.push((high_at, high));
                    direction = -1;
                    low = value;
                    low_at = i;
                }
            }
            if direction <= 0 {
                if value <= low {
                    low = value;
                    low_at = i;
                }
                if value.saturating_sub(low) >= 8 {
                    self.extrema.push((low_at, low));
                    direction = 1;
                    high = value;
                    high_at = i;
                }
            }
        }
        self.extrema.push(if direction == 1 {
            (high_at, high)
        } else {
            (low_at, low)
        });
        let first_black = self.extrema.len() > 1 && self.extrema[0].1 < self.extrema[1].1;
        self.edges.push(0.);
        for pair in self.extrema.windows(2) {
            let [(left, a), (right, b)] = [pair[0], pair[1]];
            let cut = (f32::from(a) + f32::from(b)) * 0.5;
            for i in left + 1..=right {
                let previous = f32::from(row[i - 1]);
                let next = f32::from(row[i]);
                if (previous >= cut) != (next >= cut) {
                    self.edges
                        .push(i as f32 - 1. + (cut - previous) / (next - previous));
                    break;
                }
            }
        }
        self.edges.push(row.len() as f32);
        let mut x = 0.;
        let mut dark = first_black;
        for pair in self.edges.windows(2) {
            let end = x + f64::from(pair[1] - pair[0]);
            self.runs.push((x, end, dark));
            x = end;
            dark = !dark;
        }
        &self.runs
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::retail_pipeline::peaks;
    #[test]
    fn identical_runs_to_frozen_peak_recipe() {
        let mut scratch = Scratch::default();
        let mut seed = 19u32;
        for n in [0, 1, 2, 3, 17, 64, 128, 512, 1024, 4096] {
            for mode in 0..100 {
                let row: Vec<u8> = (0..n)
                    .map(|i| {
                        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                        match mode % 5 {
                            0 => 0,
                            1 => 128,
                            2 => (i % 256) as u8,
                            3 => {
                                if i % 8 < 4 {
                                    12
                                } else {
                                    241
                                }
                            }
                            _ => (seed >> 24) as u8,
                        }
                    })
                    .collect();
                let (bits, widths, _) = peaks::runs(&row);
                let mut expected = Vec::new();
                if !bits.is_empty() {
                    let (mut x, mut dark) = (0., bits[0]);
                    for w in widths {
                        let end = x + f64::from(w);
                        expected.push((x, end, dark));
                        x = end;
                        dark = !dark;
                    }
                }
                assert_eq!(scratch.runs(&row), expected);
            }
        }
    }
}

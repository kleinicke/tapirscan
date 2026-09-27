//! Spatially restrict QR finder clustering while preserving linear-search order.
use super::Finder;

pub(super) struct Index {
    finders: Vec<Finder>,
    cells: Vec<Vec<usize>>,
    columns: usize,
    rows: usize,
}

impl Index {
    pub(super) fn new(width: usize, height: usize) -> Self {
        let columns = width.div_ceil(32).max(1);
        let rows = height.div_ceil(32).max(1);
        Self {
            finders: Vec::new(),
            cells: vec![Vec::new(); columns * rows],
            columns,
            rows,
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "Nonnegative pixel coordinates intentionally floor into bounded 32-pixel cells; Rust saturating casts and the grid clamp preserve boundary behavior."
    )]
    fn cell(&self, x: f64, y: f64) -> usize {
        let column = (x.max(0.) as usize / 32).min(self.columns - 1);
        let row = (y.max(0.) as usize / 32).min(self.rows - 1);
        row * self.columns + column
    }

    pub(super) fn insert(&mut self, x: f32, y: f32, module: f32) {
        let radius = module * 2.;
        let first = self.cell(
            f64::from(x) - f64::from(radius),
            f64::from(y) - f64::from(radius),
        );
        let last = self.cell(
            f64::from(x) + f64::from(radius),
            f64::from(y) + f64::from(radius),
        );
        let mut matched = None;
        let first_row = first / self.columns;
        let last_row = last / self.columns;
        let first_column = first % self.columns;
        let last_column = last % self.columns;
        let cell_count = (last_row - first_row + 1) * (last_column - first_column + 1);
        let eligible = |index: usize| {
            let finder = &self.finders[index];
            (finder.x - x).abs() < radius
                && (finder.y - y).abs() < radius
                && finder.module / module > 0.4
                && finder.module / module < 2.5
        };
        if cell_count >= self.finders.len() {
            matched = self
                .finders
                .iter()
                .enumerate()
                .find_map(|(index, _)| eligible(index).then_some(index));
        } else {
            for row in first_row..=last_row {
                for column in first_column..=last_column {
                    for &index in &self.cells[row * self.columns + column] {
                        if matched.is_some_and(|old| old <= index) {
                            continue;
                        }
                        if eligible(index) {
                            matched = Some(index);
                        }
                    }
                }
            }
        }
        if let Some(index) = matched {
            let before = self.cell(
                f64::from(self.finders[index].x),
                f64::from(self.finders[index].y),
            );
            let finder = &mut self.finders[index];
            #[expect(
                clippy::cast_precision_loss,
                reason = "Keep the original finder averaging arithmetic and rounding for every support count."
            )]
            let support_count = finder.support as f32;
            finder.x = (finder.x * support_count + x) / (support_count + 1.);
            finder.y = (finder.y * support_count + y) / (support_count + 1.);
            finder.module = (finder.module * support_count + module) / (support_count + 1.);
            finder.support += 1;
            let after = self.cell(
                f64::from(self.finders[index].x),
                f64::from(self.finders[index].y),
            );
            if before != after {
                self.cells[before].retain(|&old| old != index);
                self.cells[after].push(index);
            }
        } else {
            let index = self.finders.len();
            let cell = self.cell(f64::from(x), f64::from(y));
            self.cells[cell].push(index);
            self.finders.push(Finder {
                x,
                y,
                module,
                support: 1,
                quad: None,
            });
        }
    }

    pub(super) fn into_centers(self) -> Vec<Finder> {
        self.finders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_finders_match_linear_search() {
        let mut indexed = Index::new(320, 180);
        let mut expected: Vec<Finder> = Vec::new();
        let mut seed = 18_374_u32;
        for _ in 0..5000 {
            let mut random = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from(u16::try_from(seed % 65_536).unwrap()) / f32::from(u16::MAX)
            };
            let x = random() * 320.;
            let y = random() * 180.;
            let module = random() * 12. + 0.8;
            indexed.insert(x, y, module);
            if let Some(finder) = expected.iter_mut().find(|finder| {
                let finder = &**finder;
                (finder.x - x).abs() < module * 2.
                    && (finder.y - y).abs() < module * 2.
                    && finder.module / module > 0.4
                    && finder.module / module < 2.5
            }) {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "Keep the original finder averaging arithmetic and rounding for every support count."
                )]
                let support_count = finder.support as f32;
                finder.x = (finder.x * support_count + x) / (support_count + 1.);
                finder.y = (finder.y * support_count + y) / (support_count + 1.);
                finder.module = (finder.module * support_count + module) / (support_count + 1.);
                finder.support += 1;
            } else {
                expected.push(Finder {
                    x,
                    y,
                    module,
                    support: 1,
                    quad: None,
                });
            }
        }
        let actual = indexed.into_centers();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(
                (actual.x, actual.y, actual.module, actual.support),
                (expected.x, expected.y, expected.module, expected.support)
            );
        }
    }

    #[test]
    fn boundary_cells_match_linear_search() {
        let points = [
            (0.2, 0.2, 0.8),
            (31.8, 31.7, 3.2),
            (32.1, 31.9, 2.1),
            (63.8, 63.7, 12.0),
            (64.1, 64.2, 1.1),
            (95.9, 0.1, 6.4),
            (0.1, 95.8, 4.7),
        ];
        let mut indexed = Index::new(96, 96);
        let mut expected: Vec<Finder> = Vec::new();
        for &(x, y, module) in &points {
            indexed.insert(x, y, module);
            if let Some(finder) = expected.iter_mut().find(|finder| {
                let finder = &**finder;
                (finder.x - x).abs() < module * 2.
                    && (finder.y - y).abs() < module * 2.
                    && finder.module / module > 0.4
                    && finder.module / module < 2.5
            }) {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "Keep the original finder averaging arithmetic and rounding for every support count."
                )]
                let support_count = finder.support as f32;
                finder.x = (finder.x * support_count + x) / (support_count + 1.);
                finder.y = (finder.y * support_count + y) / (support_count + 1.);
                finder.module = (finder.module * support_count + module) / (support_count + 1.);
                finder.support += 1;
            } else {
                expected.push(Finder {
                    x,
                    y,
                    module,
                    support: 1,
                    quad: None,
                });
            }
        }
        let actual = indexed.into_centers();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(
                (actual.x, actual.y, actual.module, actual.support),
                (expected.x, expected.y, expected.module, expected.support)
            );
        }
    }
}

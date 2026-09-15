//! Preserve first-insertion clustering while restricting distance checks to nearby cells.
use super::Center;

pub(super) struct Index {
    pub centers: Vec<Center>,
    cells: Vec<Vec<usize>>,
    columns: usize,
    rows: usize,
}
impl Index {
    pub fn new(w: usize, h: usize) -> Self {
        let columns = w.div_ceil(32).max(1);
        let rows = h.div_ceil(32).max(1);
        Self {
            centers: Vec::new(),
            cells: vec![Vec::new(); columns * rows],
            columns,
            rows,
        }
    }
    fn cell(&self, x: f32, y: f32) -> usize {
        let column = (x.max(0.) as usize / 32).min(self.columns - 1);
        let row = (y.max(0.) as usize / 32).min(self.rows - 1);
        row * self.columns + column
    }
    pub fn insert(&mut self, x: f32, y: f32, module: f32, radius: f32) {
        let first = self.cell(x - radius, y - radius);
        let last = self.cell(x + radius, y + radius);
        let mut matched = None;
        for row in first / self.columns..=last / self.columns {
            for col in first % self.columns..=last % self.columns {
                for &index in &self.cells[row * self.columns + col] {
                    if matched.is_some_and(|old| old <= index) {
                        continue;
                    }
                    let c = &self.centers[index];
                    if (c.x - x).hypot(c.y - y) < radius {
                        matched = Some(index);
                    }
                }
            }
        }
        if let Some(index) = matched {
            let before = self.cell(self.centers[index].x, self.centers[index].y);
            let c = &mut self.centers[index];
            let n = c.support as f32;
            c.x = (c.x * n + x) / (n + 1.);
            c.y = (c.y * n + y) / (n + 1.);
            c.module = (c.module * n + module) / (n + 1.);
            c.support += 1;
            let after = self.cell(self.centers[index].x, self.centers[index].y);
            if before != after {
                self.cells[before].retain(|&old| old != index);
                self.cells[after].push(index);
            }
        } else {
            let cell = self.cell(x, y);
            self.cells[cell].push(self.centers.len());
            self.centers.push(Center {
                x,
                y,
                module,
                support: 1,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn moving_clusters_match_first_insertion_search_exactly() {
        let mut indexed = Index::new(320, 180);
        let mut expected: Vec<Center> = Vec::new();
        let mut seed = 18374_u32;
        for _ in 0..5000 {
            let mut random = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                seed as f32 / u32::MAX as f32
            };
            let x = random() * 320.;
            let y = random() * 180.;
            let module = random() * 12. + 0.8;
            let radius = module * 1.5;
            indexed.insert(x, y, module, radius);
            if let Some(c) = expected
                .iter_mut()
                .find(|c| (c.x - x).hypot(c.y - y) < radius)
            {
                let n = c.support as f32;
                c.x = (c.x * n + x) / (n + 1.);
                c.y = (c.y * n + y) / (n + 1.);
                c.module = (c.module * n + module) / (n + 1.);
                c.support += 1;
            } else {
                expected.push(Center {
                    x,
                    y,
                    module,
                    support: 1,
                });
            }
        }
        assert_eq!(indexed.centers.len(), expected.len());
        for (a, b) in indexed.centers.iter().zip(expected) {
            assert_eq!(
                (a.x, a.y, a.module, a.support),
                (b.x, b.y, b.module, b.support)
            );
        }
    }
}

//! Nearby source rows seeded only by a complete checksum-valid short observation.
// Preserve validated arithmetic and observation layout. Coordinates are bounded
// by image/profile limits; digit and pixel casts follow explicit clamps.
#![allow(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::wildcard_imports
)]
use super::*;
impl Collector {
    pub(super) fn recover_anchors(
        &mut self,
        im: ImageView<'_>,
        done: &[bool],
        work: &mut Work,
        budget: &mut AssociationBudget,
    ) -> Vec<crate::experiment::Detection> {
        let mut seeds = Vec::new();
        for (i, (q, rows)) in self.groups.iter().enumerate().take(done.len()) {
            if done[i] {
                continue;
            }
            for row in rows.iter().filter(|o| !o.ambiguous && o.digits[0] == 14) {
                if self.protected_observation(*q, row) {
                    self.protection_skips += 1;
                    continue;
                }
                if seeds
                    .iter()
                    .any(|(j, _, axis, fraction): &(usize, Quad, usize, f64)| {
                        *j == i && *axis == row.axis && (*fraction - row.fraction).abs() < 0.02
                    })
                {
                    continue;
                }
                seeds.push((i, *q, row.axis, row.fraction));
            }
        }
        let mut sampler = crate::experiment::Experiment::default();
        let mut raw = Vec::new();
        let mut found = Vec::new();
        let mut spent = 0usize;
        for (i, q, axis, fraction) in seeds.into_iter().take(3) {
            let across = if axis == 0 {
                (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1])
            } else {
                (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1])
            };
            let along = if axis == 0 {
                (q[3][0] - q[0][0]).hypot(q[3][1] - q[0][1])
            } else {
                (q[1][0] - q[0][0]).hypot(q[1][1] - q[0][1])
            };
            if along < 4. {
                continue;
            }
            let expected = ((across * 1.3).ceil() as usize + 2).clamp(64, 4096);
            self.active = i;
            let old_cap = self.peak_counts[i];
            self.peak_counts[i] = 0;
            for offset in [-1., 1., -2., 2., -4., 4., -8., 8.] {
                if spent + expected > 16_384 || self.recovery_samples + expected > 65_536 {
                    self.capped = true;
                    break;
                }
                let f = fraction + offset / along;
                if !(0.0..=1.0).contains(&f) {
                    continue;
                }
                let Ok(Some(p)) = sampler.diagnostic_profile(im, q, axis, f, true) else {
                    continue;
                };
                if spent + p.len() > 16_384 || self.recovery_samples + p.len() > 65_536 {
                    self.capped = true;
                    break;
                }
                spent += p.len();
                self.recovery_paths += 1;
                self.recovery_samples += p.len();
                if multi_profile::sample_runs(&p, 64, &mut raw).is_err() {
                    continue;
                }
                let primary = multi_profile::decode_runs(&raw, 64);
                let before = self.groups[i].1.len();
                self.raw(&raw, &primary, axis, f, -0.15, 1.15, p.len());
                self.peaks(&p, &primary, axis, f, -0.15, 1.15);
                if self.groups[i].1.len() == before {
                    self.recovery_profile(&p, &primary, axis, f, -0.15, 1.15);
                }
            }
            self.peak_counts[i] = old_cap;
            let Ok(m) = crate::scan::transform(q) else {
                continue;
            };
            found.extend(crate::experiment::assemble_many_budget_options(
                im,
                m.0,
                &self.groups[i].1,
                work,
                true,
                budget,
                false,
            ));
        }
        found
    }
}
